//! Google Cast devices, through the `chromecast` crate: Chromecasts,
//! Google TVs and TVs with Cast built in, offered for video, and Nest
//! speakers and speaker groups, for sound. What one plays goes by the
//! model it says it is, as Google lists its own; any other is taken to
//! play what they all do, H.264 and AAC.
//!
//! Subtitles go as a WebVTT track beside the media, looking as Settings
//! say. Turning them off and on, or how they look, is changed on the
//! track; other subtitles, which a track can't be changed to, load the
//! media again from where it is. The tile's volume is the stream's own,
//! so the device's stays as it is.

use ::chromecast::{
    Browser, Color, Edge, Ended, Event, FontFamily, HlsSegments, IdleReason, Media, MediaStatus,
    PlayerState, StreamType, TextTrack, TextTrackStyle,
};
use futures::channel::mpsc::UnboundedReceiver;
use stream_server::{AudioCodec, Container, Hdr, Profile, Segments, SubtitleSource, VideoCodec};

use super::target::{Device, Error, Load, Session, Shows, State, Status, Subtitles, Target};
use crate::media::tracks::language_tag;
use crate::settings::{Rgb, SubtitleEdge, SubtitleStyle, SubtitleWeight};

pub const KIND: &str = "Chromecast";

/// The subtitle track's id among what's cast.
const TRACK: u32 = 1;

/// What subtitles go as.
const WEBVTT: &str = "text/vtt";

const FULL_HD: (u32, u32) = (1920, 1080);
const UHD: (u32, u32) = (3840, 2160);

/// The containers every Cast device plays as files.
const FILES: [Container; 7] = [
    Container::Mp4,
    Container::WebM,
    Container::Mp3,
    Container::Aac,
    Container::Flac,
    Container::Ogg,
    Container::Wav,
];

/// The sound every Cast device decodes, AAC first.
const SOUND: [AudioCodec; 6] = [
    AudioCodec::Aac,
    AudioCodec::Mp3,
    AudioCodec::Opus,
    AudioCodec::Vorbis,
    AudioCodec::Flac,
    AudioCodec::Pcm,
];

pub struct ChromecastTarget {
    /// Looking for devices until dropped; tests have their fakes instead.
    browser: Option<Browser>,
    #[cfg(test)]
    fakes: Vec<::chromecast::Device>,
}

impl ChromecastTarget {
    /// Look for devices on every network this computer is on.
    pub fn start() -> ::chromecast::Result<Self> {
        // What's found is asked for when wanted, so news of it goes unheard.
        let (browser, _found) = Browser::start()?;
        Ok(Self {
            browser: Some(browser),
            #[cfg(test)]
            fakes: Vec::new(),
        })
    }

    /// For tests: these fake devices, as if found.
    #[cfg(test)]
    pub fn with(fakes: Vec<::chromecast::Device>) -> Self {
        Self {
            browser: None,
            fakes,
        }
    }

    fn found(&self) -> Vec<::chromecast::Device> {
        let found = self
            .browser
            .as_ref()
            .map(Browser::devices)
            .unwrap_or_default();
        #[cfg(test)]
        let found = [found, self.fakes.clone()].concat();
        found
    }
}

/// What a device shows: a group of speakers, or one, sound only; one that
/// does neither isn't offered.
fn shows(device: &::chromecast::Device) -> Option<Shows> {
    if device.shows_video() {
        Some(Shows::Video)
    } else if device.plays_audio() {
        Some(Shows::Sound)
    } else {
        None
    }
}

impl Target for ChromecastTarget {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn devices(&self) -> Vec<Device> {
        self.found()
            .into_iter()
            .filter_map(|device| {
                Some(Device {
                    kind: KIND,
                    shows: shows(&device)?,
                    address: device.addresses.first()?.ip(),
                    id: device.id,
                    name: device.name,
                })
            })
            .collect()
    }

    fn connect(&self, device: &Device) -> Result<Box<dyn Session>, Error> {
        let device = self
            .found()
            .into_iter()
            .find(|found| found.id == device.id)
            .ok_or(Error::Gone)?;
        let profile = profile(&device.model, device.shows_video());
        let (session, events) = ::chromecast::Session::connect(&device)
            .map_err(|err| Error::Failed(err.to_string()))?;
        Ok(Box::new(ChromecastSession {
            session: Some(session),
            events,
            profile,
            media: None,
            subtitles: None,
            style: None,
            given: 0,
            media_session: None,
            state: PlayerState::Idle(None),
            position: 0.0,
            duration: None,
            volume: 1.0,
            muted: false,
            playing: false,
            finished: false,
            start_over: None,
            ended: None,
        }))
    }
}

