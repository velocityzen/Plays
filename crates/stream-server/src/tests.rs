//! Serving the fixtures over HTTP, and reading back what comes out.

use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ffmpeg::codec::Id;
use ffmpeg::media::Type;
use ffmpeg_next as ffmpeg;

use crate::{
    AudioCodec, AudioPlan, CastRequest, Delivery, Error, Pick, Profile, Segments, Server, Source,
    SubtitleSource, VideoCodec, VideoPlan,
};

const LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A server with one-second segments, so the two-second fixtures have two.
fn server() -> Server {
    Server::start_with(SocketAddr::new(LOCALHOST, 0), 1.0).expect("the server starts")
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

/// The video and sound streams of `bytes`, read as a file.
fn probe(bytes: &[u8], extension: &str) -> (Option<Probed>, Option<Probed>) {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "stream-server-{}-{}.{extension}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, bytes).unwrap();
    let mut input = ffmpeg::format::input(&path).expect("the output opens");
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
    let cast = cast(&server, "clip.mp4", Profile::default());
    assert_eq!(server.casts().len(), 1);
    server.remove(&cast);
    assert!(server.casts().is_empty());
    assert_eq!(get(&cast.url()).status, 404);
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

    // Not a subtitle track.
    let mut wrong = CastRequest::new(Source::File(fixture("clip-subs.mkv")), Profile::default());
    wrong.subtitles = Some(SubtitleSource::Track(0));
    let cast = server.add(wrong, LOCALHOST).unwrap();
    assert_eq!(get(&cast.webvtt_url().unwrap()).status, 500);
}
