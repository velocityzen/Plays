//! A tile's link to the device it casts to. Every call to a device may wait
//! on the network, so the session runs on a thread of its own: the tile
//! sends it what to do and reads what the device last said, showing what
//! it asked for until the device has done it.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use stream_server::{Cast, CastRequest, Pick, Server, Source, SubtitleSource};

use super::target::{Device, Error, Load, Session, State, Status, Subtitles, Target};
use crate::media::text_encoding;
use crate::settings::SubtitleStyle;

/// How often the device is asked what it's doing.
const STATUS_POLL: Duration = Duration::from_millis(500);

/// What to cast.
#[derive(Clone, Debug)]
pub struct Request {
    pub source: Source,
    pub title: String,
    pub video: Pick,
    pub audio: Pick,
    pub subtitles: Option<SubtitleChoice>,
    /// Where to start, in seconds.
    pub start: f64,
    pub autoplay: bool,
    pub volume: f32,
    pub muted: bool,
    pub looping: bool,
}

/// Subtitles to send along.
#[derive(Clone, PartialEq, Debug)]
pub struct SubtitleChoice {
    /// A file's encoding is found on the link's thread, as that reads it.
    pub source: SubtitleSource,
    pub language: Option<String>,
    pub name: String,
    pub style: SubtitleStyle,
}

/// How a cast is going.
#[derive(Clone, PartialEq, Debug)]
pub enum Phase {
    Connecting,
    /// Playing there, the media served as `serves` says: "as is",
    /// "repackaged", "re-encoded to H.264".
    On {
        serves: String,
    },
    /// Over: the device went away, or wouldn't play it.
    Over(Error),
}

enum Command {
    Play,
    Pause,
    Seek(f64),
    Volume {
        volume: f32,
        muted: bool,
    },
    Looping(bool),
    Subtitles(Option<SubtitleChoice>),
    /// Load the media again with other tracks, from `start`.
    Reload {
        video: Pick,
        audio: Pick,
        start: f64,
        autoplay: bool,
    },
}

/// What the link's thread tells the tile.
struct Shared {
    phase: Phase,
    /// What the device last said, and when.
    status: Option<(Status, Instant)>,
    /// How many commands had been done then.
    done: u64,
    /// How long the media is, as the stream server read it.
    duration: Option<f64>,
    /// What last went wrong, for the tile to say.
    failure: Option<String>,
}

/// What the tile last asked for: where the device should be, and whether
/// it should play.
#[derive(Clone, Copy, Debug)]
struct Asked {
    position: f64,
    playing: bool,
}

pub struct Link {
    device: Device,
    commands: Sender<Command>,
    shared: Arc<Mutex<Shared>>,
    /// Commands sent so far.
    sent: u64,
    asked: Asked,
    thread: Option<JoinHandle<()>>,
}