/// How the stream server serves a device, by the model it says it is: a
/// speaker, sound only; Google's own, what Google lists for each; any
/// other, H.264 and AAC. A Chromecast takes its subtitles beside HLS, not
/// in it.
fn profile(model: &str, shows_video: bool) -> Profile {
    let unknown = Profile {
        hls_subtitles: false,
        ..Profile::default()
    };
    // The first three, which say no more than this.
    let first = Profile {
        files: FILES.to_vec(),
        video: vec![VideoCodec::H264, VideoCodec::Vp8],
        audio: SOUND.to_vec(),
        ..unknown.clone()
    };
    // HEVC, VP9 and HDR, which take HLS of fragmented MP4.
    let newer = |size, video: &[VideoCodec], hdr: &[Hdr]| Profile {
        hls: Some(Segments::Fmp4),
        video: video.to_vec(),
        max_size: Some(size),
        max_bit_depth: 10,
        hdr: hdr.to_vec(),
        ..first.clone()
    };
    const ULTRA: [VideoCodec; 4] = [
        VideoCodec::H264,
        VideoCodec::Hevc,
        VideoCodec::Vp9,
        VideoCodec::Vp8,
    ];
    const STREAMER: [VideoCodec; 5] = [
        VideoCodec::H264,
        VideoCodec::Hevc,
        VideoCodec::Vp9,
        VideoCodec::Av1,
        VideoCodec::Vp8,
    ];
    const GOOGLE_TV: [Hdr; 2] = [Hdr::Hdr10, Hdr::Hlg];

    if !shows_video {
        return Profile {
            files: FILES.to_vec(),
            audio: SOUND.to_vec(),
            ..unknown
        };
    }
    if model == "Chromecast" {
        first
    } else if model.contains("Ultra") {
        newer(UHD, &ULTRA, &[Hdr::Hdr10])
    } else if model.contains("Streamer") {
        newer(UHD, &STREAMER, &GOOGLE_TV)
    } else if model == "Chromecast HD" || model.contains("Google TV (HD)") {
        newer(FULL_HD, &ULTRA, &GOOGLE_TV)
    } else if model.contains("Google TV") {
        newer(UHD, &ULTRA, &GOOGLE_TV)
    } else {
        unknown
    }
}

/// Subtitles as Settings say they look.
fn style(style: &SubtitleStyle) -> TextTrackStyle {
    let color = |rgb: Rgb, opacity: f32| {
        let [_, red, green, blue] = rgb.0.to_be_bytes();
        // Within 0 to 255 once clamped.
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color([red, green, blue, alpha])
    };
    TextTrackStyle {
        foreground: color(style.color, 1.0),
        background: color(style.background, style.background_opacity),
        edge: match style.edge {
            SubtitleEdge::None => Edge::None,
            SubtitleEdge::Outline => Edge::Outline,
            SubtitleEdge::Shadow => Edge::DropShadow,
        },
        edge_color: color(style.edge_color, 1.0),
        font_scale: style.size,
        font_family: Some(style.font.clone()).filter(|font| !font.is_empty()),
        generic_family: FontFamily::SansSerif,
        bold: style.weight == SubtitleWeight::Bold,
        italic: false,
    }
}

/// The subtitles as a track, given anew the `given`th time at a URL of
/// their own, as the device keeps what it fetched.
fn track(subtitles: &Subtitles, given: u32) -> TextTrack {
    TextTrack {
        id: TRACK,
        url: if given == 0 {
            subtitles.webvtt_url.clone()
        } else {
            format!("{}?{given}", subtitles.webvtt_url)
        },
        content_type: WEBVTT.into(),
        name: subtitles.name.clone(),
        language: subtitles.language.as_deref().map(language_tag),
    }
}

/// How a session's end is told.
fn over(ended: Ended) -> Error {
    match ended {
        Ended::Gone => Error::Gone,
        Ended::Stopped => Error::Stopped,
        Ended::Replaced => Error::Replaced,
    }
}

