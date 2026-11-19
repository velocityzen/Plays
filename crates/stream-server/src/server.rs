//! The server: each cast is served under a path of its own, a random
//! token, so a device can reach what it was told to play and nothing else.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ffmpeg::format::context::Input;
use ffmpeg_next as ffmpeg;

use crate::encode::{Colors, VideoEncoder, VideoSettings};
use crate::http::{self, Body, Request, Response};
use crate::job::{self, Live, Media, Vod};
use crate::media::{self, MediaInfo, Source};
use crate::mux::OFFSET;
use crate::plan::{self, AudioPlan, Delivery, Pick, Plan, VideoPlan};
use crate::playlist::{self, SubtitlesRendition, Variant};
use crate::profile::{AudioCodec, HLS_MIME, Profile, Segments};
use crate::subtitles::{self, Cue, SubtitleSource};
use crate::{Error, segments};

/// How long segments are, about, in seconds.
const SEGMENT: f64 = 6.0;
/// How long a request waits for subtitles to be read.
const SUBTITLES_WAIT: Duration = Duration::from_secs(60);
/// The DLNA flags for media: streamed, in the background, and allowed to
/// stall, by DLNA 1.5.
const DLNA_FLAGS: &str = "01700000000000000000000000000000";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// What to cast, and what the device it goes to plays.
#[derive(Clone, Debug)]
pub struct CastRequest {
    pub source: Source,
    pub profile: Profile,
    pub video: Pick,
    pub audio: Pick,
    pub subtitles: Option<SubtitleSource>,
}

impl CastRequest {
    /// The source's default video and sound, without subtitles.
    pub fn new(source: Source, profile: Profile) -> Self {
        Self {
            source,
            profile,
            video: Pick::Best,
            audio: Pick::Best,
            subtitles: None,
        }
    }
}

pub struct Server {
    shared: Arc<Shared>,
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    casts: Mutex<HashMap<String, Arc<CastState>>>,
    segment: f64,
}

