//! The audio output every player plays through, and which device that is:
//! the one picked in Settings → Audio, or the system's default. While the
//! device picked isn't connected, the sound waits for it (players keep
//! playing, silently) or goes through the system's default, as settings
//! say; either way it goes back to the device as soon as it's there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AppContext as _, Global};

use crate::media::output::{self, DeviceEvent, DeviceInfo, OpenError, Playing};
use crate::media::{AudioOutput, Mixer};
use crate::settings::{AudioOutputSettings, Settings};

/// How often a device that isn't connected is looked for.
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// How long until a device that is there but wouldn't play is tried again.
const RETRY_AFTER: Duration = Duration::from_secs(10);

/// Where the sound goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    /// Through this device. `missing` names the device picked, when this is
    /// the system's default standing in for it.
    Device {
        playing: Playing,
        missing: Option<String>,
    },
    /// Nowhere: the device picked, named, isn't connected, and settings say
    /// to wait for it.
    Waiting(String),
    /// Nowhere, for this reason.
    Unavailable(String),
}

/// What reaches the UI thread from audio threads, with the routing it is
/// about (`AudioEngine::generation`), as one routed since is past it.
enum Event {
    Device(u64, DeviceEvent),
    /// The device wanted is there again, or it is time to try it again.
    Back(u64),
}

/// The single audio output shared by every player.
pub struct AudioEngine {
    output: AudioOutput,
    route: Route,
    /// What settings asked for when it was routed.
    wanted: AudioOutputSettings,
    /// Counts routings, to tell what an event is about.
    generation: u64,
    /// The output devices, as last listed.
    devices: Vec<DeviceInfo>,
    events: UnboundedSender<Event>,
    /// Looks for the device wanted while it can't be played through.
    watch: Option<Watch>,
}

impl Global for AudioEngine {}