impl Link {
    /// Connect to `device`, have `server` serve the media for it and load
    /// it there, all on the link's thread. Dropping the link stops it.
    pub fn start(
        target: Arc<dyn Target>,
        device: Device,
        server: Arc<Server>,
        request: Request,
    ) -> Self {
        let (commands, receiver) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared {
            phase: Phase::Connecting,
            status: None,
            done: 0,
            duration: None,
            failure: None,
        }));
        let asked = Asked {
            position: request.start,
            playing: request.autoplay,
        };
        let spawned = {
            let device = device.clone();
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name(format!("cast to {}", device.name))
                .spawn(move || run(&*target, &device, server, request, receiver, shared))
        };
        let thread = spawned
            .inspect_err(|err| {
                shared.lock().phase = Phase::Over(Error::Failed(err.to_string()));
            })
            .ok();
        Self {
            device,
            commands,
            shared,
            sent: 0,
            asked,
            thread,
        }
    }

    /// Stop the device, and hand back the thread stopping it, to wait for.
    pub fn stop(mut self) -> Option<JoinHandle<()>> {
        self.thread.take()
    }

    pub fn device(&self) -> &Device {
        &self.device
    }

    pub fn phase(&self) -> Phase {
        self.shared.lock().phase.clone()
    }

    /// What last went wrong, once.
    pub fn take_failure(&self) -> Option<String> {
        self.shared.lock().failure.take()
    }

    /// Where the device is, and whether it plays: what it last said, moved
    /// on by the time since; or what was last asked of it, until it has
    /// done that.
    fn shown(&self) -> (f64, bool) {
        let shared = self.shared.lock();
        match shared.status {
            Some((status, at)) if shared.done >= self.sent => {
                let mut position = status.position;
                if status.state == State::Playing {
                    position += at.elapsed().as_secs_f64();
                }
                if let Some(duration) = status.duration.or(shared.duration) {
                    position = position.min(duration);
                }
                let playing = matches!(status.state, State::Playing | State::Buffering);
                (position.max(0.0), playing)
            }
            _ => (self.asked.position, self.asked.playing),
        }
    }

    pub fn position(&self) -> f64 {
        self.shown().0
    }

    pub fn is_playing(&self) -> bool {
        self.shown().1
    }

    /// Played to the end.
    pub fn has_ended(&self) -> bool {
        let shared = self.shared.lock();
        shared.done >= self.sent
            && shared
                .status
                .is_some_and(|(status, _)| status.state == State::Ended)
    }

    pub fn duration(&self) -> Option<f64> {
        let shared = self.shared.lock();
        shared
            .status
            .and_then(|(status, _)| status.duration)
            .or(shared.duration)
    }

    fn send(&mut self, command: Command, asked: Asked) {
        self.asked = asked;
        self.sent += 1;
        let _ = self.commands.send(command);
    }

    pub fn play(&mut self) {
        let position = if self.has_ended() {
            0.0
        } else {
            self.position()
        };
        self.send(
            Command::Play,
            Asked {
                position,
                playing: true,
            },
        );
    }

    pub fn pause(&mut self) {
        let position = self.position();
        self.send(
            Command::Pause,
            Asked {
                position,
                playing: false,
            },
        );
    }

    pub fn seek(&mut self, time: f64) {
        let time = match self.duration() {
            Some(duration) => time.clamp(0.0, duration),
            None => time.max(0.0),
        };
        let playing = self.is_playing();
        self.send(
            Command::Seek(time),
            Asked {
                position: time,
                playing,
            },
        );
    }

    pub fn set_volume(&mut self, volume: f32, muted: bool) {
        let (position, playing) = self.shown();
        self.send(
            Command::Volume { volume, muted },
            Asked { position, playing },
        );
    }

    pub fn set_looping(&mut self, looping: bool) {
        let (position, playing) = self.shown();
        self.send(Command::Looping(looping), Asked { position, playing });
    }

    pub fn set_subtitles(&mut self, subtitles: Option<SubtitleChoice>) {
        let (position, playing) = self.shown();
        self.send(Command::Subtitles(subtitles), Asked { position, playing });
    }

    /// Play other video or sound: the media loaded again from where it is.
    pub fn reload(&mut self, video: Pick, audio: Pick) {
        let (position, playing) = self.shown();
        self.send(
            Command::Reload {
                video,
                audio,
                start: position,
                autoplay: playing,
            },
            Asked { position, playing },
        );
    }
}

fn run(
    target: &dyn Target,
    device: &Device,
    server: Arc<Server>,
    request: Request,
    commands: Receiver<Command>,
    shared: Arc<Mutex<Shared>>,
) {
    match Worker::start(target, device, server, request, Arc::clone(&shared)) {
        Ok(worker) => worker.serve(commands),
        Err(err) => {
            log::warn!("can't cast to {}: {err}", device.name);
            shared.lock().phase = Phase::Over(err);
        }
    }
}

