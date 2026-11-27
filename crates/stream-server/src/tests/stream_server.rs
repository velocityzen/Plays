//! Serving the fixtures over HTTP, and reading back what comes out.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use ffmpeg::codec::Id;
use ffmpeg::ffi;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;

use crate::encode::Colors;
use crate::http;
use crate::{
    Adjustments, AudioCodec, AudioPlan, CastRequest, Container, Delivery, Error, MediaInfo, Pick,
    Profile, Segments, Server, Source, SubtitleSource, VideoCodec, VideoPlan,
};

/// This computer, as a device: on no port in particular.
const LOCALHOST: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A server with one-second segments, so the two-second fixtures have two.
fn server() -> Server {
    Server::start_with(LOCALHOST, 1.0).expect("the server starts")
}

fn cast(server: &Server, name: &str, profile: Profile) -> crate::Cast {
    server
        .add(
            CastRequest::new(Source::File(fixture(name)), profile),
            LOCALHOST,
        )
        .expect("the fixture casts")
}

/// A device that plays HLS of this kind and nothing as a file.
fn hls(segments: Segments) -> Profile {
    Profile {
        files: Vec::new(),
        hls: Some(segments),
        ..Profile::default()
    }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("text")
    }
}

/// Send raw request text to the server at `url`'s host; all it answers.
fn exchange(url: &str, request: &str) -> Vec<u8> {
    let host = url
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut stream = TcpStream::connect(host).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut all = Vec::new();
    stream.read_to_end(&mut all).unwrap();
    all
}

fn request(method: &str, url: &str, headers: &[(&str, &str)]) -> Reply {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let mut text = format!("{method} /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (name, value) in headers {
        text.push_str(&format!("{name}: {value}\r\n"));
    }
    text.push_str("\r\n");
    let all = exchange(url, &text);
    let split = all
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8(all[..split].to_vec()).unwrap();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
        .collect();
    Reply {
        status,
        headers,
        body: all[split + 4..].to_vec(),
    }
}

fn get(url: &str) -> Reply {
    request("GET", url, &[])
}

/// `url` with its last part replaced by `name`.
fn sibling(url: &str, name: &str) -> String {
    format!("{}{name}", &url[..=url.rfind('/').unwrap()])
}

struct Probed {
    codec: Id,
    width: u32,
    height: u32,
    /// Each packet's presentation time, in seconds, in the order read.
    times: Vec<f64>,
    keys: Vec<bool>,
}

impl Probed {
    fn first(&self) -> f64 {
        self.times.iter().copied().fold(f64::INFINITY, f64::min)
    }
}

/// `bytes` in a file of their own, to read back.
fn written(bytes: &[u8], extension: &str) -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "stream-server-{}-{}.{extension}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, bytes).unwrap();
    path
}

/// The video and sound streams of `bytes`, read as a file.
fn probe(bytes: &[u8], extension: &str) -> (Option<Probed>, Option<Probed>) {
    let path = written(bytes, extension);
    let mut input = ffmpeg_safe::input_file(&path).expect("the output opens");
    let mut streams: Vec<(usize, Type, Probed)> = input
        .streams()
        .map(|stream| {
            let parameters = stream.parameters();
            // SAFETY: the parameters belong to an open stream.
            let (width, height) = unsafe {
                let raw = &*parameters.as_ptr();
                (raw.width.max(0) as u32, raw.height.max(0) as u32)
            };
            (
                stream.index(),
                parameters.medium(),
                Probed {
                    codec: parameters.id(),
                    width,
                    height,
                    times: Vec::new(),
                    keys: Vec::new(),
                },
            )
        })
        .collect();
    let time_bases: Vec<f64> = input.streams().map(|s| f64::from(s.time_base())).collect();
    for (stream, packet) in input.packets() {
        let index = stream.index();
        if let Some((_, _, probed)) = streams.iter_mut().find(|(i, _, _)| *i == index)
            && let Some(pts) = packet.pts()
        {
            probed.times.push(pts as f64 * time_bases[index]);
            probed.keys.push(packet.is_key());
        }
    }
    drop(input);
    let _ = fs::remove_file(&path);
    let mut take = |kind: Type| {
        streams
            .iter()
            .position(|(_, medium, _)| *medium == kind)
            .map(|at| streams.remove(at).2)
    };
    let video = take(Type::Video);
    (video, take(Type::Audio))
}

/// Segment `number` as a device gets it (MP4 ones after the init segment),
/// read back: its video and its sound.
fn fetch(url: &str, kind: Segments, number: usize) -> (Probed, Option<Probed>) {
    let name = format!("{number}.{}", crate::playlist::extension(kind));
    let segment = get(&sibling(url, &name));
    assert_eq!(segment.status, 200, "{name}");
    let (mut bytes, extension) = match kind {
        Segments::Fmp4 => (get(&sibling(url, "init.mp4")).body, "mp4"),
        Segments::Ts => {
            assert!(
                starts_with_tables(&segment.body),
                "{name} starts with PAT and PMT"
            );
            (Vec::new(), "ts")
        }
    };
    bytes.extend(segment.body);
    let (video, audio) = probe(&bytes, extension);
    (video.expect("video in the segment"), audio)
}

/// Whether MPEG-TS starts with the tables that say what is in it, before
/// any of its streams: the program association table, the program's map,
/// and (as FFmpeg writes it) the service description.
fn starts_with_tables(bytes: &[u8]) -> bool {
    let tables: Vec<u8> = bytes
        .chunks(188)
        .take(3)
        .filter_map(|packet| {
            // A payload starting a section, without an adaptation field: a
            // pointer, then the table's id.
            let starts = packet.len() == 188 && packet[0] == 0x47 && packet[1] & 0x40 != 0;
            let pointer = usize::from(*packet.get(4)?);
            starts.then(|| packet.get(5 + pointer).copied())?
        })
        .collect();
    [0x00, 0x02, 0x42].iter().all(|id| tables.contains(id))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.05
}

/// A cast for the device at 127.0.0.2, and its URL as this computer,
/// which isn't it, reaches it.
fn cast_elsewhere(server: &Server) -> (crate::Cast, String) {
    let device = SocketAddr::from(([127, 0, 0, 2], 0));
    let elsewhere = server
        .add(
            CastRequest::new(Source::File(fixture("clip.mp4")), Profile::default()),
            device,
        )
        .expect("the fixture casts");
    let url = elsewhere.url().replace("127.0.0.2", "127.0.0.1");
    (elsewhere, url)
}

/// Served over IPv4 and IPv6 both, each cast to its own device alone.
#[test]
fn a_server_on_both_ip_versions_serves_devices_on_either() {
    let server = Server::start_with((Ipv6Addr::UNSPECIFIED, 0).into(), 1.0).unwrap();
    let on = |device: SocketAddr| {
        server
            .add(
                CastRequest::new(Source::File(fixture("clip.mp4")), Profile::default()),
                device,
            )
            .unwrap()
            .url()
    };
    let over_ipv4 = on(LOCALHOST);
    let over_ipv6 = on((Ipv6Addr::LOCALHOST, 0).into());
    assert!(over_ipv4.starts_with("http://127.0.0.1:"), "{over_ipv4}");
    assert!(over_ipv6.starts_with("http://[::1]:"), "{over_ipv6}");
    assert_eq!(get(&over_ipv4).status, 200);
    assert_eq!(get(&over_ipv6).status, 200);
    let port = server.port().to_string();
    let swapped = |url: &str, from: &str, to: &str| url.replacen(from, to, 1);
    assert_eq!(
        get(&swapped(
            &over_ipv4,
            &format!("127.0.0.1:{port}"),
            &format!("[::1]:{port}")
        ))
        .status,
        404,
        "not to the other's device"
    );
    assert_eq!(
        get(&swapped(
            &over_ipv6,
            &format!("[::1]:{port}"),
            &format!("127.0.0.1:{port}")
        ))
        .status,
        404
    );
}

#[test]
fn a_cast_is_served_only_to_its_device() {
    let server = server();
    let (_elsewhere, url) = cast_elsewhere(&server);
    // This computer, the device of a cast of its own, asks for the other.
    let here = cast(&server, "clip.mp4", Profile::default());
    assert_eq!(get(&here.url()).status, 200);
    assert_eq!(get(&url).status, 404);
}

/// A connection to `url`'s host asking for it, kept open.
fn asking(url: &str) -> TcpStream {
    let (host, path) = url
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    let mut stream = TcpStream::connect(host).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let ask = format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\n\r\n");
    // Closed already, it may not take this.
    let _ = stream.write_all(ask.as_bytes());
    stream
}

/// Whether the server closed `stream` without answering it.
fn closed_unanswered(mut stream: TcpStream) -> bool {
    let mut answer = Vec::new();
    let closed = match stream.read_to_end(&mut answer) {
        Ok(_) => true,
        Err(err) => !matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
    };
    closed && answer.is_empty()
}

/// Others on the network are closed on as they connect, unanswered: kept
/// open, they would take the places a device's connections need.
#[test]
fn others_on_the_network_take_none_of_the_connections() {
    let server = server();
    let (_elsewhere, url) = cast_elsewhere(&server);
    let others: Vec<_> = (0..=http::MAX_CONNECTIONS).map(|_| asking(&url)).collect();
    for other in others {
        assert!(closed_unanswered(other));
    }
    let here = cast(&server, "clip.mp4", Profile::default());
    assert_eq!(get(&here.url()).status, 200);
}