impl AudioEngine {
    /// Start the audio, play through the output settings ask for, and follow
    /// settings and devices from then on.
    pub(super) fn start(cx: &mut App) {
        let settings = cx.global::<Settings>();
        let (events, mut received) = unbounded();
        let engine = Self {
            output: AudioOutput::new(settings.master_volume),
            route: Route::Unavailable("Not started".into()),
            wanted: settings.audio_output.clone(),
            generation: 0,
            devices: Vec::new(),
            events,
            watch: None,
        };
        cx.set_global(engine);
        Self::reroute(cx);
        Self::list_devices(cx);

        cx.spawn(async move |cx| {
            while let Some(event) = received.next().await {
                if cx.update(|cx| Self::handle(event, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.observe_global::<Settings>(|cx| {
            let wanted = cx.global::<Settings>().audio_output.clone();
            let engine = cx.global_mut::<Self>();
            if wanted == engine.wanted {
                return;
            }
            // Only whether to wait changed, and the device wanted is
            // playing: nothing to move.
            if wanted.device == engine.wanted.device
                && matches!(engine.route, Route::Device { missing: None, .. })
            {
                engine.wanted = wanted;
                return;
            }
            Self::reroute(cx);
        })
        .detach();
    }

    fn handle(event: Event, cx: &mut App) {
        let current = cx.global::<Self>().generation;
        match event {
            Event::Device(generation, DeviceEvent::Followed) if generation == current => {
                let engine = cx.global_mut::<Self>();
                engine.output.followed_default();
                if let (Route::Device { playing, .. }, Some(now)) =
                    (&mut engine.route, engine.output.playing())
                {
                    *playing = now.clone();
                }
                cx.refresh_windows();
            }
            Event::Device(generation, DeviceEvent::Lost) | Event::Back(generation)
                if generation == current =>
            {
                Self::reroute(cx)
            }
            // About an output since replaced.
            _ => return,
        }
        Self::list_devices(cx);
    }

    /// Play through what settings ask for, or as near as there is.
    fn reroute(cx: &mut App) {
        let wanted = cx.global::<Settings>().audio_output.clone();
        cx.global_mut::<Self>().route_to(wanted);
        cx.refresh_windows();
    }

    fn route_to(&mut self, wanted: AudioOutputSettings) {
        self.generation += 1;
        self.watch = None;
        let generation = self.generation;
        let events = self.events.clone();
        let on_event = move |event| {
            let _ = events.unbounded_send(Event::Device(generation, event));
        };
        let picked = wanted.device.as_ref();
        let result = self
            .output
            .play_through(picked.map(|d| d.id.as_str()), on_event.clone())
            .cloned();
        self.route = match (result, picked) {
            (Ok(playing), _) => Route::Device {
                playing,
                missing: None,
            },
            (Err(err), Some(device)) => {
                log::info!("audio output {}: {err}", device.name);
                // Trying one that is there but wouldn't play again would
                // break the sound standing in for it each time: only while
                // nothing is heard.
                if matches!(err, OpenError::Missing) || wanted.wait_for_device {
                    self.watch = Some(Watch::start(
                        Some(device.id.clone()),
                        &err,
                        generation,
                        self.events.clone(),
                    ));
                }
                if wanted.wait_for_device {
                    Route::Waiting(device.name.clone())
                } else {
                    match self.output.play_through(None, on_event) {
                        Ok(playing) => Route::Device {
                            playing: playing.clone(),
                            missing: Some(device.name.clone()),
                        },
                        Err(err) => Route::Unavailable(unavailable(&err)),
                    }
                }
            }
            (Err(err), None) => {
                log::warn!("no audio output: {err}");
                self.watch = Some(Watch::start(None, &err, generation, self.events.clone()));
                Route::Unavailable(unavailable(&err))
            }
        };
        self.wanted = wanted;
    }

    /// List the output devices again, in the background, for Settings.
    pub fn list_devices(cx: &mut App) {
        let task = cx.background_spawn(async { output::devices() });
        cx.spawn(async move |cx| {
            let devices = task.await;
            cx.update(|cx| {
                if cx.global::<Self>().devices != devices {
                    cx.global_mut::<Self>().devices = devices;
                    cx.refresh_windows();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn mixer(cx: &App) -> Option<Arc<Mixer>> {
        Some(cx.try_global::<Self>()?.output.mixer().clone())
    }

    /// The output devices, as last listed.
    pub fn devices(cx: &App) -> &[DeviceInfo] {
        cx.try_global::<Self>()
            .map_or(&[], |engine| &engine.devices)
    }

    /// Where the sound goes.
    pub fn route(cx: &App) -> Option<&Route> {
        cx.try_global::<Self>().map(|engine| &engine.route)
    }

    /// Where the sound goes, in a few words.
    pub fn device_description(cx: &App) -> String {
        match Self::route(cx) {
            Some(Route::Device { playing, .. }) => {
                format!("{} · {} kHz", playing.name, playing.rate as f32 / 1000.0)
            }
            Some(Route::Waiting(name)) => format!("Nothing, until {name} is connected"),
            Some(Route::Unavailable(why)) => format!("Unavailable: {why}"),
            None => "Unavailable".into(),
        }
    }
}

fn unavailable(err: &OpenError) -> String {
    match err {
        OpenError::Missing => "no audio output".into(),
        OpenError::Failed(err) => format!("{err:#}"),
    }
}

/// Looks, on a thread of its own, for the device that couldn't be played
/// through (`None`: the system's default) to be back, and says so once.
/// Stops when dropped.
struct Watch {
    stop: Arc<AtomicBool>,
}

impl Watch {
    fn start(
        id: Option<String>,
        err: &OpenError,
        generation: u64,
        events: UnboundedSender<Event>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        // One that is there but wouldn't play is only tried again later.
        let missing = matches!(err, OpenError::Missing);
        let stopped = stop.clone();
        std::thread::Builder::new()
            .name("audio device watch".into())
            .spawn(move || {
                let mut waited = Duration::ZERO;
                loop {
                    std::thread::sleep(LOOK_EVERY);
                    if stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    waited += LOOK_EVERY;
                    let back = if missing {
                        output::is_connected(id.as_deref())
                    } else {
                        waited >= RETRY_AFTER
                    };
                    if back {
                        let _ = events.unbounded_send(Event::Back(generation));
                        return;
                    }
                }
            })
            .inspect_err(|err| log::warn!("can't watch for the audio device: {err}"))
            .ok();
        Self { stop }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
