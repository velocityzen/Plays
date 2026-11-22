//! DLNA renderers, the TVs and speakers UPnP finds, through the `dlna`
//! crate. What one plays comes from its protocol info, asked once as it's
//! found: whether it shows video, and so is offered for video at all, and
//! what the stream server serves it. A file goes as it is where the
//! renderer lists its kind; else as a progressive MPEG-TS stream, which
//! more TVs take than HLS. One that doesn't say is taken for a TV that
//! plays H.264 and AAC in MPEG-TS.
//!
//! Subtitles go as SRT beside the media, which a renderer takes only with
//! the media, so changing them loads it again from where it is. DLNA has
//! no volume for the media apart from the renderer's own, so the block's
//! scales that: at the block's volume when cast, the renderer stays where
//! it was.

use std::collections::HashMap;
#[cfg(test)]
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use ::dlna::{
    Browser, Capability, Ended, Event, Fault, Found, Kind, Media, TransportState, Volume,
};
use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use parking_lot::Mutex;
use stream_server::{AudioCodec, Container, Profile, VideoCodec};

use super::target::{
    Device, Error, FOLLOW_POLL, Load, Session, Shows, State, Status, Subtitles, Target,
};

pub const KIND: &str = "DLNA";

/// What subtitles go as.
const SRT: &str = "text/srt";

/// How many times a seek put off until the renderer plays is tried then,
/// as it's asked what it's doing, twice a second.
const CATCH_UP_TRIES: u32 = 6;

/// The names renderers know each container by, for video and for sound,
/// the stream server's first.
const MIMES: &[(Container, &[&str])] = &[
    (Container::Mp4, &["video/mp4", "video/x-m4v", "video/mpeg4"]),
    (
        Container::Mp4,
        &["audio/mp4", "audio/m4a", "audio/x-m4a", "audio/mp4a"],
    ),
    (
        Container::MpegTs,
        &["video/mp2t", "video/mpeg", "video/vnd.dlna.mpeg-tts"],
    ),
    (
        Container::Matroska,
        &["video/x-matroska", "video/x-mkv", "video/mkv"],
    ),
    (Container::Matroska, &["audio/x-matroska"]),
    (Container::WebM, &["video/webm"]),
    (Container::WebM, &["audio/webm"]),
    (
        Container::Avi,
        &["video/x-msvideo", "video/avi", "video/divx"],
    ),
    (
        Container::Mp3,
        &[
            "audio/mpeg",
            "audio/mp3",
            "audio/mpeg3",
            "audio/x-mpeg",
            "audio/x-mpeg3",
            "audio/mpga",
        ],
    ),
    (
        Container::Aac,
        &[
            "audio/aac",
            "audio/x-aac",
            "audio/vnd.dlna.adts",
            "audio/aacp",
        ],
    ),
    (Container::Flac, &["audio/flac", "audio/x-flac"]),
    (
        Container::Ogg,
        &["audio/ogg", "application/ogg", "audio/x-ogg"],
    ),
    (
        Container::Wav,
        &["audio/wav", "audio/x-wav", "audio/wave", "audio/vnd.wave"],
    ),
];

/// What each renderer found plays, by its UDN: `None` where it doesn't
/// say.
type Known = HashMap<String, Option<Vec<Capability>>>;

pub struct DlnaTarget {
    browser: Browser,
    known: Arc<Mutex<Known>>,
}

impl DlnaTarget {
    /// Look for renderers on every network this computer is on.
    pub fn start() -> Result<Self, ::dlna::Error> {
        let (browser, found) = Browser::start()?;
        Ok(Self::with(browser, found))
    }

    /// For tests: look for the fake renderer that answers at `search`.
    #[cfg(test)]
    pub fn start_at(search: SocketAddr) -> Self {
        let (browser, found) = Browser::start_at(search).expect("looking for the fake renderer");
        Self::with(browser, found)
    }