/// The session, on the link's thread, and what it plays.
struct Worker {
    session: Box<dyn Session>,
    server: Arc<Server>,
    address: IpAddr,
    request: CastRequest,
    cast: Cast,
    subtitles: Option<SubtitleChoice>,
    load: Load,
    looping: bool,
    /// What the device last said it was doing.
    state: State,
    shared: Arc<Mutex<Shared>>,
}

impl Worker {
    fn start(
        target: &dyn Target,
        device: &Device,
        server: Arc<Server>,
        request: Request,
        shared: Arc<Mutex<Shared>>,
    ) -> Result<Self, Error> {
        let session = target.connect(device)?;
        let subtitles = request.subtitles.map(with_encoding);
        let cast_request = CastRequest {
            source: request.source,
            profile: session.profile(),
            video: request.video,
            audio: request.audio,
            subtitles: subtitles.as_ref().map(|choice| choice.source.clone()),
        };
        let cast = server
            .add(cast_request.clone(), device.address)
            .map_err(|err| Error::Failed(err.to_string()))?;
        let load = Load {
            title: request.title,
            start: request.start,
            autoplay: request.autoplay,
            volume: request.volume,
            muted: request.muted,
            ..load_for(&cast)
        };
        let mut worker = Self {
            session,
            server,
            address: device.address,
            request: cast_request,
            cast,
            subtitles,
            load,
            looping: request.looping,
            state: State::Buffering,
            shared,
        };
        worker.load.subtitles = worker.served_subtitles();
        if let Err(err) = worker.session.load(&worker.load) {
            worker.server.remove(&worker.cast);
            let _ = worker.session.stop();
            return Err(err);
        }
        {
            let mut shared = worker.shared.lock();
            shared.phase = Phase::On {
                serves: worker.cast.plan().describe(),
            };
            shared.duration = worker.cast.duration();
        }
        log::info!(
            "casting to {} ({}): {}",
            device.name,
            device.kind,
            worker.cast.plan().describe()
        );
        Ok(worker)
    }

