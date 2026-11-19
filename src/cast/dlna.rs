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
//! no volume for the media apart from the renderer's own, so the tile's
//! scales that: at the tile's volume when cast, the renderer stays where
//! it was.

use std::collections::HashMap;
#[cfg(test)]
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;

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

/// The tile's volume on a renderer that has only its own: the tile's
/// scales the renderer's.
#[derive(Clone, Copy, Debug)]
struct Loudness {
    /// The renderer's volume at the tile's full one, 0 to 1.
    full: f32,
    /// The tile's volume, as last set.
    tile: f32,
    /// The renderer's, as last set or heard.
    level: f32,
    muted: bool,
}

impl Loudness {
    fn new(heard: Volume, tile: f32) -> Self {
        Self {
            full: Self::full_at(heard.level, tile),
            tile,
            level: heard.level,
            muted: heard.muted,
        }
    }

    /// The renderer's volume at the tile's full one, where it is at `level`
    /// with the tile at `tile`.
    fn full_at(level: f32, tile: f32) -> f32 {
        if tile > 0.01 {
            (level / tile).min(1.0)
        } else {
            level
        }
    }

    /// Its volume, turned on the renderer: the tile's now scales from
    /// there.
    fn heard(&mut self, heard: Volume) {
        if (heard.level - self.level).abs() > 0.005 {
            self.full = Self::full_at(heard.level, self.tile);
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
    /// What it was last given, to give it again with other subtitles.
    media: Option<Media>,
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

    /// What went wrong, as the tile is told: a session that ended says
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
        let loaded = self.session()?.load(&media);
        loaded.map_err(|err| self.failed(err))?;
        self.media = Some(media);
        self.playing = false;
        if start > 0.0 {
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
        if done.is_ok() && (volume - loudness.tile).abs() > f32::EPSILON {
            let level = (loudness.full * volume).clamp(0.0, 1.0);
            done = session.set_volume(level);
            loudness.tile = volume;
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
        let duration = status
            .duration
            .filter(|duration| *duration > 0.0)
            .or_else(|| self.media.as_ref().and_then(|media| media.duration));
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
            State::Ended => duration.or(status.position),
            _ => self.pending_seek.or(status.position),
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
mod tests {
    use super::*;

    fn listed(name: &str) -> Option<Vec<Capability>> {
        let list = match name {
            "samsung" => include_str!("../../crates/dlna/fixtures/samsung-protocol-info.txt"),
            "lg" => include_str!("../../crates/dlna/fixtures/lg-protocol-info.txt"),
            "sonos" => include_str!("../../crates/dlna/fixtures/sonos-protocol-info.txt"),
            "devialet" => include_str!("../../crates/dlna/fixtures/devialet-protocol-info.txt"),
            _ => unreachable!(),
        };
        Some(Capability::parse_list(list))
    }

    #[test]
    fn a_tv_shows_video_and_a_speaker_sound() {
        assert_eq!(shows(listed("samsung").as_deref()), Some(Shows::Video));
        assert_eq!(shows(listed("lg").as_deref()), Some(Shows::Video));
        assert_eq!(shows(listed("sonos").as_deref()), Some(Shows::Sound));
        assert_eq!(shows(listed("devialet").as_deref()), Some(Shows::Sound));
        assert_eq!(shows(None), Some(Shows::Video), "one that doesn't say");
        let frame = Capability::parse_list("http-get:*:image/jpeg:*,http-get:*:image/png:*");
        assert_eq!(shows(Some(&frame)), None, "a picture frame");
        let elsewhere = Capability::parse_list("rtsp-rtp-udp:*:video/H264:*");
        assert_eq!(shows(Some(&elsewhere)), None, "nothing over HTTP");
    }

    #[test]
    fn a_tv_gets_the_files_it_lists_and_else_a_stream() {
        let samsung = profile(listed("samsung").as_deref());
        assert_eq!(
            samsung.files,
            [
                Container::Mp4,
                Container::MpegTs,
                Container::Matroska,
                Container::Avi,
                Container::Mp3,
                Container::Flac,
            ]
        );
        assert!(samsung.stream);
        assert!(
            samsung.sound_stream.is_empty(),
            "a TV gets the MPEG-TS stream"
        );
        assert_eq!(samsung.hls, None, "MPEG-TS rather than HLS");
        assert!(!samsung.fetches_urls);
        assert_eq!(samsung.video, [VideoCodec::H264, VideoCodec::Mpeg2]);
        assert_eq!(
            samsung.audio,
            [
                AudioCodec::Aac,
                AudioCodec::Mp3,
                AudioCodec::Flac,
                AudioCodec::Pcm
            ]
        );
        assert_eq!(samsung.max_channels, 6, "AAC_MULT5");
        assert_eq!(samsung.max_size, Some((1920, 1080)));

        let lg = profile(listed("lg").as_deref());
        assert_eq!(
            lg.files,
            [
                Container::Mp4,
                Container::MpegTs,
                Container::Matroska,
                Container::WebM,
                Container::Mp3,
                Container::Aac,
                Container::Flac,
            ]
        );
        assert_eq!(
            lg.video,
            [
                VideoCodec::H264,
                VideoCodec::Mpeg2,
                VideoCodec::Vp9,
                VideoCodec::Vp8
            ]
        );
        assert_eq!(
            lg.audio,
            [
                AudioCodec::Aac,
                AudioCodec::Mp3,
                AudioCodec::Ac3,
                AudioCodec::Flac,
                AudioCodec::Vorbis,
                AudioCodec::Opus
            ]
        );
        assert_eq!(lg.max_channels, 6, "AC3");
    }

    #[test]
    fn a_speaker_gets_files_and_a_stream_of_its_sound() {
        let sonos = profile(listed("sonos").as_deref());
        assert_eq!(
            sonos.files,
            [
                Container::Mp4,
                Container::Mp3,
                Container::Flac,
                Container::Wav
            ]
        );
        assert!(!sonos.stream, "no stream of MPEG-TS for a speaker");
        assert_eq!(sonos.sound_stream, [Container::Flac], "it lists no AAC");
        assert_eq!(sonos.hls, None);
        assert_eq!(
            sonos.audio,
            [
                AudioCodec::Aac,
                AudioCodec::Mp3,
                AudioCodec::Flac,
                AudioCodec::Pcm
            ]
        );

        let devialet = profile(listed("devialet").as_deref());
        assert!(!devialet.stream);
        assert_eq!(devialet.sound_stream, [Container::Aac, Container::Flac]);
        assert_eq!(
            devialet.files,
            [
                Container::Mp4,
                Container::Mp3,
                Container::Aac,
                Container::Flac,
                Container::Ogg,
                Container::Wav,
            ]
        );
        assert!(devialet.audio.contains(&AudioCodec::Opus));
        assert!(devialet.audio.contains(&AudioCodec::Ac3), "audio/x-ac3");
    }

    #[test]
    fn one_that_doesnt_say_gets_h264_and_aac_in_mpeg_ts() {
        let anything = Capability::parse_list("http-get:*:*:*");
        for profile in [profile(None), profile(Some(&anything))] {
            assert_eq!(profile.files, [Container::MpegTs]);
            assert!(profile.stream);
            assert_eq!(profile.hls, None);
            assert_eq!(profile.video, [VideoCodec::H264]);
            assert_eq!(profile.audio, [AudioCodec::Aac]);
            assert_eq!(profile.max_size, Some((1920, 1080)));
        }
    }

    #[test]
    fn a_4k_tv_gets_4k() {
        let tv = Capability::parse_list(
            "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD_AAC,\
             http-get:*:video/mp4:DLNA.ORG_PN=HEVC_MP4_MP_UHD_AAC",
        );
        let profile = profile(Some(&tv));
        assert_eq!(profile.max_size, Some((3840, 2160)));
        assert_eq!(profile.video, [VideoCodec::H264, VideoCodec::Hevc]);
    }

    #[test]
    fn a_mime_goes_by_the_name_the_renderer_lists() {
        let samsung = listed("samsung");
        let lg = listed("lg");
        let sonos = listed("sonos");
        let devialet = listed("devialet");
        assert_eq!(named_as("video/mp2t", samsung.as_deref()), "video/mpeg");
        assert_eq!(named_as("video/mp2t", lg.as_deref()), "video/mpeg");
        assert_eq!(named_as("video/mp4", samsung.as_deref()), "video/mp4");
        assert_eq!(named_as("audio/flac", samsung.as_deref()), "audio/x-flac");
        assert_eq!(named_as("audio/mpeg", sonos.as_deref()), "audio/mpeg");
        assert_eq!(named_as("audio/mp4", devialet.as_deref()), "audio/mp4");
        assert_eq!(named_as("audio/aac", lg.as_deref()), "audio/vnd.dlna.adts");
        assert_eq!(
            named_as("video/webm", samsung.as_deref()),
            "video/webm",
            "as it is, where it lists no other name"
        );
        assert_eq!(named_as("video/mp2t", None), "video/mp2t");
    }

    #[test]
    fn the_tiles_volume_scales_the_renderers() {
        let at = |level, muted| Volume { level, muted };
        let mut loudness = Loudness::new(at(0.3, false), 0.5);
        assert!((loudness.full - 0.6).abs() < 1e-6);
        assert!(
            (Loudness::new(at(0.8, false), 0.5).full - 1.0).abs() < 1e-6,
            "never louder than it goes"
        );
        assert!(
            (Loudness::new(at(0.3, false), 0.0).full - 0.3).abs() < 1e-6,
            "a silent tile leaves the renderer where it is"
        );

        loudness.heard(at(0.4, true));
        assert!((loudness.full - 0.8).abs() < 1e-6, "turned on its remote");
        assert!(loudness.muted);
        loudness.heard(at(0.401, false));
        assert!((loudness.full - 0.8).abs() < 1e-6, "as it was set");
    }
}
