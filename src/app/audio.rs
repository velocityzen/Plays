//! The audio outputs players play through, and which devices those are.
//! The app's plays through the one picked in Settings → Audio, or the
//! system's default. A window may play through a device of its own
//! instead, whose output every window picking it shares. While a device
//! picked isn't connected, its sound waits for it (players keep playing,
//! silently) or goes through the system's default, as settings say; either
//! way it goes back to the device as soon as it's there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{App, AppContext as _, Global};

use super::menus;

use crate::media::output::{self, DeviceEvent, DeviceInfo, OpenError, Playing};
use crate::media::{AudioOutput, Mixer};
use crate::settings::{AudioDevice, AudioOutputSettings, Settings};

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
/// about (`Output::generation`), as one routed since is past it.
enum Event {
    Device(u64, DeviceEvent),
    /// The device wanted is there again, or it is time to try it again.
    Back(u64),
    /// An output was connected or disconnected.
    Devices,
}

/// An output, and where its sound goes.
struct Output {
    output: AudioOutput,
    route: Route,
    /// What it was asked for when it was routed.
    wanted: AudioOutputSettings,
    /// Its routing, numbered among all of the engine's, which events about
    /// it name.
    generation: u64,
    /// Looks for the device wanted while it can't be played through.
    watch: Option<Watch>,
}

/// The output of a device windows play through instead of the app's.
struct WindowOutput {
    device: AudioDevice,
    output: Output,
    /// How many windows play through it.
    windows: usize,
}

/// The audio outputs, shared by every player.
pub struct AudioEngine {
    /// The app's: what every window without a device of its own plays
    /// through.
    main: Output,
    windows: Vec<WindowOutput>,
    /// Counts routings, to tell what an event is about.
    generation: u64,
    /// The output devices, as last listed.
    devices: Vec<DeviceInfo>,
    events: UnboundedSender<Event>,
    /// For tests: outputs play nowhere, and are never routed.
    silent: bool,
}

impl Global for AudioEngine {}

impl AudioEngine {
    /// For tests: the mixers run in time, but play nowhere, never list or
    /// open a device, and don't follow settings.
    #[cfg(test)]
    pub(super) fn start_silent(cx: &mut App) {
        let settings = cx.global::<Settings>();
        let (events, _) = unbounded();
        cx.set_global(Self {
            main: Output::new(settings, settings.audio_output.clone(), "Silent in tests"),
            windows: Vec::new(),
            generation: 0,
            devices: Vec::new(),
            events,
            silent: true,
        });
    }