struct ChromecastSession {
    /// Until stopped.
    session: Option<::chromecast::Session>,
    events: UnboundedReceiver<Event>,
    profile: Profile,
    /// What it was last given, to give it again.
    media: Option<Media>,
    /// Where the subtitles it was given come from, and whether they show.
    subtitles: Option<(SubtitleSource, bool)>,
    style: Option<TextTrackStyle>,
    /// How many times it was given other subtitles.
    given: u32,
    /// The device's id for what it was last given.
    media_session: Option<u64>,
    /// What it said it does with that, last.
    state: PlayerState,
    position: f64,
    duration: Option<f64>,
    /// The tile's volume, which is the stream's.
    volume: f32,
    muted: bool,
    /// Whether it was told to play.
    playing: bool,
    /// Whether it played to the end, after which it's given the media again
    /// to play it.
    finished: bool,
    /// Where it was sent since, to play from.
    start_over: Option<f64>,
    ended: Option<Ended>,
}

impl ChromecastSession {
    fn session(&self) -> Result<&::chromecast::Session, Error> {
        self.session.as_ref().ok_or(Error::Gone)
    }

    /// Take in what it said since last asked.
    fn hear(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Media(status) => self.heard(&status),
                Event::Volume(_) => {}
                Event::Ended(ended) => self.ended = Some(ended),
            }
        }
    }

    /// Take in what it says of what it plays; what it was given before
    /// doesn't count.
    fn heard(&mut self, status: &MediaStatus) {
        if Some(status.media_session) != self.media_session {
            return;
        }
        self.state = status.state;
        self.position = status.position;
        if status.duration.is_some() {
            self.duration = status.duration;
        }
        if status.state == PlayerState::Idle(Some(IdleReason::Finished)) {
            self.finished = true;
        }
    }

    /// How the cast is over, if it is: it went, stopped, or couldn't play
    /// what it was given.
    fn over(&self) -> Option<Error> {
        if let Some(ended) = self.ended {
            return Some(over(ended));
        }
        match self.state {
            PlayerState::Idle(Some(IdleReason::Error)) => Some(Error::Unplayable),
            PlayerState::Idle(Some(IdleReason::Cancelled)) => Some(Error::Stopped),
            _ => None,
        }
    }

    /// What went wrong, as the tile is told: a session that ended says
    /// how.
    fn failed(&mut self, err: ::chromecast::Error) -> Error {
        match err {
            ::chromecast::Error::Ended => {
                self.hear();
                over(self.ended.unwrap_or(Ended::Gone))
            }
            err => Error::Failed(err.to_string()),
        }
    }

    /// Take in the answer to a command.
    fn answered(&mut self, answer: ::chromecast::Result<MediaStatus>) -> Result<(), Error> {
        let status = answer.map_err(|err| self.failed(err))?;
        self.heard(&status);
        Ok(())
    }

    /// Give it `media` to play from `start`, and play it with `play`.
    fn give(&mut self, media: Media, play: bool, start: f64) -> Result<(), Error> {
        // What it said till now is of what it played before.
        self.hear();
        let shown = self.subtitles.as_ref().is_some_and(|(_, shown)| *shown)
            && !media.text_tracks.is_empty();
        let load = ::chromecast::Load {
            start,
            autoplay: play,
            active_tracks: if shown { vec![TRACK] } else { Vec::new() },
            style: self.style.clone(),
        };
        let loaded = self.session()?.load(&media, &load);
        let status = loaded.map_err(|err| self.failed(err))?;
        self.media_session = Some(status.media_session);
        self.duration = media.duration;
        self.media = Some(media);
        self.playing = play;
        self.finished = false;
        self.start_over = None;
        self.heard(&status);
        // Each starts at full volume.
        if self.volume < 1.0 || self.muted {
            self.turn()?;
        }
        Ok(())
    }

    /// Set the stream's volume to the tile's.
    fn turn(&mut self) -> Result<(), Error> {
        let turned = self.session()?.set_stream_volume(self.volume, self.muted);
        self.answered(turned)
    }

    /// Show the subtitles track or not, looking as `style` says.
    fn show(&mut self, shown: bool) -> Result<(), Error> {
        if let Some((_, showing)) = &mut self.subtitles {
            *showing = shown;
        }
        if self.finished {
            return Ok(());
        }
        let active = if shown { vec![TRACK] } else { Vec::new() };
        let style = self.style.clone();
        let edited = self.session()?.set_tracks(&active, style.as_ref());
        self.answered(edited)
    }
}

impl Session for ChromecastSession {
    fn profile(&self) -> Profile {
        self.profile.clone()
    }