    fn with(browser: Browser, found: UnboundedReceiver<Found>) -> Self {
        let known = Arc::new(Mutex::new(Known::new()));
        let learning = Arc::clone(&known);
        // Asking a renderer waits on the network, so it's done apart, as
        // each is found, until the browser goes.
        let spawned = thread::Builder::new()
            .name("dlna capabilities".into())
            .spawn(move || futures::executor::block_on(learn(found, &learning)));
        if let Err(err) = spawned {
            log::warn!("dlna: can't ask renderers what they play: {err}");
        }
        Self { browser, known }
    }
}

/// Ask each renderer found what it plays.
async fn learn(mut found: UnboundedReceiver<Found>, known: &Mutex<Known>) {
    while let Some(found) = found.next().await {
        match found {
            Found::Added(device) | Found::Changed(device) => {
                let capabilities = device
                    .capabilities()
                    .inspect_err(|err| {
                        log::info!("dlna: {} doesn't say what it plays: {err}", device.name);
                    })
                    .ok()
                    .filter(|capabilities| !capabilities.is_empty());
                known.lock().insert(device.id, capabilities);
            }
            Found::Removed(device) => {
                known.lock().remove(&device.id);
            }
        }
    }
}

impl Target for DlnaTarget {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// Those asked what they play, so each is offered for what it shows
    /// from the first.
    fn devices(&self) -> Vec<Device> {
        let known = self.known.lock();
        self.browser
            .devices()
            .into_iter()
            .filter_map(|device| {
                let shows = shows(known.get(&device.id)?.as_deref())?;
                Some(Device {
                    kind: KIND,
                    address: device.address()?,
                    id: device.id,
                    name: device.name,
                    shows,
                })
            })
            .collect()
    }

    fn connect(&self, device: &Device) -> Result<Box<dyn Session>, Error> {
        let renderer = self
            .browser
            .devices()
            .into_iter()
            .find(|renderer| renderer.id == device.id)
            .ok_or(Error::Gone)?;
        let capabilities = self.known.lock().get(&device.id).cloned().flatten();
        let (session, events) =
            ::dlna::Session::connect(&renderer).map_err(|err| Error::Failed(err.to_string()))?;
        Ok(Box::new(DlnaSession {
            session: Some(session),
            events,
            capabilities,
            media: None,
            starts_at_url: false,
            given: (0.0, Instant::now()),
            playing: false,
            played: false,
            finished: false,
            pending_seek: None,
            tries: 0,
            ended: None,
            loudness: None,
        }))
    }
}

/// `media` from `start`, by its URL, as the stream server makes it.
fn starting_at(media: &Media, start: f64) -> Media {
    if start <= 0.0 {
        return media.clone();
    }
    let separator = if media.url.contains('?') { '&' } else { '?' };
    Media {
        url: format!("{}{separator}start={start:.3}", media.url),
        duration: media.duration.map(|duration| (duration - start).max(0.0)),
        ..media.clone()
    }
}

/// HTTP's entries of what a renderer plays.
fn http(capabilities: Option<&[Capability]>) -> impl Iterator<Item = &Capability> {
    capabilities
        .unwrap_or_default()
        .iter()
        .filter(|capability| capability.is_http())
}

fn is_video(capability: &Capability) -> bool {
    capability.mime == "*" || capability.mime.to_ascii_lowercase().starts_with("video/")
}

fn is_sound(capability: &Capability) -> bool {
    let mime = capability.mime.to_ascii_lowercase();
    mime.starts_with("audio/") || mime == "application/ogg"
}

/// What a renderer shows, by what it plays. One that doesn't say is taken
/// for a TV; one that plays neither video nor sound, as a picture frame,
/// is left out.
fn shows(capabilities: Option<&[Capability]>) -> Option<Shows> {
    if capabilities.is_none() || http(capabilities).any(is_video) {
        Some(Shows::Video)
    } else if http(capabilities).any(is_sound) {
        Some(Shows::Sound)
    } else {
        None
    }
}