    /// Start the audio, play through the output settings ask for, and follow
    /// settings and devices from then on.
    pub(super) fn start(cx: &mut App) {
        let settings = cx.global::<Settings>();
        let (events, mut received) = unbounded();
        let engine = Self {
            main: Output::new(settings, settings.audio_output.clone(), "Not started"),
            windows: Vec::new(),
            generation: 0,
            devices: Vec::new(),
            events,
            silent: false,
        };
        cx.set_global(engine);
        Self::reroute(cx);
        Self::list_devices(cx);
        let events = cx.global::<Self>().events.clone();
        output::watch_devices(Box::new(move || {
            events.unbounded_send(Event::Devices).ok();
        }));

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
            if wanted == engine.main.wanted {
                return;
            }
            // The Audio menu checks the device picked.
            if wanted.device != engine.main.wanted.device {
                menus::set_menus(cx);
            }
            let engine = cx.global_mut::<Self>();
            if wanted.wait_for_device != engine.main.wanted.wait_for_device {
                engine.windows_wait_for_devices(wanted.wait_for_device);
            }
            // Only whether to wait changed, and the device wanted is
            // playing: nothing to move.
            if wanted.device == engine.main.wanted.device
                && matches!(engine.main.route, Route::Device { missing: None, .. })
            {
                engine.main.wanted = wanted;
                cx.refresh_windows();
                return;
            }
            Self::reroute(cx);
        })
        .detach();
    }

    fn handle(event: Event, cx: &mut App) {
        match event {
            Event::Device(generation, DeviceEvent::Followed) => {
                let Some(output) = cx.global_mut::<Self>().output_mut(generation) else {
                    return;
                };
                output.followed_default();
                cx.refresh_windows();
            }
            Event::Device(generation, DeviceEvent::Lost) | Event::Back(generation) => {
                let engine = cx.global_mut::<Self>();
                if engine.main.generation == generation {
                    Self::reroute(cx);
                } else if let Some(index) = engine
                    .windows
                    .iter()
                    .position(|window| window.output.generation == generation)
                {
                    let wait = engine.main.wanted.wait_for_device;
                    engine.route_window(index, wait);
                    cx.refresh_windows();
                } else {
                    // About an output since routed again, or closed.
                    return;
                }
            }
            Event::Devices => {}
        }
        Self::list_devices(cx);
    }

    /// The output routed as `generation`, as it is still.
    fn output_mut(&mut self, generation: u64) -> Option<&mut Output> {
        std::iter::once(&mut self.main)
            .chain(self.windows.iter_mut().map(|window| &mut window.output))
            .find(|output| output.generation == generation)
    }

    /// Play the app's output through what settings ask for, or as near as
    /// there is.
    fn reroute(cx: &mut App) {
        let wanted = cx.global::<Settings>().audio_output.clone();
        let engine = cx.global_mut::<Self>();
        engine.generation += 1;
        engine
            .main
            .route_to(wanted, engine.generation, &engine.events);
        cx.refresh_windows();
    }

    /// Play the output of window device `index` through it, or as near as
    /// there is, waiting for it while it isn't connected if `wait`.
    fn route_window(&mut self, index: usize, wait: bool) {
        self.generation += 1;
        let window = &mut self.windows[index];
        let wanted = AudioOutputSettings {
            device: Some(window.device.clone()),
            wait_for_device: wait,
        };
        window
            .output
            .route_to(wanted, self.generation, &self.events);
    }

    /// Settings say whether to wait for a device now: windows' outputs not
    /// playing through theirs go by it.
    fn windows_wait_for_devices(&mut self, wait: bool) {
        for index in 0..self.windows.len() {
            let output = &mut self.windows[index].output;
            if matches!(output.route, Route::Device { missing: None, .. }) {
                output.wanted.wait_for_device = wait;
            } else {
                self.route_window(index, wait);
            }
        }
    }

    /// The mixer of `device`'s output, for a window to play through instead
    /// of the app's: the one windows play through already, or a new one.
    /// The window lets go of it (`let_go`) once it doesn't.
    pub fn window_mixer(device: &AudioDevice, cx: &mut App) -> Option<Arc<Mixer>> {
        if !cx.has_global::<Self>() {
            return None;
        }
        let settings = cx.global::<Settings>().clone();
        let engine = cx.global_mut::<Self>();
        let index = match engine
            .windows
            .iter()
            .position(|window| window.device.id == device.id)
        {
            Some(index) => index,
            None => {
                let wanted = AudioOutputSettings {
                    device: Some(device.clone()),
                    ..settings.audio_output.clone()
                };
                let why = if engine.silent {
                    "Silent in tests"
                } else {
                    "Not started"
                };
                engine.windows.push(WindowOutput {
                    device: device.clone(),
                    output: Output::new(&settings, wanted, why),
                    windows: 0,
                });
                let index = engine.windows.len() - 1;
                if !engine.silent {
                    engine.route_window(index, settings.audio_output.wait_for_device);
                }
                index
            }
        };
        let window = &mut engine.windows[index];
        window.windows += 1;
        let mixer = window.output.output.mixer().clone();
        cx.refresh_windows();
        Some(mixer)
    }

    /// A window doesn't play through `device`'s output any more: it closes
    /// once none does.
    pub fn let_go(device: &AudioDevice, cx: &mut App) {
        if !cx.has_global::<Self>() {
            return;
        }
        let engine = cx.global_mut::<Self>();
        if let Some(index) = engine
            .windows
            .iter()
            .position(|window| window.device.id == device.id)
        {
            let window = &mut engine.windows[index];
            window.windows = window.windows.saturating_sub(1);
            if window.windows == 0 {
                engine.windows.remove(index);
            }
        }
    }

    /// Every output's volume, over each player's.
    pub fn set_master_volume(volume: f32, cx: &App) {
        if let Some(engine) = cx.try_global::<Self>() {
            for output in std::iter::once(&engine.main)
                .chain(engine.windows.iter().map(|window| &window.output))
            {
                output.output.mixer().set_master_volume(volume);
            }
        }
    }

    /// List the output devices again, in the background, for Settings,
    /// the Audio menu and the Play On menu.
    pub fn list_devices(cx: &mut App) {
        if cx.try_global::<Self>().is_none_or(|engine| engine.silent) {
            return;
        }
        let task = cx.background_spawn(async { output::devices() });
        cx.spawn(async move |cx| {
            let devices = task.await;
            cx.update(|cx| {
                if cx.global::<Self>().devices != devices {
                    cx.global_mut::<Self>().devices = devices;
                    cx.refresh_windows();
                    menus::set_menus(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The app's output's mixer.
    pub fn mixer(cx: &App) -> Option<Arc<Mixer>> {
        Some(cx.try_global::<Self>()?.main.output.mixer().clone())
    }

    /// The output devices, as last listed.
    pub fn devices(cx: &App) -> &[DeviceInfo] {
        cx.try_global::<Self>()
            .map_or(&[], |engine| &engine.devices)
    }

    /// For tests: list `devices` as the outputs connected.
    #[cfg(test)]
    pub fn set_devices(devices: Vec<DeviceInfo>, cx: &mut App) {
        cx.global_mut::<Self>().devices = devices;
    }

    /// For tests: how many outputs windows play through of their own.
    #[cfg(test)]
    pub fn window_outputs(cx: &App) -> usize {
        cx.global::<Self>().windows.len()
    }

    /// Where the app's sound goes.
    pub fn route(cx: &App) -> Option<&Route> {
        cx.try_global::<Self>().map(|engine| &engine.main.route)
    }

    /// Where the app's sound goes, in a few words.
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

impl Output {
    /// One playing nowhere until routed, at the master volume `settings`
    /// say, for `wanted`.
    fn new(settings: &Settings, wanted: AudioOutputSettings, why: &str) -> Self {
        Self {
            output: AudioOutput::new(settings.master_volume),
            route: Route::Unavailable(why.into()),
            wanted,
            generation: 0,
            watch: None,
        }
    }

    /// Play through `wanted`, or as near as there is, as routing
    /// `generation`, which events about it name.
    fn route_to(
        &mut self,
        wanted: AudioOutputSettings,
        generation: u64,
        events: &UnboundedSender<Event>,
    ) {
        self.generation = generation;
        self.watch = None;
        let on_event = {
            let events = events.clone();
            move |event| {
                let _ = events.unbounded_send(Event::Device(generation, event));
            }
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
                        events.clone(),
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
                self.watch = Some(Watch::start(None, &err, generation, events.clone()));
                Route::Unavailable(unavailable(&err))
            }
        };
        self.wanted = wanted;
    }

    /// The system's default output changed, and the stream with it.
    fn followed_default(&mut self) {
        self.output.followed_default();
        if let (Route::Device { playing, .. }, Some(now)) = (&mut self.route, self.output.playing())
        {
            *playing = now.clone();
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