impl Server {
    /// Serve on every IPv4 interface, on a port the system picks.
    pub fn start() -> Result<Self, Error> {
        Self::start_on(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0))
    }

    pub fn start_on(address: SocketAddr) -> Result<Self, Error> {
        Self::start_with(address, SEGMENT)
    }

    /// With segments about `segment` seconds long.
    pub(crate) fn start_with(address: SocketAddr, segment: f64) -> Result<Self, Error> {
        crate::init();
        let listener = TcpListener::bind(address)?;
        let address = listener.local_addr()?;
        let shared = Arc::new(Shared {
            casts: Mutex::default(),
            segment,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let handler: http::Handler = {
            let shared = Arc::clone(&shared);
            Arc::new(move |request: &Request| shared.handle(request))
        };
        let thread = {
            let stop = Arc::clone(&stop);
            thread::Builder::new()
                .name("stream-server".into())
                .spawn(move || http::serve(listener, handler, stop))?
        };
        Ok(Self {
            shared,
            address,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    /// Get media ready for a device at `device`, and the URL to tell it.
    /// Opens the source to see what is in it, so it may take a moment.
    pub fn add(&self, request: CastRequest, device: IpAddr) -> Result<Cast, Error> {
        let host = local_address(device, self.address)?;
        let stop = Arc::new(AtomicBool::new(false));
        let mut input = media::open(&request.source, Arc::clone(&stop))?;
        let info = MediaInfo::read(&input, &request.source.location());
        let plan = plan::plan(
            &info,
            &request.source,
            request.video,
            request.audio,
            &request.profile,
        )?;
        log::info!("casting {}: {}", request.source.location(), plan.describe());
        let live = request.source.is_live() || info.duration.is_none();
        let segment = self.shared.segment;
        let variant = variant(&input, &info, &plan);
        let timeline = match plan.delivery {
            Delivery::Hls(_) if !live => {
                Some(segments::timeline(&mut input, &info, &plan, segment))
            }
            _ => None,
        };
        drop(input);
        let media = Arc::new(Media {
            source: request.source.clone(),
            kind: plan.segments(),
            info,
            plan,
            segment,
        });
        let output = match (media.plan.delivery, timeline) {
            (Delivery::Direct, _) => Output::Direct,
            (Delivery::File, _) => match &media.source {
                Source::File(path) => Output::File(path.clone()),
                Source::Stream { .. } => Output::Direct,
            },
            (Delivery::Hls(_), Some(timeline)) => {
                Output::Vod(Vod::new(Arc::clone(&media), timeline))
            }
            (Delivery::Hls(_), None) => Output::Live(Live::new(Arc::clone(&media))),
            (Delivery::Stream, _) => Output::Stream,
        };
        let token = token()?;
        let state = Arc::new(CastState {
            base: format!(
                "http://{}/{token}/",
                SocketAddr::new(host, self.address.port())
            ),
            file_name: file_name(&media.source),
            token: token.clone(),
            media,
            output,
            variant,
            hls_subtitles: request.profile.hls_subtitles,
            stop,
            subtitles: Mutex::default(),
        });
        state.set_subtitles(request.subtitles);
        lock(&self.shared.casts).insert(token, Arc::clone(&state));
        Ok(Cast { state })
    }

    /// Stop serving a cast.
    pub fn remove(&self, cast: &Cast) {
        if let Some(state) = lock(&self.shared.casts).remove(&cast.state.token) {
            state.close();
        }
    }

    /// The casts being served.
    pub fn casts(&self) -> Vec<Cast> {
        lock(&self.shared.casts)
            .values()
            .map(|state| Cast {
                state: Arc::clone(state),
            })
            .collect()
    }

    /// Serve other subtitles with a cast, or none.
    pub fn set_subtitles(&self, cast: &Cast, subtitles: Option<SubtitleSource>) {
        cast.state.set_subtitles(subtitles);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the listener to see it's to stop.
        let wake = SocketAddr::new(
            if self.address.ip().is_unspecified() {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            } else {
                self.address.ip()
            },
            self.address.port(),
        );
        let _ = TcpStream::connect_timeout(&wake, Duration::from_secs(1));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        for (_, state) in lock(&self.shared.casts).drain() {
            state.close();
        }
    }
}

/// A cast being served.
#[derive(Clone)]
pub struct Cast {
    state: Arc<CastState>,
}

impl std::fmt::Debug for Cast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cast").field("url", &self.url()).finish()
    }
}

impl Cast {
    /// The URL to tell the device to play.
    pub fn url(&self) -> String {
        let state = &self.state;
        match &state.output {
            Output::Direct => state.media.source.location(),
            Output::File(_) => format!("{}{}", state.base, state.file_name),
            Output::Vod(_) | Output::Live(_) => format!("{}master.m3u8", state.base),
            Output::Stream => format!("{}stream.ts", state.base),
        }
    }

    /// The MIME type to tell the device.
    pub fn mime(&self) -> &'static str {
        self.state.mime()
    }

    /// What the device gets, and what is done to it on the way.
    pub fn plan(&self) -> &Plan {
        &self.state.media.plan
    }

    /// What is in the source.
    pub fn media(&self) -> &MediaInfo {
        &self.state.media.info
    }

    /// Seconds, when the media has an end.
    pub fn duration(&self) -> Option<f64> {
        self.state.media.info.duration
    }

    /// The subtitles as WebVTT, for a device that takes them beside the
    /// media; HLS lists them in its playlist too.
    pub fn webvtt_url(&self) -> Option<String> {
        self.state
            .has_subtitles()
            .then(|| format!("{}subs.vtt", self.state.base))
    }

    /// The subtitles as SRT, as DLNA TVs take them.
    pub fn srt_url(&self) -> Option<String> {
        self.state
            .has_subtitles()
            .then(|| format!("{}subs.srt", self.state.base))
    }

    /// UPnP's `protocolInfo` for the media: `http-get:*:{mime}:{features}`.
    pub fn protocol_info(&self) -> String {
        format!("http-get:*:{}:{}", self.mime(), self.state.features())
    }
}

enum Output {
    /// The device fetches the source's own URL.
    Direct,
    File(PathBuf),
    Vod(Arc<Vod>),
    Live(Arc<Live>),
    Stream,
}

struct CastState {
    token: String,
    /// `http://host:port/token/`.
    base: String,
    /// The file's name in its URL, keeping its extension for devices that
    /// go by it.
    file_name: String,
    media: Arc<Media>,
    output: Output,
    variant: Variant,
    /// Whether the HLS playlist lists the subtitles.
    hls_subtitles: bool,
    stop: Arc<AtomicBool>,
    subtitles: Mutex<Option<Arc<Subtitles>>>,
}

/// A cast's subtitles, read on a thread of their own.
struct Subtitles {
    source: SubtitleSource,
    name: String,
    language: Option<String>,
    cues: Mutex<Option<Result<Arc<Vec<Cue>>, String>>>,
    ready: Condvar,
}

impl Subtitles {
    fn cues(&self) -> Result<Arc<Vec<Cue>>, String> {
        let deadline = Instant::now() + SUBTITLES_WAIT;
        let mut cues = lock(&self.cues);
        loop {
            if let Some(result) = &*cues {
                return result.clone();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("reading the subtitles took too long".into());
            }
            cues = self
                .ready
                .wait_timeout(cues, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }
}

impl CastState {
    fn close(&self) {
        self.stop.store(true, Ordering::Relaxed);
        match &self.output {
            Output::Vod(vod) => vod.close(),
            Output::Live(live) => live.close(),
            _ => {}
        }
    }

    fn mime(&self) -> &'static str {
        let info = &self.media.info;
        let has_video = self.media.plan.video.is_some();
        match self.output {
            Output::Direct | Output::File(_) => info.container.mime(has_video),
            Output::Vod(_) | Output::Live(_) => HLS_MIME,
            Output::Stream => "video/mp2t",
        }
    }

    /// DLNA's content features: whether it can be fetched by bytes or by
    /// time, and whether it is made as it is sent.
    fn features(&self) -> String {
        match self.output {
            Output::File(_) => format!("DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS={DLNA_FLAGS}"),
            Output::Stream => format!("DLNA.ORG_OP=10;DLNA.ORG_CI=1;DLNA.ORG_FLAGS={DLNA_FLAGS}"),
            _ => "*".into(),
        }
    }

    fn has_subtitles(&self) -> bool {
        lock(&self.subtitles).is_some()
    }

    fn set_subtitles(&self, source: Option<SubtitleSource>) {
        let mut slot = lock(&self.subtitles);
        let Some(source) = source else {
            *slot = None;
            return;
        };
        let (name, language) = match &source {
            SubtitleSource::Track(index) => {
                let stream = self.media.info.stream(*index);
                (
                    stream
                        .and_then(|stream| stream.title.clone())
                        .unwrap_or_else(|| "Subtitles".into()),
                    stream.and_then(|stream| stream.language.clone()),
                )
            }
            SubtitleSource::File { path, .. } => (
                path.file_stem().map_or_else(
                    || "Subtitles".into(),
                    |stem| stem.to_string_lossy().into_owned(),
                ),
                None,
            ),
        };
        let subtitles = Arc::new(Subtitles {
            source,
            name,
            language,
            cues: Mutex::default(),
            ready: Condvar::new(),
        });
        *slot = Some(Arc::clone(&subtitles));
        let media = self.media.source.clone();
        let stop = Arc::clone(&self.stop);
        let spawned = thread::Builder::new()
            .name("stream-server subtitles".into())
            .spawn({
                let subtitles = Arc::clone(&subtitles);
                move || {
                    let read = subtitles::read(&subtitles.source, &media, stop)
                        .map(Arc::new)
                        .map_err(|err| err.to_string());
                    if let Err(err) = &read {
                        log::warn!("can't read the subtitles: {err}");
                    }
                    *lock(&subtitles.cues) = Some(read);
                    subtitles.ready.notify_all();
                }
            });
        if let Err(err) = spawned {
            *lock(&subtitles.cues) = Some(Err(err.to_string()));
        }
    }

    fn respond(&self, name: &str, request: &Request) -> Response {
        if let Some(response) = self.respond_subtitles(name) {
            return response;
        }
        match &self.output {
            Output::File(path) if name == self.file_name => {
                match File::open(path).and_then(|file| file.metadata()) {
                    Ok(metadata) => {
                        let body = Body::File {
                            path: path.clone(),
                            len: metadata.len(),
                        };
                        let response = Response::new(200, body).with("Content-Type", self.mime());
                        self.media_response(response, request)
                    }
                    Err(err) => {
                        log::warn!("can't serve {}: {err}", path.display());
                        Response::status(404)
                    }
                }
            }
            Output::Vod(vod) => self.respond_vod(vod, name),
            Output::Live(live) => self.respond_live(live, name),
            Output::Stream if name == "stream.ts" => self.respond_stream(request),
            _ => Response::status(404),
        }
    }

    /// The headers DLNA TVs look for on media.
    fn media_response(&self, response: Response, request: &Request) -> Response {
        let mut response = response
            .with(
                "transferMode.dlna.org",
                request
                    .header("transferMode.dlna.org")
                    .unwrap_or("Streaming"),
            )
            .with("contentFeatures.dlna.org", self.features());
        // Samsung TVs ask for subtitles this way.
        if request.header("getcaptionInfo.sec").is_some() && self.has_subtitles() {
            response = response.with("CaptionInfo.sec", format!("{}subs.srt", self.base));
        }
        response
    }

    fn master(&self) -> Response {
        let subtitles = match &self.output {
            Output::Vod(_) if self.hls_subtitles => {
                lock(&self.subtitles)
                    .as_ref()
                    .map(|subtitles| SubtitlesRendition {
                        language: subtitles.language.clone(),
                        name: subtitles.name.clone(),
                    })
            }
            _ => None,
        };
        Response::text(
            HLS_MIME,
            playlist::master(&self.variant, subtitles.as_ref()),
        )
    }

    fn respond_vod(&self, vod: &Arc<Vod>, name: &str) -> Response {
        let kind = self.media.kind;
        match name {
            "master.m3u8" => self.master(),
            "video.m3u8" => Response::text(HLS_MIME, playlist::vod(vod.timeline(), kind)),
            "init.mp4" if kind == Segments::Fmp4 => answer(vod.init(), "video/mp4"),
            _ => match segment_number(name, kind) {
                Some(segment) if segment < vod.timeline().len() => {
                    answer(vod.segment(segment), segment_mime(kind))
                }
                _ => Response::status(404),
            },
        }
    }

    fn respond_live(&self, live: &Arc<Live>, name: &str) -> Response {
        let kind = self.media.kind;
        match name {
            "master.m3u8" => self.master(),
            "video.m3u8" => match live.window() {
                Ok(window) => Response::text(HLS_MIME, playlist::live(&window, kind))
                    .with("Cache-Control", "no-cache"),
                Err(err) => failure(&err),
            },
            "init.mp4" if kind == Segments::Fmp4 => answer(live.init(), "video/mp4"),
            _ => match segment_number(name, kind) {
                Some(sequence) => answer(live.segment(sequence), segment_mime(kind)),
                None => Response::status(404),
            },
        }
    }

    fn respond_stream(&self, request: &Request) -> Response {
        let duration = self.media.info.duration;
        let seek = request
            .header("TimeSeekRange.dlna.org")
            .and_then(npt_start)
            .map(|start| (start, true))
            .or_else(|| {
                request
                    .param("start")
                    .and_then(|start| start.parse::<f64>().ok())
                    .filter(|start| start.is_finite())
                    .map(|start| (start, false))
            });
        let start = seek.map_or(0.0, |(start, _)| start.max(0.0));
        let body = if request.method == http::Method::Head {
            Body::Stream(sync_channel(0).1)
        } else {
            Body::Stream(job::stream(
                Arc::clone(&self.media),
                self.media.info.start + start,
                Arc::clone(&self.stop),
            ))
        };
        let mut response = Response::new(200, body).with("Content-Type", self.mime());
        if let Some((_, true)) = seek {
            response = response.with(
                "TimeSeekRange.dlna.org",
                match duration {
                    Some(duration) => format!("npt={start:.3}-{duration:.3}/{duration:.3}"),
                    None => format!("npt={start:.3}-"),
                },
            );
        }
        self.media_response(response, request)
    }

    /// Subtitles, by any of their names.
    fn respond_subtitles(&self, name: &str) -> Option<Response> {
        let is_segment = name
            .strip_prefix('s')
            .and_then(|rest| rest.strip_suffix(".vtt"))
            .and_then(|number| number.parse::<usize>().ok());
        if !matches!(name, "subs.vtt" | "subs.srt" | "subs.m3u8") && is_segment.is_none() {
            return None;
        }
        let Some(subtitles) = lock(&self.subtitles).clone() else {
            return Some(Response::status(404));
        };
        let vod = match &self.output {
            Output::Vod(vod) => Some(vod),
            _ => None,
        };
        if name == "subs.m3u8" {
            return Some(match vod {
                Some(vod) => Response::text(HLS_MIME, playlist::subtitles(vod.timeline())),
                None => Response::status(404),
            });
        }
        let cues = match subtitles.cues() {
            Ok(cues) => cues,
            Err(err) => {
                log::warn!("can't serve subtitles: {err}");
                return Some(Response::status(500));
            }
        };
        const VTT: &str = "text/vtt; charset=utf-8";
        Some(match (name, is_segment, vod) {
            ("subs.vtt", _, _) => Response::text(VTT, subtitles::webvtt(&cues, None, None)),
            ("subs.srt", _, _) => {
                Response::text("application/x-subrip; charset=utf-8", subtitles::srt(&cues))
            }
            (_, Some(segment), Some(vod)) if segment < vod.timeline().len() => {
                let timeline = vod.timeline();
                let start = self.media.info.start;
                let range = (
                    timeline.start(segment) - start,
                    timeline.end_of(segment) - start,
                );
                let mpegts = ((OFFSET + start) * 90_000.0).round() as u64;
                Response::text(VTT, subtitles::webvtt(&cues, Some(range), Some(mpegts)))
            }
            _ => Response::status(404),
        })
    }
}

impl Shared {
    fn handle(&self, request: &Request) -> Response {
        let path = request.path.trim_start_matches('/');
        let Some((token, name)) = path.split_once('/') else {
            return Response::status(404);
        };
        let Some(state) = lock(&self.casts).get(token).cloned() else {
            return Response::status(404);
        };
        log::debug!("{:?} {name}", request.method);
        state.respond(name, request)
    }
}

fn answer(result: Result<Arc<Vec<u8>>, Error>, mime: &str) -> Response {
    match result {
        Ok(bytes) => Response::bytes(mime, bytes),
        Err(err) => failure(&err),
    }
}

fn failure(err: &Error) -> Response {
    log::warn!("can't serve a segment: {err}");
    Response::status(500)
}

fn segment_mime(kind: Segments) -> &'static str {
    match kind {
        Segments::Fmp4 => "video/mp4",
        Segments::Ts => "video/mp2t",
    }
}

/// The number of a segment's name, `12.ts` or `12.m4s`.
fn segment_number(name: &str, kind: Segments) -> Option<usize> {
    name.strip_suffix(playlist::extension(kind))?
        .strip_suffix('.')?
        .parse()
        .ok()
}

/// Where a DLNA time seek starts: `npt=12.5-`, `npt=0:01:02.5-30`.
fn npt_start(header: &str) -> Option<f64> {
    let spec = header.trim().strip_prefix("npt=")?;
    let start = spec.split('-').next()?.trim();
    if start.is_empty() {
        return Some(0.0);
    }
    let mut seconds = 0.0;
    for part in start.split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    seconds.is_finite().then_some(seconds)
}

/// The address the device reaches this computer at: the one a packet to
/// it goes out from.
fn local_address(device: IpAddr, listening: SocketAddr) -> Result<IpAddr, Error> {
    if device.is_ipv6() && listening.is_ipv4() {
        return Err(Error::Unsupported(
            "the device has an IPv6 address, and media is served over IPv4".into(),
        ));
    }
    if device.is_loopback() {
        return Ok(device);
    }
    let any = match device {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
    };
    let socket = UdpSocket::bind(SocketAddr::new(any, 0))?;
    // Connecting a UDP socket sends nothing; it only picks the route.
    socket.connect(SocketAddr::new(device, 9))?;
    Ok(socket.local_addr()?.ip())
}

/// 16 random bytes, in hex.
fn token() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn file_name(source: &Source) -> String {
    match source {
        Source::File(path) => match Path::new(path).extension() {
            Some(extension) => format!("media.{}", extension.to_string_lossy().to_lowercase()),
            None => "media".into(),
        },
        Source::Stream { .. } => "media".into(),
    }
}

/// What the master playlist says: bandwidth, codecs, size and rate.
fn variant(input: &Input, info: &MediaInfo, plan: &Plan) -> Variant {
    let mut codecs = Vec::new();
    let mut known = true;
    let mut copied = false;
    let mut encoded = 0;
    let mut resolution = None;
    let mut frame_rate = None;
    match &plan.video {
        Some(VideoPlan::Copy { stream, .. }) => {
            copied = true;
            match input
                .stream(*stream)
                .and_then(|s| playlist::video_codec(&s.parameters()))
            {
                Some(codec) => codecs.push(codec),
                None => known = false,
            }
            if let Some(video) = info.video(*stream) {
                resolution = Some((video.width, video.height));
                frame_rate = video.frame_rate;
            }
        }
        Some(VideoPlan::Encode {
            stream,
            to,
            encoder,
            width,
            height,
            ten_bit,
            bit_rate,
            ..
        }) => {
            encoded += bit_rate * 2;
            resolution = Some((*width, *height));
            frame_rate = info.video(*stream).and_then(|video| video.frame_rate);
            // The encoder's parameter sets name its profile and level.
            let settings = VideoSettings {
                encoder,
                codec: *to,
                width: *width,
                height: *height,
                ten_bit: *ten_bit,
                bit_rate: *bit_rate,
                time_base: ffmpeg::Rational::new(1, 90_000),
                frame_rate: frame_rate
                    .map(|rate| ffmpeg::Rational::new((rate * 1000.0).round() as i32, 1000)),
                colors: Colors::default(),
                global_header: true,
            };
            match VideoEncoder::open(&settings)
                .ok()
                .and_then(|encoder| playlist::video_codec(&encoder.parameters()))
            {
                Some(codec) => codecs.push(codec),
                None => known = false,
            }
        }
        None => {}
    }
    match &plan.audio {
        Some(AudioPlan::Copy { stream, .. }) => {
            copied = true;
            match input
                .stream(*stream)
                .and_then(|s| playlist::audio_codec(&s.parameters()))
            {
                Some(codec) => codecs.push(codec),
                None => known = false,
            }
        }
        Some(AudioPlan::Encode { to, bit_rate, .. }) => {
            encoded += bit_rate;
            codecs.push(
                match to {
                    AudioCodec::Ac3 => "ac-3",
                    AudioCodec::Eac3 => "ec-3",
                    _ => "mp4a.40.2",
                }
                .into(),
            );
        }
        None => {}
    }
    let copied = if copied {
        info.bit_rate.unwrap_or(8_000_000)
    } else {
        0
    };
    Variant {
        bandwidth: (copied + encoded) * 11 / 10,
        codecs: (known && !codecs.is_empty()).then(|| codecs.join(",")),
        resolution,
        frame_rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_time_seeks() {
        assert_eq!(npt_start("npt=12.5-"), Some(12.5));
        assert_eq!(npt_start("npt=0:01:02.5-30"), Some(62.5));
        assert_eq!(npt_start("npt=-"), Some(0.0));
        assert_eq!(npt_start("bytes=0-"), None);
        assert_eq!(npt_start("npt=abc-"), None);
    }

    #[test]
    fn names_segments() {
        assert_eq!(segment_number("12.ts", Segments::Ts), Some(12));
        assert_eq!(segment_number("3.m4s", Segments::Fmp4), Some(3));
        assert_eq!(segment_number("3.ts", Segments::Fmp4), None);
        assert_eq!(segment_number("x.ts", Segments::Ts), None);
    }
}