#[test]
fn serves_a_file_as_it_is_with_ranges() {
    let server = server();
    let cast = cast(&server, "clip.mp4", Profile::default());
    assert_eq!(cast.plan().delivery, Delivery::File);
    assert_eq!(cast.plan().describe(), "as is");
    assert_eq!(cast.mime(), "video/mp4");
    assert!(cast.url().ends_with("/media.mp4"), "{}", cast.url());
    assert_eq!(
        cast.protocol_info(),
        "http-get:*:video/mp4:DLNA.ORG_OP=01;DLNA.ORG_CI=0;\
         DLNA.ORG_FLAGS=01700000000000000000000000000000"
    );
    let file = fs::read(fixture("clip.mp4")).unwrap();

    let whole = get(&cast.url());
    assert_eq!(whole.status, 200);
    assert_eq!(whole.body, file);
    assert_eq!(
        whole.header("Content-Length"),
        Some(file.len().to_string().as_str())
    );
    assert_eq!(whole.header("Accept-Ranges"), Some("bytes"));
    assert_eq!(whole.header("Content-Type"), Some("video/mp4"));
    assert_eq!(whole.header("Access-Control-Allow-Origin"), Some("*"));
    assert_eq!(whole.header("transferMode.dlna.org"), Some("Streaming"));
    assert!(
        whole
            .header("contentFeatures.dlna.org")
            .unwrap()
            .starts_with("DLNA.ORG_OP=01")
    );

    let part = request("GET", &cast.url(), &[("Range", "bytes=10-19")]);
    assert_eq!(part.status, 206);
    assert_eq!(part.body, file[10..20]);
    assert_eq!(
        part.header("Content-Range"),
        Some(format!("bytes 10-19/{}", file.len()).as_str())
    );
    let tail = request("GET", &cast.url(), &[("Range", "bytes=-5")]);
    assert_eq!(tail.body, file[file.len() - 5..]);
    let past = request("GET", &cast.url(), &[("Range", "bytes=999999-")]);
    assert_eq!(past.status, 416);
    assert_eq!(
        past.header("Content-Range"),
        Some(format!("bytes */{}", file.len()).as_str())
    );

    let head = request("HEAD", &cast.url(), &[]);
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(
        head.header("Content-Length"),
        Some(file.len().to_string().as_str())
    );

    let options = request("OPTIONS", &cast.url(), &[]);
    assert_eq!(options.status, 204);
    assert!(
        options
            .header("Access-Control-Allow-Methods")
            .unwrap()
            .contains("GET")
    );

    // Nothing but what was cast.
    assert_eq!(get(&sibling(&cast.url(), "master.m3u8")).status, 404);
    let token = cast.url().split('/').nth(3).unwrap().to_owned();
    assert_eq!(token.len(), 32);
    let elsewhere = cast
        .url()
        .replace(&token, "0123456789abcdef0123456789abcdef");
    assert_eq!(get(&elsewhere).status, 404);
    assert_eq!(request("POST", &cast.url(), &[]).status, 405);
}

#[test]
fn keeps_a_connection_open_between_requests() {
    let server = server();
    let cast = cast(&server, "clip.mp4", Profile::default());
    let path = cast
        .url()
        .split_once("//")
        .unwrap()
        .1
        .split_once('/')
        .unwrap()
        .1
        .to_owned();
    let all = exchange(
        &cast.url(),
        &format!(
            "HEAD /{path} HTTP/1.1\r\nHost: x\r\n\r\nGET /{path} HTTP/1.1\r\nHost: x\r\n\
             Range: bytes=0-3\r\nConnection: close\r\n\r\n"
        ),
    );
    let text = String::from_utf8_lossy(&all);
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
    assert!(text.contains("HTTP/1.1 206 Partial Content\r\n"), "{text}");
}

#[test]
fn stops_serving_a_cast_once_removed() {
    let server = server();
    let removed = cast(&server, "clip.mp4", Profile::default());
    let kept = cast(&server, "tone.m4a", Profile::default());
    server.remove(&removed);
    assert_eq!(server.casts().len(), 1);
    // Its device, casting something else, finds nothing there.
    assert_eq!(get(&removed.url()).status, 404);
    // Casting nothing, it isn't let in.
    server.remove(&kept);
    assert!(server.casts().is_empty());
    assert!(closed_unanswered(asking(&kept.url())));
}

#[test]
fn plans_the_least_work_that_plays() {
    let server = server();
    let mkv = cast(&server, "clip-ip.mkv", Profile::default());
    assert_eq!(mkv.plan().delivery, Delivery::Hls(Segments::Ts));
    assert_eq!(mkv.plan().describe(), "repackaged");
    assert_eq!(mkv.mime(), "application/vnd.apple.mpegurl");
    assert!(mkv.url().ends_with("/master.m3u8"));

    let tone = cast(&server, "tone.m4a", Profile::default());
    assert_eq!(tone.plan().describe(), "as is");
    assert_eq!(tone.mime(), "audio/mp4");
    assert!(tone.plan().video.is_none());

    // A device without AAC gets the sound as AC-3, the video as it is.
    let ac3 = Profile {
        audio: vec![AudioCodec::Ac3],
        ..Profile::default()
    };
    let sound = cast(&server, "clip.mp4", ac3);
    assert_eq!(sound.plan().delivery, Delivery::Hls(Segments::Ts));
    assert!(matches!(sound.plan().video, Some(VideoPlan::Copy { .. })));
    assert!(matches!(
        sound.plan().audio,
        Some(AudioPlan::Encode {
            to: AudioCodec::Ac3,
            channels: 1,
            sample_rate: 48_000,
            ..
        })
    ));
    assert_eq!(
        sound.plan().describe(),
        "repackaged, sound re-encoded to AC-3"
    );

    // Without sound, when asked.
    let quiet = server
        .add(
            CastRequest {
                audio: Pick::Off,
                ..CastRequest::new(Source::File(fixture("clip.mp4")), Profile::default())
            },
            LOCALHOST,
        )
        .unwrap();
    assert!(quiet.plan().audio.is_none());
    assert_eq!(quiet.plan().delivery, Delivery::Hls(Segments::Ts));

    // A device without H.264 needs the hardware encoder: there is one on
    // every Mac, and none in the Linux test container.
    let hevc = Profile {
        video: vec![VideoCodec::Hevc],
        ..Profile::default()
    };
    let result = server.add(
        CastRequest::new(Source::File(fixture("clip.mp4")), hevc),
        LOCALHOST,
    );
    if cfg!(target_os = "macos") {
        let cast = result.expect("VideoToolbox encodes HEVC");
        assert!(matches!(
            cast.plan().video,
            Some(VideoPlan::Encode {
                to: VideoCodec::Hevc,
                encoder: "hevc_videotoolbox",
                ..
            })
        ));
        assert_eq!(cast.plan().describe(), "re-encoded to HEVC");
    } else {
        assert!(matches!(result, Err(Error::NoEncoder)), "{result:?}");
    }

    let nothing = Profile {
        hls: None,
        ..Profile::default()
    };
    assert!(matches!(
        server.add(
            CastRequest::new(Source::File(fixture("clip-ip.mkv")), nothing),
            LOCALHOST
        ),
        Err(Error::Unsupported(_))
    ));
}