/// How the stream server serves a renderer that plays `capabilities`.
fn profile(capabilities: Option<&[Capability]>) -> Profile {
    let listed: Vec<&Capability> = http(capabilities).collect();
    if listed.is_empty() || listed.iter().any(|capability| capability.mime == "*") {
        return Profile {
            files: vec![Container::MpegTs],
            ..stream_of_h264()
        };
    }
    let takes = |mime: &str| listed.iter().any(|capability| capability.takes(mime));
    let takes_any = |mimes: &[&str]| mimes.iter().any(|mime| takes(mime));
    let profiles: Vec<String> = listed
        .iter()
        .filter_map(|capability| capability.profile.as_deref())
        .map(str::to_ascii_uppercase)
        .collect();
    let named = |part: &str| profiles.iter().any(|profile| profile.contains(part));
    let shows_video = listed.iter().any(|capability| is_video(capability));

    let mut files = Vec::new();
    for (container, mimes) in MIMES {
        if !files.contains(container) && takes_any(mimes) {
            files.push(*container);
        }
    }

    let mut video = vec![VideoCodec::H264];
    if named("HEVC") || takes_any(&["video/hevc", "video/h265"]) {
        video.push(VideoCodec::Hevc);
    }
    if ["MPEG_TS", "MPEG_PS", "MPEG1", "MPEG2"]
        .iter()
        .any(|part| named(part))
    {
        video.push(VideoCodec::Mpeg2);
    }
    if files.contains(&Container::WebM) {
        video.extend([VideoCodec::Vp9, VideoCodec::Vp8]);
    }

    let mut audio = Vec::new();
    let has = |container| files.contains(&container);
    if shows_video || has(Container::Mp4) || has(Container::Aac) {
        audio.push(AudioCodec::Aac);
    }
    if has(Container::Mp3) || named("MP3") {
        audio.push(AudioCodec::Mp3);
    }
    if named("AC3") || takes_any(&["audio/ac3", "audio/x-ac3", "audio/vnd.dolby.dd-raw"]) {
        audio.push(AudioCodec::Ac3);
    }
    if named("EAC3") || named("DDP") || takes_any(&["audio/eac3", "audio/x-eac3"]) {
        audio.push(AudioCodec::Eac3);
    }
    if has(Container::Flac) {
        audio.push(AudioCodec::Flac);
    }
    if has(Container::Wav) || takes("audio/l16") {
        audio.push(AudioCodec::Pcm);
    }
    if has(Container::Ogg) || has(Container::WebM) || takes("audio/x-vorbis") {
        audio.push(AudioCodec::Vorbis);
    }
    if has(Container::WebM) || takes("audio/opus") {
        audio.push(AudioCodec::Opus);
    }
    if audio.is_empty() {
        audio.push(AudioCodec::Aac);
    }
    // A speaker takes files, and a stream of the sound as internet radio
    // sends it, AAC or FLAC, where it takes those.
    let sound_stream: Vec<Container> = [Container::Aac, Container::Flac]
        .into_iter()
        .filter(|container| !shows_video && has(*container))
        .collect();
    let surround = audio
        .iter()
        .any(|codec| matches!(codec, AudioCodec::Ac3 | AudioCodec::Eac3))
        || named("MULT5");

    Profile {
        files,
        // A TV takes a stream of MPEG-TS whether it lists one or not.
        stream: shows_video,
        sound_stream,
        max_size: Some(if named("UHD") || named("2160") {
            (3840, 2160)
        } else {
            (1920, 1080)
        }),
        max_channels: if surround { 6 } else { 2 },
        video,
        audio,
        ..stream_of_h264()
    }
}

/// H.264 and AAC in a stream of MPEG-TS, which a TV that says nothing of
/// what it plays most likely plays. It fetches only what this computer
/// serves it, as many can't fetch HTTPS.
fn stream_of_h264() -> Profile {
    Profile {
        files: Vec::new(),
        hls: None,
        stream: true,
        fetches_urls: false,
        video: vec![VideoCodec::H264],
        audio: vec![AudioCodec::Aac],
        ..Profile::default()
    }
}