    /// Do what the tile asks, and ask the device what it's doing, until
    /// the tile lets go or the device goes away.
    fn serve(mut self, commands: Receiver<Command>) {
        let mut done = 0;
        loop {
            let mut batch = Vec::new();
            match commands.recv_timeout(STATUS_POLL) {
                Ok(command) => batch.push(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return self.stop(),
            }
            batch.extend(commands.try_iter());
            done += batch.len() as u64;
            for command in coalesce(batch) {
                match self.apply(command) {
                    Ok(()) => {}
                    Err(Error::Failed(why)) => {
                        log::warn!("casting: {why}");
                        self.shared.lock().failure = Some(why);
                    }
                    Err(over) => return self.over(over),
                }
            }
            let status = match self.session.status() {
                Ok(status) => status,
                Err(Error::Failed(why)) => {
                    log::debug!("casting: no status: {why}");
                    continue;
                }
                Err(over) => return self.over(over),
            };
            self.state = status.state;
            if status.state == State::Ended && self.looping && !self.load.live {
                match self.restart() {
                    Err(Error::Failed(why)) => log::debug!("casting: looping: {why}"),
                    Err(over) => return self.over(over),
                    Ok(()) => {}
                }
                continue;
            }
            let mut shared = self.shared.lock();
            shared.status = Some((status, Instant::now()));
            shared.done = done;
        }
    }

    fn apply(&mut self, command: Command) -> Result<(), Error> {
        match command {
            Command::Play if self.state == State::Ended => self.restart(),
            Command::Play => self.session.play(),
            Command::Pause => self.session.pause(),
            Command::Seek(time) => self.session.seek(time),
            Command::Volume { volume, muted } => {
                self.load.volume = volume;
                self.load.muted = muted;
                self.session.set_volume(volume, muted)
            }
            Command::Looping(looping) => {
                self.looping = looping;
                Ok(())
            }
            Command::Subtitles(subtitles) => {
                self.subtitles = subtitles.map(with_encoding);
                self.request.subtitles = self.subtitles.as_ref().map(|s| s.source.clone());
                self.server
                    .set_subtitles(&self.cast, self.request.subtitles.clone());
                self.load.subtitles = self.served_subtitles();
                self.session.set_subtitles(self.load.subtitles.as_ref())
            }
            Command::Reload {
                video,
                audio,
                start,
                autoplay,
            } => {
                let mut request = self.request.clone();
                request.video = video;
                request.audio = audio;
                let cast = self
                    .server
                    .add(request.clone(), self.address)
                    .map_err(|err| Error::Failed(err.to_string()))?;
                let old = std::mem::replace(&mut self.cast, cast);
                self.request = request;
                self.load = Load {
                    title: std::mem::take(&mut self.load.title),
                    start,
                    autoplay,
                    volume: self.load.volume,
                    muted: self.load.muted,
                    ..load_for(&self.cast)
                };
                self.load.subtitles = self.served_subtitles();
                let loaded = self.session.load(&self.load);
                // The device leaves the old URL once it has the new one.
                self.server.remove(&old);
                self.shared.lock().phase = Phase::On {
                    serves: self.cast.plan().describe(),
                };
                loaded
            }
        }
    }

    /// Play again from the start.
    fn restart(&mut self) -> Result<(), Error> {
        self.session.seek(0.0)?;
        self.session.play()
    }

    /// The subtitles picked, at the stream server's URLs for them.
    fn served_subtitles(&self) -> Option<Subtitles> {
        let choice = self.subtitles.as_ref()?;
        Some(Subtitles {
            webvtt_url: self.cast.webvtt_url()?,
            srt_url: self.cast.srt_url()?,
            language: choice.language.clone(),
            name: choice.name.clone(),
            style: choice.style.clone(),
        })
    }

    fn stop(mut self) {
        if let Err(err) = self.session.stop() {
            log::debug!("casting: stopping: {err}");
        }
        self.server.remove(&self.cast);
    }

    /// The device went away, or plays something else now.
    fn over(self, why: Error) {
        log::info!("casting: over: {why}");
        self.server.remove(&self.cast);
        self.shared.lock().phase = Phase::Over(why);
    }
}

/// What a device is told of a cast, before the tile's part.
fn load_for(cast: &Cast) -> Load {
    Load {
        url: cast.url(),
        mime: cast.mime().to_owned(),
        protocol_info: cast.protocol_info(),
        title: String::new(),
        duration: cast.duration(),
        live: cast.duration().is_none(),
        start: 0.0,
        autoplay: true,
        volume: 1.0,
        muted: false,
        subtitles: None,
    }
}

/// A subtitle file's encoding, read from it.
fn with_encoding(mut choice: SubtitleChoice) -> SubtitleChoice {
    if let SubtitleSource::File {
        path,
        encoding: encoding @ None,
    } = &mut choice.source
    {
        *encoding = text_encoding(path).map(str::to_owned);
    }
    choice
}

/// Commands as they're to be done: of seeks in a row, as a slider sends
/// them, only the last.
fn coalesce(commands: Vec<Command>) -> Vec<Command> {
    let mut kept: Vec<Command> = Vec::with_capacity(commands.len());
    for command in commands {
        if matches!(
            (&command, kept.last()),
            (Command::Seek(_), Some(Command::Seek(_)))
        ) {
            kept.pop();
        }
        kept.push(command);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_last_of_seeks_in_a_row() {
        let kept = coalesce(vec![
            Command::Seek(1.0),
            Command::Seek(2.0),
            Command::Pause,
            Command::Seek(3.0),
            Command::Seek(4.0),
        ]);
        let seeks: Vec<Option<f64>> = kept
            .iter()
            .map(|command| match command {
                Command::Seek(time) => Some(*time),
                _ => None,
            })
            .collect();
        assert_eq!(seeks, [Some(2.0), None, Some(4.0)]);
    }
}