/// The segments a media playlist lists.
fn segment_names(playlist: &str) -> Vec<String> {
    playlist
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn makes_hls_of_mpeg_ts_segments() {
    let server = server();
    let cast = cast(&server, "clip-ip.mkv", Profile::default());
    let master = get(&cast.url());
    assert_eq!(master.status, 200);
    assert_eq!(
        master.header("Content-Type"),
        Some("application/vnd.apple.mpegurl")
    );
    let master = master.text();
    assert!(master.contains("#EXT-X-STREAM-INF:BANDWIDTH="), "{master}");
    assert!(master.contains("CODECS=\"avc1.") && master.contains("RESOLUTION=160x120"));
    assert!(!master.contains("SUBTITLES"));

    let playlist = get(&sibling(&cast.url(), "video.m3u8")).text();
    assert!(
        playlist.contains("#EXT-X-PLAYLIST-TYPE:VOD") && playlist.ends_with("#EXT-X-ENDLIST\n")
    );
    assert!(
        playlist.contains("#EXTINF:1.000,\n0.ts\n#EXTINF:1.000,\n1.ts\n"),
        "{playlist}"
    );

    let mut all = Vec::new();
    for (number, name) in segment_names(&playlist).iter().enumerate() {
        let segment = get(&sibling(&cast.url(), name));
        assert_eq!(segment.status, 200, "{name}");
        assert_eq!(segment.header("Content-Type"), Some("video/mp2t"));
        assert!(
            starts_with_tables(&segment.body),
            "{name} starts with PAT and PMT"
        );
        let (video, audio) = probe(&segment.body, "ts");
        let video = video.expect("video in every segment");
        assert!(audio.is_none());
        assert_eq!(
            (video.codec, video.width, video.height),
            (Id::H264, 160, 120)
        );
        assert_eq!(video.times.len(), 25, "{name}");
        assert!(video.keys[0], "{name} starts with a keyframe");
        // Later by the muxer's offset: ten seconds.
        assert!(
            close(video.first(), 10.0 + number as f64),
            "{name}: {}",
            video.first()
        );
        all.extend(segment.body);
    }
    let (video, _) = probe(&all, "ts");
    assert_eq!(video.unwrap().times.len(), 50);
    assert_eq!(get(&sibling(&cast.url(), "2.ts")).status, 404);
}

#[test]
fn makes_hls_of_fragmented_mp4() {
    let server = server();
    let cast = cast(&server, "clip.mp4", hls(Segments::Fmp4));
    assert_eq!(cast.plan().describe(), "repackaged");
    let master = get(&cast.url()).text();
    assert!(
        master.contains("CODECS=\"avc1.") && master.contains(",mp4a.40.2\""),
        "{master}"
    );
    let playlist = get(&sibling(&cast.url(), "video.m3u8")).text();
    assert!(playlist.contains("#EXT-X-MAP:URI=\"init.mp4\""));
    let names = segment_names(&playlist);
    assert_eq!(names, ["0.m4s", "1.m4s"]);

    let init = get(&sibling(&cast.url(), "init.mp4"));
    assert_eq!(init.status, 200);
    assert_eq!(&init.body[4..8], b"ftyp");
    let mut all = init.body.clone();
    let mut frames = 0;
    let mut sound = 0.0;
    for name in &names {
        let segment = get(&sibling(&cast.url(), name));
        assert_eq!(&segment.body[4..8], b"moof", "{name} is a fragment");
        let mut alone = init.body.clone();
        alone.extend(&segment.body);
        let (video, audio) = probe(&alone, "mp4");
        let (video, audio) = (video.unwrap(), audio.unwrap());
        assert_eq!(video.codec, Id::H264);
        assert_eq!(audio.codec, Id::AAC);
        assert!(video.keys[0], "{name} starts with a keyframe");
        frames += video.times.len();
        sound += audio.times.len() as f64 * 1024.0 / 48_000.0;
        all.extend(segment.body);
    }
    assert_eq!(frames, 50);
    assert!((sound - 2.0).abs() < 0.1, "{sound} s of sound");
    let (video, audio) = probe(&all, "mp4");
    let video = video.unwrap();
    assert!(close(video.first(), 10.0), "{}", video.first());
    assert!(audio.unwrap().first() >= 9.9);
}

/// AC-3 and E-AC-3, most films' sound, go into MP4 segments as they are;
/// the init segment says what is in them, which only a packet tells.
#[test]
fn makes_hls_of_fragmented_mp4_with_ac3_and_eac3_sound() {
    let server = server();
    let surround = Profile {
        audio: vec![AudioCodec::Aac, AudioCodec::Ac3, AudioCodec::Eac3],
        max_channels: 8,
        ..hls(Segments::Fmp4)
    };
    for (name, codec, id, sample_entry) in [
        ("clip-sound-ac3.mkv", "ac-3", Id::AC3, b"dac3"),
        ("clip-sound-eac3.mkv", "ec-3", Id::EAC3, b"dec3"),
    ] {
        let cast = cast(&server, name, surround.clone());
        assert_eq!(cast.plan().describe(), "repackaged", "{name}");
        let master = get(&cast.url()).text();
        assert!(master.contains(&format!(",{codec}\"")), "{master}");
        let init = get(&sibling(&cast.url(), "init.mp4"));
        assert_eq!(init.status, 200, "{name}'s init segment");
        assert_eq!(&init.body[4..8], b"ftyp");
        assert!(
            init.body.windows(4).any(|bytes| bytes == sample_entry),
            "{name}'s init segment says what its sound is"
        );
        assert!(
            !init.body.windows(4).any(|bytes| bytes == b"moof"),
            "{name}'s init segment has no fragment in it"
        );
        for number in 0..2 {
            let segment = get(&sibling(&cast.url(), &format!("{number}.m4s")));
            assert_eq!(
                &segment.body[4..8],
                b"moof",
                "{name}: {number} is a fragment"
            );
            let (video, audio) = fetch(&cast.url(), Segments::Fmp4, number);
            assert_eq!(video.codec, Id::H264);
            assert!(video.keys[0], "{name}: {number} starts with a keyframe");
            let audio = audio.expect("sound");
            assert_eq!(audio.codec, id, "{name}");
            assert!(!audio.times.is_empty(), "{name}: sound in {number}");
        }
    }
}

#[test]
fn seeks_into_the_middle() {
    for kind in [Segments::Ts, Segments::Fmp4] {
        // Asked for first, the second segment is made by a job starting
        // there; it holds what the one made from the start does.
        let (expected_video, expected_audio) = {
            let server = server();
            let cast = cast(&server, "clip.mp4", hls(kind));
            fetch(&cast.url(), kind, 0);
            fetch(&cast.url(), kind, 1)
        };
        let server = server();
        let cast = cast(&server, "clip.mp4", hls(kind));
        let (video, audio) = fetch(&cast.url(), kind, 1);
        let audio = audio.unwrap();
        assert!(video.keys[0], "{kind:?}");
        assert!(close(video.first(), 11.0), "{kind:?}: {}", video.first());
        assert_eq!(video.times, expected_video.times, "{kind:?}");
        assert_eq!(audio.times, expected_audio.unwrap().times, "{kind:?}");
        // And back to the start, by another job.
        let (video, _) = fetch(&cast.url(), kind, 0);
        assert!(close(video.first(), 10.0), "{kind:?}");
    }
}

#[test]
fn re_encodes_sound_the_device_lacks() {
    let server = server();
    let profile = Profile {
        audio: vec![AudioCodec::Ac3],
        ..Profile::default()
    };
    let cast = cast(&server, "clip.mp4", profile);
    let master = get(&cast.url()).text();
    assert!(master.contains(",ac-3\""), "{master}");
    let mut all = Vec::new();
    for name in ["0.ts", "1.ts"] {
        let segment = get(&sibling(&cast.url(), name));
        assert_eq!(segment.status, 200);
        all.extend(segment.body);
    }
    let (video, audio) = probe(&all, "ts");
    assert_eq!(video.unwrap().times.len(), 50);
    let audio = audio.unwrap();
    assert_eq!(audio.codec, Id::AC3);
    // AC-3 frames are 1536 samples.
    let seconds = audio.times.len() as f64 * 1536.0 / 48_000.0;
    assert!((seconds - 2.0).abs() < 0.1, "{seconds} s of sound");
    assert!(audio.first() >= 9.9, "{}", audio.first());
}

/// A gap in the source's sound stays a gap when the sound is re-encoded:
/// the sound after it plays with the picture it went with, not early, the
/// same whichever job makes its segment. A job from the start puts in that
/// segment, too, what it made after cutting the one before: the sound's
/// last frames, which the encoder held, and the silence over the gap, from
/// where that segment's sound stops.
#[test]
fn re_encoded_sound_keeps_to_the_picture_over_a_gap_in_it() {
    // AAC frames are 1024 samples.
    let frame = 1024.0 / 48_000.0;
    // When a segment's sound from `start` on starts and ends.
    let span = |audio: &Probed, start: f64| {
        let times = audio
            .times
            .iter()
            .copied()
            .filter(|&time| time > start - frame);
        let first = times.clone().fold(f64::INFINITY, f64::min);
        (first, times.fold(f64::NEG_INFINITY, f64::max) + frame)
    };
    let gap = |server: &Server| cast(server, "clip-sound-gap.mkv", hls(Segments::Ts));

    let whole = server();
    let cast = gap(&whole);
    assert!(
        matches!(
            cast.plan().audio,
            Some(AudioPlan::Encode {
                to: AudioCodec::Aac,
                ..
            })
        ),
        "{:?}",
        cast.plan()
    );
    let first = fetch(&cast.url(), Segments::Ts, 0).1.expect("sound");
    let (from, stops) = span(&first, 10.0);
    assert!(close(from, 10.0), "the sound starts at {from}");
    // The sound goes on at 1 s, with the second segment, to its end.
    let second = fetch(&cast.url(), Segments::Ts, 1).1.expect("sound");
    let (from, to) = span(&second, 11.0);
    assert!(
        close(from, 11.0) && close(to, 12.0),
        "sound from {from} to {to}"
    );
    let late = second.first();
    assert!(
        close(late, stops),
        "from {late}, where the first stops: {stops}"
    );

    let middle = server();
    let again = fetch(&gap(&middle).url(), Segments::Ts, 1)
        .1
        .expect("sound");
    let (again, until) = span(&again, 11.0);
    assert!(
        close(again, from) && close(until, to),
        "sound from {again} to {until}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn re_encodes_video_whose_keyframes_are_too_far_apart_to_cut_it_at() {
    let server = server();
    // Its only keyframe is its first: copied, its six seconds would be one
    // segment, all of it held at once.
    let cast = cast(&server, "clip-one-key.mp4", hls(Segments::Ts));
    assert!(
        matches!(cast.plan().video, Some(VideoPlan::Encode { .. })),
        "{:?}",
        cast.plan().video
    );
    assert_eq!(cast.plan().describe(), "re-encoded to H.264");
    let playlist = get(&sibling(&cast.url(), "video.m3u8")).text();
    assert_eq!(segment_names(&playlist).len(), 6, "{playlist}");
    let (video, _) = fetch(&cast.url(), Segments::Ts, 3);
    assert!(video.keys[0], "it starts with a keyframe");
    assert_eq!(video.times.len(), 25);
    // Keyframes a second apart are cut at as they are.
    let near = self::cast(&server, "clip.mp4", hls(Segments::Ts));
    assert!(matches!(near.plan().video, Some(VideoPlan::Copy { .. })));
}

#[cfg(target_os = "macos")]
#[test]
fn re_encodes_video_on_the_hardware_encoder() {
    // Too big for the device, so it is made smaller.
    let profile = |kind| Profile {
        max_size: Some((80, 60)),
        ..hls(kind)
    };
    for kind in [Segments::Ts, Segments::Fmp4] {
        let server = server();
        let cast = cast(&server, "clip.mp4", profile(kind));
        assert!(matches!(
            cast.plan().video,
            Some(VideoPlan::Encode {
                encoder: "h264_videotoolbox",
                width: 80,
                height: 60,
                ..
            })
        ));
        let master = get(&cast.url()).text();
        assert!(
            master.contains("RESOLUTION=80x60") && master.contains("CODECS=\"avc1."),
            "{master}"
        );
        let mut frames = 0;
        let mut segments = Vec::new();
        for number in 0..2 {
            let (video, audio) = fetch(&cast.url(), kind, number);
            let audio = audio.expect("sound in every segment");
            let start = 10.0 + number as f64;
            assert_eq!((video.codec, video.width, video.height), (Id::H264, 80, 60));
            assert!(video.keys[0], "{kind:?} {number} starts with a keyframe");
            assert!(
                close(video.first(), start),
                "{kind:?} {number}: {}",
                video.first()
            );
            // The encoder hands frames back late; the sound in a segment is
            // still only what plays during it.
            assert_eq!(audio.codec, Id::AAC);
            assert!(
                audio
                    .times
                    .iter()
                    .all(|time| *time >= start - 0.03 && *time < start + 1.0),
                "{kind:?} {number}: sound at {:?}",
                audio.times
            );
            frames += video.times.len();
            segments.push((video, audio));
        }
        assert_eq!(frames, 50, "{kind:?}");
        // Made by a job starting there, the second segment is the same.
        let fresh = self::server();
        let middle = self::cast(&fresh, "clip.mp4", profile(kind));
        let (video, audio) = fetch(&middle.url(), kind, 1);
        assert_eq!(video.times, segments[1].0.times, "{kind:?}");
        let audio = audio.unwrap();
        assert_eq!(audio.times.first(), segments[1].1.times.first(), "{kind:?}");
    }
}

/// FFmpeg hands colors over as C enums, and a file can hold numbers they
/// have no names for: those are read as unknown, never as a Rust enum.
#[test]
fn reads_colors_ffmpeg_has_no_names_for_as_unknown() {
    use ffi::AVColorPrimaries::AVCOL_PRI_BT2020;
    use ffi::AVColorSpace::AVCOL_SPC_BT2020_NCL;
    use ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE2084;

    let source = Source::File(fixture("clip-odd-colors.mkv"));
    let input = crate::media::open(&source, Arc::default()).expect("the fixture opens");
    let stream = input.streams().best(Type::Video).expect("a picture");
    let colors = Colors::of(&stream.parameters());
    assert_eq!(
        (colors.primaries, colors.transfer, colors.space),
        (None, None, None)
    );
    let media = MediaInfo::read(&input, "clip-odd-colors.mkv");
    let video = media.video(stream.index()).expect("a picture");
    assert_eq!((video.bit_depth, video.hdr), (10, None));

    // Ones it names are kept.
    let mut parameters = ffmpeg::codec::Parameters::new();
    // SAFETY: the parameters are FFmpeg's own, and these are values it names.
    unsafe {
        let raw = parameters.as_mut_ptr();
        (*raw).color_primaries = AVCOL_PRI_BT2020;
        (*raw).color_trc = AVCOL_TRC_SMPTE2084;
        (*raw).color_space = AVCOL_SPC_BT2020_NCL;
    }
    let colors = Colors::of(&parameters);
    assert_eq!(
        (colors.primaries, colors.transfer, colors.space),
        (
            Some(AVCOL_PRI_BT2020),
            Some(AVCOL_TRC_SMPTE2084),
            Some(AVCOL_SPC_BT2020_NCL)
        )
    );
}

#[cfg(target_os = "macos")]
#[test]
fn re_encodes_video_whose_colors_ffmpeg_has_no_names_for() {
    let server = server();
    let profile = Profile {
        video: vec![VideoCodec::Hevc],
        max_bit_depth: 10,
        ..hls(Segments::Fmp4)
    };
    let cast = cast(&server, "clip-odd-colors.mkv", profile);
    assert!(
        matches!(
            cast.plan().video,
            Some(VideoPlan::Encode {
                to: VideoCodec::Hevc,
                ten_bit: true,
                hdr: None,
                ..
            })
        ),
        "{:?}",
        cast.plan()
    );
    let mut frames = 0;
    for number in 0..2 {
        let (video, _) = fetch(&cast.url(), Segments::Fmp4, number);
        assert_eq!(
            (video.codec, video.width, video.height),
            (Id::HEVC, 160, 120)
        );
        frames += video.times.len();
    }
    assert_eq!(frames, 50);
}

/// HDR10 cast to a device that isn't given HDR comes fitted to SDR: tagged
/// BT.709, its reference white shown nearly white, not as the dim grey
/// PQ's code for it is taken for BT.709's (143 of 235).
#[cfg(target_os = "macos")]
#[test]
fn fits_hdr_to_sdr_for_a_device_not_given_it() {
    let server = server();
    let cast = cast(&server, "clip-hdr10.mkv", hls(Segments::Ts));
    assert!(
        matches!(
            cast.plan().video,
            Some(VideoPlan::Encode {
                to: VideoCodec::H264,
                ten_bit: false,
                hdr: None,
                ..
            })
        ),
        "{:?}",
        cast.plan()
    );
    let frame = first_frame(&cast, Segments::Ts);
    assert_eq!(crate::hdr::Tags::of(&frame), crate::hdr::tests::sdr_tags());
    let (data, stride) = ffmpeg_safe::plane(&frame, 0).unwrap();
    let luma = data[stride * 60 + 80];
    assert!(luma > 210, "{luma}");
}

/// The first picture of `cast`'s first segment of `kind`, decoded.
fn first_frame(cast: &crate::Cast, kind: Segments) -> ffmpeg::frame::Video {
    let name = format!("0.{}", crate::playlist::extension(kind));
    let segment = get(&sibling(&cast.url(), &name));
    assert_eq!(segment.status, 200);
    let (mut bytes, extension) = match kind {
        Segments::Fmp4 => (get(&sibling(&cast.url(), "init.mp4")).body, "mp4"),
        Segments::Ts => (Vec::new(), "ts"),
    };
    bytes.extend(segment.body);
    let path = written(&bytes, extension);
    let mut input = ffmpeg_safe::input_file(&path).expect("the segment opens");
    let stream = input.streams().best(Type::Video).expect("video");
    let index = stream.index();
    let mut decoder = ffmpeg::codec::Context::from_parameters(stream.parameters())
        .unwrap()
        .decoder()
        .video()
        .unwrap();
    let mut frame = ffmpeg::frame::Video::empty();
    let decoded = input
        .packets()
        .filter(|(stream, _)| stream.index() == index)
        .any(|(_, packet)| {
            decoder.send_packet(&packet).unwrap();
            decoder.receive_frame(&mut frame).is_ok()
        });
    drop(input);
    let _ = fs::remove_file(&path);
    assert!(decoded, "a frame");
    frame
}

/// Dolby Vision in IPT, profile 20 here, shows green and purple without
/// its RPU, so a device it's repackaged for gets it reshaped: HDR10 for an
/// Apple TV 4K, which shows that, its master playlist saying PQ, SDR for
/// one that doesn't. The picture's left half is a skin tone, redder than
/// it is blue, and its right half green; their IPT taken for YCbCr would
/// be green and magenta.
#[test]
fn reshapes_dolby_visions_ipt_for_a_device() {
    use crate::Hdr;
    use crate::hdr::Tags;
    let server = server();
    let apple_tv = Profile {
        video: vec![VideoCodec::H264, VideoCodec::Hevc],
        max_bit_depth: 10,
        hdr: vec![Hdr::Hdr10, Hdr::Hlg, Hdr::DolbyVision],
        ..hls(Segments::Fmp4)
    };
    for (profile, kind, hdr) in [
        (apple_tv, Segments::Fmp4, Some(Hdr::Hdr10)),
        (hls(Segments::Ts), Segments::Ts, None),
    ] {
        let cast = cast(&server, "clip-dv-ipt.mp4", profile);
        let plan = &cast.plan().video;
        assert!(
            matches!(plan, Some(VideoPlan::Encode { hdr: planned, .. }) if *planned == hdr),
            "{plan:?}"
        );
        let master = get(&cast.url()).text();
        assert_eq!(
            master.contains(",VIDEO-RANGE=PQ,"),
            hdr.is_some(),
            "{master}"
        );
        let frame = first_frame(&cast, kind);
        let tags = Tags::of(&frame);
        if hdr.is_some() {
            assert_eq!(
                tags,
                Tags {
                    primaries: ffi::AVColorPrimaries::AVCOL_PRI_BT2020 as u32,
                    transfer: ffi::AVColorTransferCharacteristic::AVCOL_TRC_SMPTE2084 as u32,
                    space: ffi::AVColorSpace::AVCOL_SPC_BT2020_NCL as u32,
                    range: ffi::AVColorRange::AVCOL_RANGE_MPEG as u32,
                }
            );
        } else {
            assert_eq!(tags, crate::hdr::tests::sdr_tags());
        }
        // Luma (0 to 1), and blue and red differences (-0.5 to 0.5),
        // mid-way down at `x`.
        let sample = |x: usize| {
            let y = frame.height() as usize / 2;
            [(0, x, y), (1, x / 2, y / 2), (2, x / 2, y / 2)].map(|(plane, x, y)| {
                let (data, stride) = ffmpeg_safe::plane(&frame, plane).unwrap();
                let at = stride * y;
                let (offset, range) = match plane {
                    0 => (64.0, 876.0),
                    _ => (512.0, 896.0),
                };
                if hdr.is_some() {
                    let data = &data[at + 2 * x..];
                    (f64::from(u16::from_le_bytes([data[0], data[1]])) - offset) / range
                } else {
                    (f64::from(data[at + x]) - offset / 4.0) / (range / 4.0)
                }
            })
        };
        // Worked out apart from this code, from the colors the halves were
        // made of, BT.709's (60, 40, 30) and (0, 50, 0) nits: through
        // BT.2087, ST 2084 and BT.2020's matrix for HDR10, through BT.1886
        // and BT.709's matrix for SDR, below where the curve rolls off.
        // SDR's 8 bits, at this size, take more of a step.
        let (halves, close) = if hdr.is_some() {
            (
                [[0.4265, -0.0157, 0.012], [0.3967, -0.0839, -0.0382]],
                0.005,
            )
        } else {
            ([[0.524, -0.0394, 0.0494], [0.3989, -0.215, -0.2533]], 0.03)
        };
        let width = frame.width() as usize;
        for (x, want) in [width / 4, width * 3 / 4].into_iter().zip(halves) {
            let got = sample(x);
            assert!(
                want.iter().zip(got).all(|(a, b)| (a - b).abs() < close),
                "{hdr:?} at {x}: want {want:?}, got {got:?}"
            );
        }
    }
}

/// A read failing partway through a file, as a network share's might: the
/// segments past it aren't served empty, and are read again when asked for.
#[test]
fn a_read_failing_partway_through_a_file_is_tried_again() {
    let dir = std::env::temp_dir().join("stream-server-tests");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("read-fails.mkv");
    fs::copy(fixture("clip-ip.mkv"), &path).unwrap();
    // Ten frames into the first segment.
    let location = path.to_string_lossy().into_owned();
    crate::job::BREAKS.lock().unwrap().push((location, 10));
    let server = server();
    let cast = server
        .add(
            CastRequest::new(Source::File(path), hls(Segments::Ts)),
            LOCALHOST,
        )
        .expect("the file casts");
    // The job asked for it failed, so the request does.
    assert_eq!(get(&sibling(&cast.url(), "0.ts")).status, 500);
    // The next is made by a job of its own, whole; so is the first, asked
    // for again.
    let (second, _) = fetch(&cast.url(), Segments::Ts, 1);
    assert_eq!(second.times.len(), 25);
    assert!(close(second.first(), 11.0), "{}", second.first());
    let (first, _) = fetch(&cast.url(), Segments::Ts, 0);
    assert_eq!(first.times.len(), 25);
}

#[test]
fn streams_mpeg_ts_from_where_asked() {
    let server = server();
    let profile = Profile {
        files: Vec::new(),
        hls: None,
        stream: true,
        ..Profile::default()
    };
    let cast = cast(&server, "clip.mp4", profile);
    assert_eq!(cast.plan().delivery, Delivery::Stream);
    assert_eq!(cast.mime(), "video/mp2t");
    assert!(cast.url().ends_with("/stream.ts"));
    assert!(
        cast.protocol_info()
            .contains("DLNA.ORG_OP=10;DLNA.ORG_CI=1")
    );

    let whole = get(&cast.url());
    assert_eq!(whole.status, 200);
    assert_eq!(whole.header("Content-Length"), None);
    assert_eq!(whole.header("Connection"), Some("close"));
    let (video, audio) = probe(&whole.body, "ts");
    let (video, audio) = (video.unwrap(), audio.unwrap());
    assert_eq!(video.times.len(), 50);
    assert_eq!(audio.codec, Id::AAC);
    assert!(close(video.first(), 10.0));

    let later = request(
        "GET",
        &cast.url(),
        &[("TimeSeekRange.dlna.org", "npt=1.0-")],
    );
    assert_eq!(
        later.header("TimeSeekRange.dlna.org"),
        Some("npt=1.000-2.000/2.000")
    );
    let (video, audio) = probe(&later.body, "ts");
    let video = video.unwrap();
    assert!(video.keys[0]);
    assert!(close(video.first(), 11.0), "{}", video.first());
    assert_eq!(video.times.len(), 25);
    assert!(audio.unwrap().first() >= 10.95);

    let query = get(&format!("{}?start=1", cast.url()));
    let (video, _) = probe(&query.body, "ts");
    assert!(close(video.unwrap().first(), 11.0));

    let head = request("HEAD", &cast.url(), &[]);
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
}

#[test]
fn streams_a_videos_sound_alone_to_a_speaker_taking_only_files() {
    let server = server();
    // Devialet's take AAC and FLAC; Sonos's FLAC and no AAC.
    let speaker = Profile {
        files: vec![Container::Aac, Container::Flac],
        hls: None,
        sound_stream: vec![Container::Aac, Container::Flac],
        audio: vec![AudioCodec::Aac, AudioCodec::Flac],
        ..Profile::default()
    };
    let flac_only = Profile {
        files: vec![Container::Flac],
        sound_stream: vec![Container::Flac],
        ..speaker.clone()
    };
    let sound_of = |name: &str, profile: Profile| {
        let mut request = CastRequest::new(Source::File(fixture(name)), profile);
        request.video = Pick::Off;
        server.add(request, LOCALHOST)
    };

    // AAC goes as it is, with ADTS headers.
    let cast = sound_of("clip.mp4", speaker.clone()).unwrap();
    assert_eq!(cast.plan().delivery, Delivery::Sound(Container::Aac));
    assert_eq!(cast.plan().video, None);
    assert!(matches!(cast.plan().audio, Some(AudioPlan::Copy { .. })));
    assert_eq!(cast.mime(), "audio/aac");
    assert!(cast.url().ends_with("/sound.aac"));
    assert!(
        cast.protocol_info()
            .contains("DLNA.ORG_OP=10;DLNA.ORG_CI=1")
    );
    let whole = get(&cast.url());
    assert_eq!(whole.status, 200);
    assert_eq!(whole.header("Content-Type"), Some("audio/aac"));
    assert_eq!(whole.header("Content-Length"), None);
    assert!(
        whole.body[0] == 0xFF && whole.body[1] & 0xF0 == 0xF0,
        "it starts with an ADTS header"
    );
    let (video, audio) = probe(&whole.body, "aac");
    assert!(video.is_none());
    let audio = audio.unwrap();
    assert_eq!(audio.codec, Id::AAC);
    let all = audio.times.len();
    // Two seconds of 1024-sample frames at 48 kHz.
    assert!((90..=97).contains(&all), "{all} frames");

    // From a DLNA time seek, the second half.
    let later = request(
        "GET",
        &cast.url(),
        &[("TimeSeekRange.dlna.org", "npt=1.0-")],
    );
    assert_eq!(
        later.header("TimeSeekRange.dlna.org"),
        Some("npt=1.000-2.000/2.000")
    );
    let half = probe(&later.body, "aac").1.unwrap().times.len();
    assert!(half.abs_diff(all / 2) <= 3, "{half} of {all} frames");

    // Other sound is re-encoded, to the first it takes.
    let cast = sound_of("clip-opus.mkv", speaker.clone()).unwrap();
    assert!(matches!(
        cast.plan().audio,
        Some(AudioPlan::Encode {
            to: AudioCodec::Aac,
            ..
        })
    ));
    let (_, audio) = probe(&get(&cast.url()).body, "aac");
    let audio = audio.unwrap();
    assert_eq!(audio.codec, Id::AAC);
    assert!(audio.times.len() >= 90, "{} frames", audio.times.len());

    // To FLAC for one that takes no AAC, the whole of it.
    let cast = sound_of("clip.mp4", flac_only.clone()).unwrap();
    assert_eq!(cast.plan().delivery, Delivery::Sound(Container::Flac));
    assert_eq!(cast.mime(), "audio/flac");
    assert!(cast.url().ends_with("/sound.flac"));
    let body = get(&cast.url()).body;
    assert_eq!(&body[..4], b"fLaC");
    let (_, audio) = probe(&body, "flac");
    let audio = audio.unwrap();
    assert_eq!(audio.codec, Id::FLAC);
    assert!(
        *audio.times.last().unwrap() > 1.8,
        "to the end: {:?}",
        audio.times.last()
    );

    // The picture and the sound together don't go this way, nor does the
    // sound to a speaker that doesn't take a stream of it.
    let mut both = CastRequest::new(Source::File(fixture("clip.mp4")), speaker.clone());
    both.video = Pick::Best;
    assert!(matches!(
        server.add(both, LOCALHOST),
        Err(Error::Unsupported(_))
    ));
    let files_only = Profile {
        sound_stream: Vec::new(),
        ..speaker
    };
    assert!(matches!(
        sound_of("clip.mp4", files_only),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn serves_a_live_stream_as_it_comes() {
    let server = server();
    let source = Source::Stream {
        url: fixture("clip-ip.mkv").to_string_lossy().into_owned(),
        live: true,
    };
    let cast = server
        .add(CastRequest::new(source, Profile::default()), LOCALHOST)
        .unwrap();
    assert_eq!(cast.plan().delivery, Delivery::Hls(Segments::Ts));
    let playlist = get(&sibling(&cast.url(), "video.m3u8")).text();
    assert!(playlist.contains("#EXT-X-MEDIA-SEQUENCE:0"), "{playlist}");
    assert!(!playlist.contains("#EXT-X-PLAYLIST-TYPE"));
    let names = segment_names(&playlist);
    assert_eq!(names, ["0.ts", "1.ts"]);
    for (number, name) in names.iter().enumerate() {
        let (video, _) = probe(&get(&sibling(&cast.url(), name)).body, "ts");
        let video = video.unwrap();
        assert!(video.keys[0]);
        assert!(close(video.first(), 10.0 + number as f64));
    }
    assert_eq!(get(&sibling(&cast.url(), "7.ts")).status, 500);
}

/// A live stream to a device taking MP4 segments, its sound AC-3: the
/// init segment comes with the first segment made.
#[test]
fn serves_a_live_stream_in_fragmented_mp4() {
    let server = server();
    let source = Source::Stream {
        url: fixture("clip-sound-ac3.mkv").to_string_lossy().into_owned(),
        live: true,
    };
    let surround = Profile {
        audio: vec![AudioCodec::Ac3],
        max_channels: 8,
        ..hls(Segments::Fmp4)
    };
    let cast = server
        .add(CastRequest::new(source, surround), LOCALHOST)
        .unwrap();
    assert_eq!(cast.plan().delivery, Delivery::Hls(Segments::Fmp4));
    let playlist = get(&sibling(&cast.url(), "video.m3u8")).text();
    assert!(
        playlist.contains("#EXT-X-MAP:URI=\"init.mp4\""),
        "{playlist}"
    );
    let names = segment_names(&playlist);
    assert_eq!(names, ["0.m4s", "1.m4s"]);
    let init = get(&sibling(&cast.url(), "init.mp4"));
    assert_eq!(init.status, 200);
    assert!(init.body.windows(4).any(|bytes| bytes == b"dac3"));
    for (number, name) in names.iter().enumerate() {
        let segment = get(&sibling(&cast.url(), name));
        assert_eq!(&segment.body[4..8], b"moof", "{name} is a fragment");
        let mut bytes = init.body.clone();
        bytes.extend(segment.body);
        let (video, audio) = probe(&bytes, "mp4");
        let video = video.unwrap();
        assert!(video.keys[0], "{name} starts with a keyframe");
        assert!(close(video.first(), 10.0 + number as f64), "{name}");
        assert_eq!(audio.unwrap().codec, Id::AC3, "{name}");
    }
}

#[test]
fn serves_a_file_that_never_says_how_long_it_is_as_the_device_plays_it() {
    let server = server();
    let cast = cast(&server, "clip-no-duration.mkv", hls(Segments::Ts));
    let playlist = sibling(&cast.url(), "video.m3u8");
    assert!(get(&playlist).text().contains("#EXT-X-MEDIA-SEQUENCE:0"));
    // A device slow to ask for the first segment finds the file still
    // waiting at its start, not played through without it.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let listed = get(&playlist).text();
    assert_eq!(segment_names(&listed), ["0.ts", "1.ts", "2.ts"], "{listed}");
    for number in 0..12 {
        let segment = get(&sibling(&cast.url(), &format!("{number}.ts")));
        assert_eq!(segment.status, 200, "segment {number}");
        let (video, _) = probe(&segment.body, "ts");
        assert!(close(video.unwrap().first(), 10.0 + f64::from(number)));
    }
    assert!(get(&playlist).text().ends_with("#EXT-X-ENDLIST\n"));
    assert_eq!(get(&sibling(&cast.url(), "12.ts")).status, 500);
}

const VTT: &str = "WEBVTT\n\n00:00:00.200 --> 00:00:01.000\nFirst\n\n\
                   00:00:01.200 --> 00:00:01.900\nSecond\n\n";
const SRT: &str = "1\n00:00:00,200 --> 00:00:01,000\nFirst\n\n\
                   2\n00:00:01,200 --> 00:00:01,900\nSecond\n\n";

#[test]
fn serves_subtitles_from_a_file() {
    let server = server();
    let mut request_ = CastRequest::new(Source::File(fixture("clip.mp4")), Profile::default());
    request_.subtitles = Some(SubtitleSource::File {
        path: fixture("clip.srt"),
        encoding: None,
    });
    let cast = server.add(request_, LOCALHOST).unwrap();
    let vtt = get(&cast.webvtt_url().unwrap());
    assert_eq!(vtt.header("Content-Type"), Some("text/vtt; charset=utf-8"));
    assert_eq!(vtt.text(), VTT);
    assert_eq!(get(&cast.srt_url().unwrap()).text(), SRT);

    // Samsung TVs ask for them with the media.
    let media = request("HEAD", &cast.url(), &[("getcaptionInfo.sec", "1")]);
    assert_eq!(media.header("CaptionInfo.sec"), cast.srt_url().as_deref());

    server.set_subtitles(&cast, None);
    assert_eq!(cast.webvtt_url(), None);
    assert_eq!(get(&sibling(&cast.url(), "subs.vtt")).status, 404);
}

#[test]
fn serves_subtitles_later_or_earlier_than_their_times() {
    let server = server();
    let delayed = |delay| {
        let mut request = CastRequest::new(Source::File(fixture("clip.mp4")), Profile::default());
        request.subtitles = Some(SubtitleSource::File {
            path: fixture("clip.srt"),
            encoding: None,
        });
        request.subtitle_delay = delay;
        server.add(request, LOCALHOST).unwrap()
    };
    let later = delayed(0.5);
    // The file still goes as it is.
    assert_eq!(later.plan().delivery, Delivery::File);
    assert_eq!(
        get(&later.webvtt_url().unwrap()).text(),
        "WEBVTT\n\n00:00:00.700 --> 00:00:01.500\nFirst\n\n\
         00:00:01.700 --> 00:00:02.400\nSecond\n\n"
    );
    // One past the start is cut short there.
    let earlier = delayed(-0.5);
    assert_eq!(
        get(&earlier.srt_url().unwrap()).text(),
        "1\n00:00:00,000 --> 00:00:00,500\nFirst\n\n\
         2\n00:00:00,700 --> 00:00:01,400\nSecond\n\n"
    );
    assert_eq!(
        get(&delayed(-1.5).srt_url().unwrap()).text(),
        "1\n00:00:00,000 --> 00:00:00,400\nSecond\n\n"
    );
}

/// A cast of `name` with its sound `delay` seconds after its picture.
fn delayed(server: &Server, name: &str, profile: Profile, delay: f64) -> crate::Cast {
    let mut request = CastRequest::new(Source::File(fixture(name)), profile);
    request.audio_delay = delay;
    server.add(request, LOCALHOST).unwrap()
}

#[test]
fn delays_the_sound_it_repackages() {
    let server = server();
    // A device that would take the file gets it repackaged, for its sound
    // to be moved.
    let later = delayed(&server, "clip.mp4", Profile::default(), 0.5);
    assert_eq!(later.plan().delivery, Delivery::Hls(Segments::Ts));
    assert!(matches!(later.plan().audio, Some(AudioPlan::Copy { .. })));
    assert_eq!(later.plan().audio_delay, 0.5);
    let (video, audio) = fetch(&later.url(), Segments::Ts, 0);
    assert!(close(video.first(), 10.0), "{}", video.first());
    let audio = audio.unwrap();
    assert!(close(audio.first(), 10.5), "{}", audio.first());

    let earlier = delayed(&server, "clip.mp4", Profile::default(), -0.5);
    let (_, first) = fetch(&earlier.url(), Segments::Ts, 0);
    let (_, second) = fetch(&earlier.url(), Segments::Ts, 1);
    let (first, second) = (first.unwrap(), second.unwrap());
    // From the start, sound from half a second in.
    assert!(close(first.first(), 10.0), "{}", first.first());
    let last = second.times.iter().copied().fold(0.0, f64::max);
    assert!(last < 11.5 && last > 11.4, "{last}");

    let kind = Segments::Fmp4;
    let later = delayed(&server, "clip.mp4", hls(kind), 0.5);
    let (_, audio) = fetch(&later.url(), kind, 0);
    let audio = audio.unwrap();
    assert!(close(audio.first(), 10.5), "{}", audio.first());
}

#[test]
fn delays_the_sound_of_a_segment_made_from_there() {
    // Asked for first, the second segment is made by a job starting
    // there, which reads the sound from earlier on: Matroska has it beside
    // the keyframe the segment starts at, MP4 here further on.
    for name in ["clip.mp4", "clip-subs.mkv"] {
        let made = |first: bool| {
            let server = server();
            let cast = delayed(&server, name, Profile::default(), 0.5);
            let before = first.then(|| fetch(&cast.url(), Segments::Ts, 0).1.unwrap().times);
            (before, fetch(&cast.url(), Segments::Ts, 1).1.unwrap().times)
        };
        let ((before, after), (_, alone)) = (made(true), made(false));
        assert_eq!(alone, after, "{name}");
        // It carries on from the sound of the one before, a frame later.
        let end = before.unwrap().iter().copied().fold(0.0, f64::max);
        let first = alone.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            first > end && first - end < 0.03,
            "{name}: {end} then {first}"
        );
    }
}

#[test]
fn delays_the_sound_it_re_encodes() {
    let server = server();
    let profile = Profile {
        audio: vec![AudioCodec::Ac3],
        ..Profile::default()
    };
    let cast = delayed(&server, "clip.mp4", profile, 0.5);
    assert!(matches!(cast.plan().audio, Some(AudioPlan::Encode { .. })));
    let (_, audio) = fetch(&cast.url(), Segments::Ts, 0);
    let audio = audio.unwrap();
    assert_eq!(audio.codec, Id::AC3);
    assert!(close(audio.first(), 10.5), "{}", audio.first());
}

#[cfg(target_os = "macos")]
#[test]
fn turns_and_mirrors_the_video_it_re_encodes() {
    let server = server();
    let adjusted = |adjustments| {
        let mut request = CastRequest::new(Source::File(fixture("clip.mp4")), Profile::default());
        request.adjustments = Some(adjustments);
        server.add(request, LOCALHOST).unwrap()
    };
    let frame = |cast: &crate::Cast| {
        let segment = get(&sibling(&cast.url(), "0.ts"));
        assert_eq!(segment.status, 200);
        decode(&segment.body, "ts").remove(0).1.remove(0)
    };
    // As it is, but re-encoded, to set the others against.
    let plain = adjusted(Adjustments::default());
    assert!(matches!(
        plain.plan().video,
        Some(VideoPlan::Encode {
            width: 160,
            height: 120,
            ..
        })
    ));
    let plain = frame(&plain);
    let turned = Adjustments {
        turns: 1,
        mirrored: true,
        ..Adjustments::default()
    };
    let cast = adjusted(turned);
    assert_eq!(cast.plan().adjustments, Some(turned));
    let (video, _) = fetch(&cast.url(), Segments::Ts, 0);
    assert_eq!((video.width, video.height), (120, 160));
    let shown = frame(&cast);
    // Each pixel where mirroring and a quarter turn take it: at (x, y) of
    // the 120 x 160 picture, (159 - y, 119 - x) of the 160 x 120 one.
    let apart = |taken: &dyn Fn(usize, usize) -> u8| {
        let total: f64 = (0..160)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .map(|(x, y)| (f64::from(shown[y * 120 + x]) - f64::from(taken(x, y))).abs())
            .sum();
        total / (120.0 * 160.0)
    };
    let mirrored_turn = apart(&|x, y| plain[(119 - x) * 160 + (159 - y)]);
    let turn_only = apart(&|x, y| plain[(119 - x) * 160 + y]);
    assert!(mirrored_turn < 4.0, "{mirrored_turn}");
    assert!(
        turn_only > mirrored_turn * 3.0,
        "{turn_only} vs {mirrored_turn}"
    );

    // Cropped square, it's the middle of the picture, not all of it
    // squeezed in.
    let cast = adjusted(Adjustments {
        crop: Some(1.0),
        ..Adjustments::default()
    });
    let (video, _) = fetch(&cast.url(), Segments::Ts, 0);
    assert_eq!((video.width, video.height), (120, 120));
    let shown = frame(&cast);
    let apart = |taken: &dyn Fn(usize, usize) -> u8| {
        let total: f64 = (0..120)
            .flat_map(|y| (0..120).map(move |x| (x, y)))
            .map(|(x, y)| (f64::from(shown[y * 120 + x]) - f64::from(taken(x, y))).abs())
            .sum();
        total / (120.0 * 120.0)
    };
    let middle = apart(&|x, y| plain[y * 160 + x + 20]);
    let squeezed = apart(&|x, y| plain[y * 160 + x * 4 / 3]);
    assert!(middle < 4.0, "{middle}");
    assert!(squeezed > middle * 3.0, "{squeezed} vs {middle}");
}

/// Video goes as its file says to show it: turned by its display matrix,
/// re-encoded where it's copied into MPEG-TS, which has none, and at its
/// pixels' shape, re-encoded where a copy keeps it in neither.
#[test]
fn shows_video_as_its_file_says() {
    let server = server();
    let luma = |cast: &crate::Cast| {
        let segment = get(&sibling(&cast.url(), "0.ts"));
        assert_eq!(segment.status, 200);
        decode(&segment.body, "ts").remove(0).1.remove(0)
    };
    let encoded = |cast: &crate::Cast| match cast.plan().video {
        Some(VideoPlan::Encode { width, height, .. }) => Some((width, height)),
        _ => None,
    };
    // A phone's, a quarter turn counterclockwise, as a viewer turning it
    // would have it.
    let turned = cast(&server, "shapes/turned.mp4", hls(Segments::Ts));
    assert_eq!(encoded(&turned), Some((120, 160)));
    let (video, _) = fetch(&turned.url(), Segments::Ts, 0);
    assert_eq!((video.width, video.height), (120, 160));
    let mut request = CastRequest::new(Source::File(fixture("clip.mp4")), hls(Segments::Ts));
    request.adjustments = Some(Adjustments {
        turns: 3,
        ..Adjustments::default()
    });
    let asked = server.add(request, LOCALHOST).unwrap();
    let (shown, wanted) = (luma(&turned), luma(&asked));
    let apart = shown
        .iter()
        .zip(&wanted)
        .map(|(a, b)| f64::from(a.abs_diff(*b)))
        .sum::<f64>()
        / shown.len() as f64;
    assert!(apart < 2.0, "{apart}");
    // Fragmented MP4 keeps its display matrix.
    let kept = cast(&server, "shapes/turned.mp4", hls(Segments::Fmp4));
    assert_eq!(encoded(&kept), None);

    // Its coded pictures' shape, which a copy keeps, re-encoded wider.
    let wide = cast(&server, "shapes/wide.mp4", hls(Segments::Ts));
    assert_eq!(encoded(&wide), None);
    let mut request = CastRequest::new(Source::File(fixture("shapes/wide.mp4")), hls(Segments::Ts));
    request.adjustments = Some(Adjustments::default());
    let wide = server.add(request, LOCALHOST).unwrap();
    assert_eq!(encoded(&wide), Some((214, 120)));
    let (video, _) = fetch(&wide.url(), Segments::Ts, 0);
    assert_eq!((video.width, video.height), (214, 120));
    // Its container's alone, which a copy loses.
    let container = cast(&server, "shapes/wide.mkv", hls(Segments::Fmp4));
    assert_eq!(encoded(&container), Some((214, 120)));
}

#[test]
fn serves_subtitles_from_a_track_with_hls() {
    let server = server();
    let mut request_ = CastRequest::new(Source::File(fixture("clip-subs.mkv")), Profile::default());
    request_.subtitles = Some(SubtitleSource::Track(2));
    let cast = server.add(request_, LOCALHOST).unwrap();
    assert_eq!(get(&cast.webvtt_url().unwrap()).text(), VTT);
    assert_eq!(get(&cast.srt_url().unwrap()).text(), SRT);

    let master = get(&cast.url()).text();
    assert!(
        master.contains(
            "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"eng\","
        ),
        "{master}"
    );
    assert!(master.contains("SUBTITLES=\"subs\""));
    let playlist = get(&sibling(&cast.url(), "subs.m3u8")).text();
    assert_eq!(segment_names(&playlist), ["s0.vtt", "s1.vtt"]);
    let first = get(&sibling(&cast.url(), "s0.vtt")).text();
    assert!(first.starts_with("WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:900000,LOCAL:00:00:00.000\n"));
    assert!(
        first.contains("First") && !first.contains("Second"),
        "{first}"
    );
    let second = get(&sibling(&cast.url(), "s1.vtt")).text();
    assert!(
        second.contains("Second") && !second.contains("First"),
        "{second}"
    );
    assert_eq!(get(&sibling(&cast.url(), "s2.vtt")).status, 404);

    // A device given them beside the media finds none in the playlist.
    let beside = Profile {
        hls_subtitles: false,
        ..Profile::default()
    };
    let mut request_ = CastRequest::new(Source::File(fixture("clip-subs.mkv")), beside);
    request_.subtitles = Some(SubtitleSource::Track(2));
    let cast = server.add(request_, LOCALHOST).unwrap();
    let master = get(&cast.url()).text();
    assert!(!master.contains("SUBTITLES"), "{master}");
    assert_eq!(get(&cast.webvtt_url().unwrap()).text(), VTT);

    // Not a subtitle track.
    let mut wrong = CastRequest::new(Source::File(fixture("clip-subs.mkv")), Profile::default());
    wrong.subtitles = Some(SubtitleSource::Track(0));
    let cast = server.add(wrong, LOCALHOST).unwrap();
    assert_eq!(get(&cast.webvtt_url().unwrap()).status, 500);
}

/// A cast of the fixture with DVD subtitles, with its subtitles or none.
fn dvd_cast(server: &Server, profile: Profile, subtitles: bool) -> crate::Cast {
    let mut request = CastRequest::new(Source::File(fixture("clip-dvdsub.mkv")), profile);
    request.subtitles = subtitles.then_some(SubtitleSource::Track(2));
    server.add(request, LOCALHOST).expect("the fixture casts")
}

#[test]
fn re_encodes_video_it_would_copy_to_draw_picture_subtitles_in() {
    let server = server();
    for profile in [Profile::default(), hls(Segments::Ts)] {
        let drawn = dvd_cast(&server, profile.clone(), true);
        assert_eq!(drawn.plan().delivery, Delivery::Hls(Segments::Ts));
        assert!(matches!(drawn.plan().video, Some(VideoPlan::Encode { .. })));
        assert_eq!(drawn.plan().subtitles, Some(2));
        assert_eq!(drawn.draws(Some(&SubtitleSource::Track(2))), Some(2));
        assert_eq!(drawn.draws(None), None);
        // Pictures have no text to serve beside it.
        assert_eq!(drawn.webvtt_url(), None);
        assert_eq!(get(&sibling(&drawn.url(), "subs.vtt")).status, 404);

        // Without them it's copied, and served again with them.
        let plain = dvd_cast(&server, profile, false);
        assert!(matches!(plain.plan().video, Some(VideoPlan::Copy { .. })));
        assert_eq!(plain.plan().subtitles, None);
        assert_eq!(plain.draws(Some(&SubtitleSource::Track(2))), Some(2));
        assert_eq!(plain.draws(None), None);
    }
}

#[test]
fn sends_the_file_without_picture_subtitles_to_a_device_that_takes_only_files() {
    let server = server();
    let files = Profile {
        files: vec![Container::Matroska],
        hls: None,
        stream: false,
        ..Profile::default()
    };
    let cast = dvd_cast(&server, files, true);
    assert_eq!(cast.plan().delivery, Delivery::File);
    assert_eq!(cast.plan().subtitles, None);
    assert_eq!(cast.draws(Some(&SubtitleSource::Track(2))), None);
    assert_eq!(cast.plan().describe(), "as is");
}

/// The video frames of `bytes`, read as a file, decoded: each one's time
/// and its planes, luma then chroma, row after row.
#[cfg(target_os = "macos")]
fn decode(bytes: &[u8], extension: &str) -> Vec<(f64, Vec<Vec<u8>>)> {
    let path = written(bytes, extension);
    let mut input = ffmpeg_safe::input_file(&path).expect("the output opens");
    let stream = input.streams().best(Type::Video).expect("video");
    let (index, time_base) = (stream.index(), f64::from(stream.time_base()));
    let mut decoder = ffmpeg::codec::context::Context::from_parameters(stream.parameters())
        .and_then(|context| context.decoder().video())
        .expect("a decoder");
    let mut frames = Vec::new();
    let mut take = |decoder: &mut ffmpeg::decoder::Video| {
        let mut frame = ffmpeg::frame::Video::empty();
        while decoder.receive_frame(&mut frame).is_ok() {
            assert_eq!(frame.format(), ffmpeg::format::Pixel::YUV420P);
            let planes = (0..3)
                .map(|plane| {
                    let width = frame.plane_width(plane) as usize;
                    let (data, stride) = ffmpeg_safe::plane(&frame, plane).unwrap();
                    data.chunks(stride)
                        .take(frame.plane_height(plane) as usize)
                        .flat_map(|row| row[..width].to_vec())
                        .collect()
                })
                .collect();
            let time = frame.timestamp().expect("a time") as f64 * time_base;
            frames.push((time, planes));
        }
    };
    for (stream, packet) in input.packets() {
        if stream.index() == index {
            decoder.send_packet(&packet).unwrap();
            take(&mut decoder);
        }
    }
    decoder.send_eof().unwrap();
    take(&mut decoder);
    drop(input);
    let _ = fs::remove_file(&path);
    frames
}

/// The frames of a cast's first two segments, with their times.
#[cfg(target_os = "macos")]
fn first_frames(cast: &crate::Cast) -> Vec<(f64, Vec<Vec<u8>>)> {
    (0..2)
        .flat_map(|number| {
            let segment = get(&sibling(&cast.url(), &format!("{number}.ts")));
            assert_eq!(segment.status, 200);
            decode(&segment.body, "ts")
        })
        .collect()
}

/// Drawn in later than their times, picture subtitles show that much
/// later: past the clip's end, not at all.
#[cfg(target_os = "macos")]
#[test]
fn draws_picture_subtitles_later_than_their_times() {
    let profile = Profile {
        max_size: Some((80, 60)),
        ..hls(Segments::Ts)
    };
    let server = server();
    let mut request = CastRequest::new(Source::File(fixture("clip-dvdsub.mkv")), profile.clone());
    request.subtitles = Some(SubtitleSource::Track(2));
    request.subtitle_delay = 5.0;
    let late = server.add(request, LOCALHOST).unwrap();
    assert_eq!(late.plan().subtitles, Some(2));
    let plain = dvd_cast(&server, profile, false);
    let (late, plain) = (first_frames(&late), first_frames(&plain));
    assert_eq!(late.len(), plain.len());
    for ((time, with), (_, without)) in late.iter().zip(&plain) {
        let apart = with[0]
            .iter()
            .zip(&without[0])
            .map(|(with, without)| (f64::from(*with) - f64::from(*without)).abs())
            .sum::<f64>()
            / with[0].len() as f64;
        assert!(apart < 0.5, "at {time:.2}: {apart}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn draws_picture_subtitles_into_video_it_re_encodes() {
    // Too big for the device, so it is re-encoded.
    let profile = Profile {
        max_size: Some((80, 60)),
        ..hls(Segments::Ts)
    };
    let server = server();
    let drawn = dvd_cast(&server, profile.clone(), true);
    let plain = dvd_cast(&server, profile.clone(), false);
    assert_eq!(drawn.plan().subtitles, Some(2));
    assert_eq!(
        drawn.plan().describe(),
        "re-encoded to H.264, subtitles drawn in"
    );
    assert_eq!(plain.plan().subtitles, None);
    assert_eq!(drawn.webvtt_url(), None);
    assert_eq!(drawn.draws(Some(&SubtitleSource::Track(2))), Some(2));
    assert_eq!(plain.draws(Some(&SubtitleSource::Track(2))), Some(2));
    assert_eq!(drawn.draws(None), None);

    // Text subtitles go beside it, not drawn in.
    let mut text = CastRequest::new(Source::File(fixture("clip-subs.mkv")), profile);
    text.subtitles = Some(SubtitleSource::Track(2));
    let text = server.add(text, LOCALHOST).unwrap();
    assert!(matches!(text.plan().video, Some(VideoPlan::Encode { .. })));
    assert_eq!(text.plan().subtitles, None);
    assert_eq!(text.draws(Some(&SubtitleSource::Track(2))), None);
    assert!(text.webvtt_url().is_some());

    let (drawn, plain) = (first_frames(&drawn), first_frames(&plain));
    assert_eq!((drawn.len(), plain.len()), (50, 50));
    // Each cue's bar at 80x60: the first low in the picture, the second
    // high. White and black, it takes the color out of what is under it.
    let low = (20..60, 48..56);
    let high = (20..60, 4..12);
    let mut shown = 0;
    for ((time, with), (_, without)) in drawn.iter().zip(&plain) {
        let at = time - 10.0;
        let cue = if (0.19..0.99).contains(&at) {
            Some(&low)
        } else if (1.19..1.89).contains(&at) {
            Some(&high)
        } else {
            None
        };
        // How much each plane differs inside the bar, and away from it:
        // luma, then chroma at half the size.
        for (plane, shrink) in [(0, 1), (1, 2), (2, 2)] {
            let width = 80 / shrink;
            let (mut inside, mut outside) = (Vec::new(), Vec::new());
            for (index, (with, without)) in with[plane].iter().zip(&without[plane]).enumerate() {
                let (x, y) = (index % width, index / width);
                let difference = (f64::from(*with) - f64::from(*without)).abs();
                let near = |margin: usize| {
                    cue.is_some_and(|(across, down)| {
                        (across.start / shrink - margin..across.end / shrink + margin).contains(&x)
                            && (down.start / shrink - margin..down.end / shrink + margin)
                                .contains(&y)
                    })
                };
                if near(0) {
                    inside.push(difference);
                } else if !near(2) {
                    outside.push(difference);
                }
            }
            let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len().max(1) as f64;
            assert!(
                mean(&outside) < 1.5,
                "at {at:.2}, plane {plane} away from any bar: {}",
                mean(&outside)
            );
            if cue.is_some() {
                assert!(
                    mean(&inside) > if plane == 0 { 30.0 } else { 10.0 },
                    "at {at:.2}, plane {plane} in the bar: {}",
                    mean(&inside)
                );
            }
        }
        shown += usize::from(cue.is_some());
    }
    assert_eq!(shown, 20 + 18);
}