    fn load(&mut self, load: &Load) -> Result<(), Error> {
        self.volume = load.volume;
        self.muted = load.muted;
        self.subtitles = load
            .subtitles
            .as_ref()
            .map(|subtitles| (subtitles.source.clone(), true));
        self.style = load
            .subtitles
            .as_ref()
            .map(|subtitles| style(&subtitles.style));
        let media = Media {
            url: load.url.clone(),
            content_type: load.mime.clone(),
            stream_type: if load.live {
                StreamType::Live
            } else {
                StreamType::Buffered
            },
            title: Some(load.title.clone()).filter(|title| !title.is_empty()),
            duration: load.duration,
            text_tracks: load
                .subtitles
                .iter()
                .map(|subtitles| track(subtitles, 0))
                .collect(),
            hls_segments: load.segments.map(|segments| match segments {
                Segments::Ts => HlsSegments::Ts,
                Segments::Fmp4 => HlsSegments::Fmp4,
            }),
        };
        self.give(media, load.autoplay, load.start)
    }

    fn play(&mut self) -> Result<(), Error> {
        self.hear();
        if self.finished
            && let Some(media) = self.media.clone()
        {
            let start = self.start_over.unwrap_or(0.0);
            return self.give(media, true, start);
        }
        let played = self.session()?.play();
        self.answered(played)?;
        self.playing = true;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Error> {
        self.hear();
        self.playing = false;
        if self.finished {
            return Ok(());
        }
        let paused = self.session()?.pause();
        self.answered(paused)
    }

    fn seek(&mut self, time: f64) -> Result<(), Error> {
        self.hear();
        if self.finished {
            self.start_over = Some(time);
            return Ok(());
        }
        let sought = self.session()?.seek(time);
        self.answered(sought)
    }

    fn set_volume(&mut self, volume: f32, muted: bool) -> Result<(), Error> {
        self.hear();
        self.volume = volume;
        self.muted = muted;
        if self.media_session.is_none() || self.finished {
            return Ok(());
        }
        self.turn()
    }

    fn set_subtitles(&mut self, subtitles: Option<&Subtitles>) -> Result<(), Error> {
        self.hear();
        let Some(mut media) = self.media.clone() else {
            return Ok(());
        };
        let Some(subtitles) = subtitles else {
            return match self.subtitles {
                Some((_, true)) => self.show(false),
                _ => Ok(()),
            };
        };
        self.style = Some(style(&subtitles.style));
        if self
            .subtitles
            .as_ref()
            .is_some_and(|(source, _)| *source == subtitles.source)
        {
            return self.show(true);
        }
        // A track can't be given other subtitles, so it's loaded again with
        // them, from where it is.
        self.given += 1;
        media.text_tracks = vec![track(subtitles, self.given)];
        self.subtitles = Some((subtitles.source.clone(), true));
        if self.finished {
            self.media = Some(media);
            return Ok(());
        }
        let asked = self.session()?.status();
        if let Some(status) = asked.map_err(|err| self.failed(err))? {
            self.heard(&status);
        }
        let (playing, position) = (self.playing, self.position);
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
        if self.over().is_none() && !self.finished {
            let asked = self.session()?.status();
            if let Some(status) = asked.map_err(|err| self.failed(err))? {
                self.heard(&status);
            }
        }
        if let Some(over) = self.over() {
            return Err(over);
        }
        let (state, position) = match (self.finished, self.start_over) {
            (true, Some(time)) => (State::Paused, time),
            (true, None) => (State::Ended, self.duration.unwrap_or(self.position)),
            (false, _) => {
                let state = match self.state {
                    PlayerState::Playing => State::Playing,
                    PlayerState::Paused => State::Paused,
                    PlayerState::Buffering => State::Buffering,
                    PlayerState::Idle(_) if self.playing => State::Buffering,
                    PlayerState::Idle(_) => State::Paused,
                };
                (state, self.position)
            }
        };
        Ok(Status {
            state,
            position,
            duration: self.duration,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(profile: &Profile) -> Vec<VideoCodec> {
        profile.video.clone()
    }

    #[test]
    fn the_first_chromecasts_get_h264_up_to_1080p_and_their_files() {
        let first = profile("Chromecast", true);
        assert_eq!(video(&first), [VideoCodec::H264, VideoCodec::Vp8]);
        assert_eq!(first.audio, SOUND);
        assert_eq!(first.files, FILES);
        assert_eq!(first.max_size, Some(FULL_HD));
        assert_eq!(first.max_bit_depth, 8);
        assert!(first.hdr.is_empty());
        assert_eq!(first.hls, Some(Segments::Ts));
        assert!(first.fetches_urls);
        assert!(!first.hls_subtitles, "they go beside it");
        assert!(!first.stream);
    }

    #[test]
    fn an_ultra_and_a_google_tv_get_hevc_vp9_and_hdr() {
        let ultra = profile("Chromecast Ultra", true);
        assert_eq!(
            video(&ultra),
            [
                VideoCodec::H264,
                VideoCodec::Hevc,
                VideoCodec::Vp9,
                VideoCodec::Vp8
            ]
        );
        assert_eq!(ultra.max_size, Some(UHD));
        assert_eq!(ultra.max_bit_depth, 10);
        assert_eq!(ultra.hdr, [Hdr::Hdr10]);
        assert_eq!(ultra.hls, Some(Segments::Fmp4));

        let google_tv = profile("Chromecast with Google TV", true);
        assert_eq!(video(&google_tv), video(&ultra));
        assert_eq!(google_tv.max_size, Some(UHD));
        assert_eq!(google_tv.hdr, [Hdr::Hdr10, Hdr::Hlg]);

        let hd = profile("Chromecast HD", true);
        assert_eq!(video(&hd), video(&ultra));
        assert_eq!(hd.max_size, Some(FULL_HD), "the HD one");
        assert_eq!(hd.hdr, [Hdr::Hdr10, Hdr::Hlg]);
        assert_eq!(hd.max_bit_depth, 10);
    }

    #[test]
    fn a_google_tv_streamer_gets_av1_too() {
        let streamer = profile("Google TV Streamer", true);
        assert!(streamer.video.contains(&VideoCodec::Av1));
        assert!(streamer.video.contains(&VideoCodec::Hevc));
        assert_eq!(streamer.max_size, Some(UHD));
        assert_eq!(streamer.hls, Some(Segments::Fmp4));
    }

    #[test]
    fn any_other_tv_gets_h264_and_aac() {
        let other = profile("BRAVIA 4K VH2", true);
        assert_eq!(video(&other), [VideoCodec::H264]);
        assert_eq!(other.audio, [AudioCodec::Aac]);
        assert_eq!(other.files, [Container::Mp4]);
        assert_eq!(other.max_size, Some(FULL_HD));
        assert_eq!(other.hls, Some(Segments::Ts));
        assert!(!other.hls_subtitles);
    }

    #[test]
    fn a_speaker_gets_sound_as_files_or_hls() {
        for model in ["Google Nest Mini", "Google Cast Group", "Chromecast"] {
            let speaker = profile(model, false);
            assert_eq!(speaker.files, FILES, "{model}");
            assert_eq!(speaker.audio, SOUND, "{model}");
            assert_eq!(speaker.hls, Some(Segments::Ts), "{model}");
            assert_eq!(speaker.max_channels, 2, "{model}");
        }
    }

    #[test]
    fn subtitles_look_as_settings_say() {
        let settings = SubtitleStyle {
            font: "Avenir".into(),
            weight: SubtitleWeight::Bold,
            size: 1.5,
            color: Rgb(0xffcc00),
            edge: SubtitleEdge::Shadow,
            edge_color: Rgb(0x102030),
            background: Rgb(0x000000),
            background_opacity: 0.5,
            ..SubtitleStyle::default()
        };
        assert_eq!(
            style(&settings),
            TextTrackStyle {
                foreground: Color([0xff, 0xcc, 0x00, 0xff]),
                background: Color([0, 0, 0, 0x80]),
                edge: Edge::DropShadow,
                edge_color: Color([0x10, 0x20, 0x30, 0xff]),
                font_scale: 1.5,
                font_family: Some("Avenir".into()),
                generic_family: FontFamily::SansSerif,
                bold: true,
                italic: false,
            }
        );
        let plain = style(&SubtitleStyle {
            font: String::new(),
            weight: SubtitleWeight::Medium,
            edge: SubtitleEdge::Outline,
            background_opacity: 0.0,
            ..SubtitleStyle::default()
        });
        assert_eq!(plain.font_family, None, "the device's own");
        assert!(!plain.bold);
        assert_eq!(plain.edge, Edge::Outline);
        assert_eq!(plain.background.0[3], 0, "no box");
        let none = style(&SubtitleStyle {
            edge: SubtitleEdge::None,
            ..SubtitleStyle::default()
        });
        assert_eq!(none.edge, Edge::None);
    }
}