/// `mime` as the renderer calls it, where it lists it by another name:
/// Samsung's and LG's TVs list MPEG-TS as `video/mpeg`, not `video/mp2t`.
fn named_as(mime: &str, capabilities: Option<&[Capability]>) -> String {
    let takes = |mime: &str| http(capabilities).any(|capability| capability.takes(mime));
    if capabilities.is_none() || takes(mime) {
        return mime.to_owned();
    }
    MIMES
        .iter()
        .find(|(_, names)| names.iter().any(|name| name.eq_ignore_ascii_case(mime)))
        .and_then(|(_, names)| names.iter().find(|name| takes(name)))
        .map_or_else(|| mime.to_owned(), |name| (*name).to_owned())
}

fn kind_of(mime: &str) -> Kind {
    let mime = mime.to_ascii_lowercase();
    if mime.starts_with("audio/") || mime == "application/ogg" {
        Kind::Audio
    } else if mime.starts_with("image/") {
        Kind::Image
    } else {
        Kind::Video
    }
}

fn srt(subtitles: &Subtitles) -> ::dlna::Subtitles {
    ::dlna::Subtitles {
        url: subtitles.srt_url.clone(),
        mime: SRT.into(),
    }
}

/// How a session's end is told.
fn over(ended: Ended) -> Error {
    match ended {
        Ended::Gone => Error::Gone,
        Ended::Replaced => Error::Replaced,
    }
}

/// The block's volume on a renderer that has only its own: the block's
/// scales the renderer's.
#[derive(Clone, Copy, Debug)]
struct Loudness {
    /// The renderer's volume at the block's full one, 0 to 1.
    full: f32,
    /// The block's volume, as last set.
    block: f32,
    /// The renderer's, as last set or heard.
    level: f32,
    muted: bool,
}

impl Loudness {
    fn new(heard: Volume, block: f32) -> Self {
        Self {
            full: Self::full_at(heard.level, block),
            block,
            level: heard.level,
            muted: heard.muted,
        }
    }

    /// The renderer's volume at the block's full one, where it is at `level`
    /// with the block at `block`.
    fn full_at(level: f32, block: f32) -> f32 {
        if block > 0.01 {
            (level / block).min(1.0)
        } else {
            level
        }
    }

    /// Its volume, turned on the renderer: the block's now scales from
    /// there.
    fn heard(&mut self, heard: Volume) {
        if (heard.level - self.level).abs() > 0.005 {
            self.full = Self::full_at(heard.level, self.block);
            self.level = heard.level;
        }
        self.muted = heard.muted;
    }
}

struct DlnaSession {
    /// Until stopped.
    session: Option<::dlna::Session>,
    events: UnboundedReceiver<Event>,
    capabilities: Option<Vec<Capability>>,
    /// What it was last given, to give it again with other subtitles, or
    /// from elsewhere where it starts at its URL.
    media: Option<Media>,
    /// It's given what starts at its URL (`Load::starts_at_url`).
    starts_at_url: bool,
    /// Where in the media what it was last given starts, and when it was
    /// given: what it said before then is of what it played before.
    given: (f64, Instant),
    /// Whether it was told to play.
    playing: bool,
    /// Whether it has played what it was given, so that stopping is the
    /// end of it.
    played: bool,
    finished: bool,
    /// Where it was told to go while it couldn't, as many can't while
    /// stopped, to go once it plays.
    pending_seek: Option<f64>,
    /// How many times that was tried since.
    tries: u32,
    ended: Option<Ended>,
    /// Once it's given media; `None` there too where it has no volume.
    loudness: Option<Option<Loudness>>,
}

impl DlnaSession {
    fn session(&self) -> Result<&::dlna::Session, Error> {
        self.session.as_ref().ok_or(Error::Gone)
    }

    /// Take in what it said since last asked.
    fn hear(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Status(status) => match status.state {
                    TransportState::Playing => self.played = true,
                    TransportState::Stopped | TransportState::NoMedia if self.played => {
                        self.finished = true;
                    }
                    _ => {}
                },
                Event::Volume(volume) => {
                    if let Some(Some(loudness)) = &mut self.loudness {
                        loudness.heard(volume);
                    }
                }
                Event::Ended(ended) => self.ended = Some(ended),
            }
        }
    }

    /// What went wrong, as the block is told: a session that ended says
    /// how.
    fn failed(&mut self, err: ::dlna::Error) -> Error {
        match err {
            ::dlna::Error::Ended => {
                self.hear();
                over(self.ended.unwrap_or(Ended::Gone))
            }
            err => Error::Failed(err.to_string()),
        }
    }

    /// Give it `media` to play from `start`, and play it with `play`.
    fn give(&mut self, media: Media, play: bool, start: f64) -> Result<(), Error> {
        // What it said till now is of what it played before.
        self.hear();
        self.played = false;
        self.finished = false;
        self.pending_seek = None;
        let given = if self.starts_at_url {
            starting_at(&media, start)
        } else {
            media.clone()
        };
        let loaded = self.session()?.load(&given);
        loaded.map_err(|err| self.failed(err))?;
        self.given = (if self.starts_at_url { start } else { 0.0 }, Instant::now());
        self.media = Some(media);
        self.playing = false;
        if start > 0.0 && !self.starts_at_url {
            self.seek(start)?;
        }
        if play {
            self.play()?;
        }
        Ok(())
    }

    /// Go where it was told to while it couldn't, now it can.
    fn catch_up(&mut self) -> Result<(), Error> {
        let Some(time) = self.pending_seek else {
            return Ok(());
        };
        let sought = self.session()?.seek(time);
        match sought {
            Ok(()) => self.pending_seek = None,
            // Still on its way, likely.
            Err(::dlna::Error::Upnp { .. }) if self.tries + 1 < CATCH_UP_TRIES => self.tries += 1,
            Err(err) => {
                log::debug!("dlna: seeking once it plays: {err}");
                self.pending_seek = None;
            }
        }
        Ok(())
    }
}

impl Session for DlnaSession {
    fn profile(&self) -> Profile {
        profile(self.capabilities.as_deref())
    }

    fn load(&mut self, load: &Load) -> Result<(), Error> {
        let mime = named_as(&load.mime, self.capabilities.as_deref());
        let media = Media {
            url: load.url.clone(),
            kind: kind_of(&mime),
            mime,
            // The fourth field of `http-get:*:{mime}:{features}`.
            features: load
                .protocol_info
                .splitn(4, ':')
                .nth(3)
                .filter(|features| !features.is_empty() && *features != "*")
                .map(str::to_owned),
            title: Some(load.title.clone()).filter(|title| !title.is_empty()),
            duration: load.duration,
            size: None,
            subtitles: load.subtitles.as_ref().map(srt),
        };
        if self.loudness.is_none() {
            let session = self.session()?;
            let loudness = session
                .volume()
                .inspect_err(|err| log::info!("dlna: no volume to set: {err}"))
                .ok()
                .map(|heard| Loudness::new(heard, load.volume));
            if let Some(loudness) = loudness
                && load.muted
                && !loudness.muted
            {
                let muted = session.set_muted(true);
                muted.map_err(|err| self.failed(err))?;
            }
            self.loudness = Some(loudness.map(|loudness| Loudness {
                muted: loudness.muted || load.muted,
                ..loudness
            }));
        }
        self.starts_at_url = load.starts_at_url;
        self.give(media, load.autoplay, load.start)?;
        // Asked often while a picture follows it, once it has the media:
        // asked before, it would say what it played before.
        self.session()?
            .poll_every(load.picture_here.then_some(FOLLOW_POLL));
        Ok(())
    }

    fn play(&mut self) -> Result<(), Error> {
        let played = self.session()?.play();
        played.map_err(|err| self.failed(err))?;
        self.playing = true;
        self.finished = false;
        self.catch_up()
    }

    fn pause(&mut self) -> Result<(), Error> {
        self.playing = false;
        let paused = self.session()?.pause();
        match paused {
            // Stopped, or not started: as good as paused.
            Err(::dlna::Error::Upnp {
                fault: Fault::TransitionNotAvailable,
                ..
            }) => Ok(()),
            paused => paused.map_err(|err| self.failed(err)),
        }
    }

    fn seek(&mut self, time: f64) -> Result<(), Error> {
        if self.starts_at_url {
            let Some(media) = self.media.clone() else {
                return Ok(());
            };
            let play = self.playing && !self.finished;
            return self.give(media, play, time);
        }
        self.hear();
        let waiting = !self.played
            || self.finished
            || matches!(
                self.session()?.status().state,
                TransportState::Stopped | TransportState::NoMedia | TransportState::Transitioning
            );
        self.finished = false;
        let sought = self.session()?.seek(time);
        match sought {
            Ok(()) => {
                self.pending_seek = None;
                Ok(())
            }
            // Many won't before they play, each refusing its own way
            // (Rygel's say 711), and some not while they get going: it
            // goes there once it plays.
            Err(::dlna::Error::Upnp { fault, .. })
                if waiting || fault == Fault::TransitionNotAvailable =>
            {
                self.pending_seek = Some(time);
                self.tries = 0;
                Ok(())
            }
            Err(err) => Err(self.failed(err)),
        }
    }

    fn set_volume(&mut self, volume: f32, muted: bool) -> Result<(), Error> {
        self.hear();
        let Some(Some(mut loudness)) = self.loudness else {
            return Ok(());
        };
        let session = self.session()?;
        let mut done = Ok(());
        if muted != loudness.muted {
            done = session.set_muted(muted);
            loudness.muted = muted;
        }
        if done.is_ok() && (volume - loudness.block).abs() > f32::EPSILON {
            let level = (loudness.full * volume).clamp(0.0, 1.0);
            done = session.set_volume(level);
            loudness.block = volume;
            loudness.level = level;
        }
        self.loudness = Some(Some(loudness));
        done.map_err(|err| self.failed(err))
    }

    fn set_subtitles(&mut self, subtitles: Option<&Subtitles>) -> Result<(), Error> {
        let Some(mut media) = self.media.clone() else {
            return Ok(());
        };
        let subtitles = subtitles.map(srt);
        if subtitles.is_none() && media.subtitles.is_none() {
            return Ok(());
        }
        // Even at the same URL, they may be another track's now.
        media.subtitles = subtitles;
        self.hear();
        let status = self.session()?.status();
        let position = self.pending_seek.or(status.position).unwrap_or(0.0);
        let playing = self.playing && !self.finished;
        self.give(media, playing, position)
    }

    fn stop(&mut self) -> Result<(), Error> {
        if let Some(session) = self.session.take() {
            session.stop();
        }
        Ok(())
    }

    fn status(&mut self) -> Result<Status, Error> {
        self.hear();
        if let Some(ended) = self.ended {
            return Err(over(ended));
        }
        let status = self.session()?.status();
        if matches!(
            status.state,
            TransportState::Playing | TransportState::Paused
        ) {
            self.catch_up()?;
        }
        let media_duration = self.media.as_ref().and_then(|media| media.duration);
        // A speaker says how long a stream is as how much it has played.
        let duration = match self.starts_at_url {
            true => media_duration,
            false => status
                .duration
                .filter(|duration| *duration > 0.0)
                .or(media_duration),
        };
        let (start, given_at) = self.given;
        let since_given = status.position_at.is_some_and(|at| at >= given_at);
        let said = match self.starts_at_url {
            // Till it says otherwise, it's where it was given it from.
            true => Some(start + status.position.filter(|_| since_given).unwrap_or(0.0)),
            false => status.position,
        };
        let state = if self.finished {
            State::Ended
        } else {
            match status.state {
                TransportState::Playing => State::Playing,
                TransportState::Paused => State::Paused,
                TransportState::Transitioning => State::Buffering,
                _ if self.playing => State::Buffering,
                _ => State::Paused,
            }
        };
        let position = match state {
            State::Ended => duration.or(said),
            _ => self.pending_seek.or(said),
        };
        // Where it was heard to be, not where it's to go or has ended.
        let heard = state != State::Ended && self.pending_seek.is_none();
        Ok(Status {
            state,
            position: position.unwrap_or(0.0),
            at: status.position_at.filter(|_| heard),
            duration,
        })
    }
}

#[cfg(test)]
#[path = "tests/dlna.rs"]
mod tests;
