//! The audio outputs players play through, and which devices those are.
//! The app's plays through the one picked in Settings → Audio, or the
//! system's default. A window may play through a device of its own
//! instead, whose output every window picking it shares. While a device
//! picked isn't connected, its sound waits for it (players keep playing,
//! silently) or goes through the system's default, as settings say; either
//! way it goes back to the device as soon as it's there. Each output
//! opens its devices on a thread of its own, as that can take a while.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
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
    /// Nowhere yet: the device named is being opened.
    Opening(String),
}

/// What reaches the UI thread from audio threads, with the routing it is
/// about (`Output::generation`), as one routed since is past it.
enum Event {
    Device(u64, DeviceEvent),
    /// The device wanted is there again, or it is time to try it again.
    Back(u64),
    /// An output was connected or disconnected.
    Devices,
    /// Where an output's device thread routed it.
    Routed(u64, Routed),
    /// What an output plays through now, having followed the system's
    /// default to another device.
    FollowedTo(u64, Playing),
}

/// What an output's device thread is asked to do.
enum Request {
    /// Play through `wanted`, or as near as there is, opening devices with
    /// `open`, as routing `generation`.
    Route {
        wanted: AudioOutputSettings,
        generation: u64,
        open: Open,
    },
    /// The system's default output changed, and the stream with it, while
    /// routed as `generation`.
    FollowDefault(u64),
}

/// Where a device thread routed its output.
struct Routed {
    route: Route,
    /// The device to look for while it can't be played through.
    watch: Option<WatchFor>,
}

/// A device that couldn't be played through: `id`, or with `None` the
/// system's default, and whether it wasn't there at all.
struct WatchFor {
    id: Option<String>,
    missing: bool,
}

/// An output, and where its sound goes.
struct Output {
    /// What its players play into.
    mixer: Arc<Mixer>,
    /// Asks the thread that holds its `AudioOutput`, and opens its devices,
    /// to route it.
    device: Sender<Request>,
    route: Route,
    /// What it was asked for when it was routed.
    wanted: AudioOutputSettings,
    /// Its routing, numbered among all of the engine's, which events about
    /// it name.
    generation: u64,
    /// Looks for the device wanted while it can't be played through.
    watch: Option<Watch>,
}

/// Holds an output's `AudioOutput`, which can't leave the thread that
/// made it, and opens its devices as asked: that can take longer than the
/// UI thread can wait. Says through `events` where it routed it, and ends,
/// the output with it, once nothing can ask it anything.
fn device_thread(mixer: Arc<Mixer>, requests: Receiver<Request>, events: UnboundedSender<Event>) {
    let mut output = AudioOutput::with_mixer(mixer);
    while let Ok(first) = requests.recv() {
        let mut asked: Vec<Request> = std::iter::once(first).chain(requests.try_iter()).collect();
        // A routing asked for since makes what was asked before it moot.
        if let Some(last) = asked
            .iter()
            .rposition(|request| matches!(request, Request::Route { .. }))
        {
            asked.drain(..last);
        }
        for request in asked {
            let event = match request {
                Request::Route {
                    wanted,
                    generation,
                    open,
                } => Event::Routed(
                    generation,
                    route(&mut output, &wanted, generation, open, &events),
                ),
                Request::FollowDefault(generation) => {
                    output.followed_default();
                    let Some(playing) = output.playing() else {
                        continue;
                    };
                    Event::FollowedTo(generation, playing.clone())
                }
            };
            if events.unbounded_send(event).is_err() {
                return;
            }
        }
    }
}

/// Play `output` through `wanted`, or as near as there is, opening devices
/// with `open`, as routing `generation`, which events about it name.
fn route(
    output: &mut AudioOutput,
    wanted: &AudioOutputSettings,
    generation: u64,
    open: Open,
    events: &UnboundedSender<Event>,
) -> Routed {
    let on_event = || -> OnEvent {
        let events = events.clone();
        Box::new(move |event| {
            let _ = events.unbounded_send(Event::Device(generation, event));
        })
    };
    let picked = wanted.device.as_ref();
    match (
        open(output, picked.map(|d| d.id.as_str()), on_event()),
        picked,
    ) {
        (Ok(playing), _) => Routed {
            route: Route::Device {
                playing,
                missing: None,
            },
            watch: None,
        },
        (Err(err), Some(device)) => {
            log::info!("audio output {}: {err}", device.name);
            let missing = matches!(err, OpenError::Missing);
            // Trying one that is there but wouldn't play again would break
            // the sound standing in for it each time: only while nothing
            // is heard.
            let watch = (missing || wanted.wait_for_device).then(|| WatchFor {
                id: Some(device.id.clone()),
                missing,
            });
            let route = if wanted.wait_for_device {
                Route::Waiting(device.name.clone())
            } else {
                match open(output, None, on_event()) {
                    Ok(playing) => Route::Device {
                        playing,
                        missing: Some(device.name.clone()),
                    },
                    Err(err) => Route::Unavailable(unavailable(&err)),
                }
            };
            Routed { route, watch }
        }
        (Err(err), None) => {
            log::warn!("no audio output: {err}");
            Routed {
                route: Route::Unavailable(unavailable(&err)),
                watch: Some(WatchFor {
                    id: None,
                    missing: matches!(err, OpenError::Missing),
                }),
            }
        }
    }
}

/// What happens to the device an output plays through, as it happens.
type OnEvent = Box<dyn Fn(DeviceEvent) + Send>;

/// Opens device `id` (`None`: the system's default) for an output:
/// `play_through`, or in tests one that answers as they say.
type Open = fn(&mut AudioOutput, Option<&str>, OnEvent) -> Result<Playing, OpenError>;

fn play_through(
    output: &mut AudioOutput,
    id: Option<&str>,
    on_event: OnEvent,
) -> Result<Playing, OpenError> {
    output.play_through(id, on_event).cloned()
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
    /// How its outputs open devices: `None` in tests where they play
    /// nowhere, and are never routed.
    open: Option<Open>,
    /// Whether it lists the output devices: not in tests.
    lists_devices: bool,
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
            main: Output::new(
                settings,
                settings.audio_output.clone(),
                "Silent in tests",
                &events,
            ),
            windows: Vec::new(),
            generation: 0,
            devices: Vec::new(),
            events,
            open: None,
            lists_devices: false,
        });
    }

    /// Start the audio, play through the output settings ask for, and follow
    /// settings and devices from then on.
    pub(super) fn start(cx: &mut App) {
        Self::start_with(play_through, true, cx);
    }

    /// `start`, opening devices with `open`, and listing them if
    /// `lists_devices`.
    fn start_with(open: Open, lists_devices: bool, cx: &mut App) {
        let settings = cx.global::<Settings>();
        let (events, mut received) = unbounded();
        let engine = Self {
            main: Output::new(
                settings,
                settings.audio_output.clone(),
                "Not started",
                &events,
            ),
            windows: Vec::new(),
            generation: 0,
            devices: Vec::new(),
            events,
            open: Some(open),
            lists_devices,
        };
        cx.set_global(engine);
        Self::reroute(cx);
        if lists_devices {
            Self::list_devices(cx);
            let events = cx.global::<Self>().events.clone();
            output::watch_devices(Box::new(move || {
                events.unbounded_send(Event::Devices).ok();
            }));
        }

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
            Event::Routed(generation, routed) => {
                let engine = cx.global_mut::<Self>();
                let events = engine.events.clone();
                if let Some(output) = engine.output_mut(generation) {
                    output.routed(routed, &events);
                    cx.refresh_windows();
                }
                return;
            }
            Event::FollowedTo(generation, now) => {
                if let Some(output) = cx.global_mut::<Self>().output_mut(generation)
                    && let Route::Device { playing, .. } = &mut output.route
                {
                    *playing = now;
                    cx.refresh_windows();
                }
                return;
            }
            Event::Device(generation, DeviceEvent::Followed) => {
                let Some(output) = cx.global_mut::<Self>().output_mut(generation) else {
                    return;
                };
                output.follow_default();
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
        let Some(open) = engine.open else {
            return;
        };
        engine.generation += 1;
        engine.main.route_to(wanted, engine.generation, open);
        cx.refresh_windows();
    }

    /// Play the output of window device `index` through it, or as near as
    /// there is, waiting for it while it isn't connected if `wait`.
    fn route_window(&mut self, index: usize, wait: bool) {
        let Some(open) = self.open else {
            return;
        };
        self.generation += 1;
        let window = &mut self.windows[index];
        let wanted = AudioOutputSettings {
            device: Some(window.device.clone()),
            wait_for_device: wait,
        };
        window.output.route_to(wanted, self.generation, open);
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
                let why = if engine.open.is_none() {
                    "Silent in tests"
                } else {
                    "Not started"
                };
                engine.windows.push(WindowOutput {
                    device: device.clone(),
                    output: Output::new(&settings, wanted, why, &engine.events),
                    windows: 0,
                });
                let index = engine.windows.len() - 1;
                engine.route_window(index, settings.audio_output.wait_for_device);
                index
            }
        };
        let window = &mut engine.windows[index];
        window.windows += 1;
        let mixer = window.output.mixer.clone();
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
                output.mixer.set_master_volume(volume);
            }
        }
    }

    /// List the output devices again, in the background, for Settings,
    /// the Audio menu and the Play On menu.
    pub fn list_devices(cx: &mut App) {
        if cx
            .try_global::<Self>()
            .is_none_or(|engine| !engine.lists_devices)
        {
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
        Some(cx.try_global::<Self>()?.main.mixer.clone())
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
            Some(Route::Opening(name)) => format!("Opening {name}…"),
            None => "Unavailable".into(),
        }
    }
}

impl Output {
    /// One playing nowhere until routed, at the master volume `settings`
    /// say, for `wanted`, with a device thread of its own, which says
    /// through `events` where it routed it.
    fn new(
        settings: &Settings,
        wanted: AudioOutputSettings,
        why: &str,
        events: &UnboundedSender<Event>,
    ) -> Self {
        let mixer = output::mixer(settings.master_volume);
        let (device, requests) = mpsc::channel();
        // Without it, routing it says so.
        std::thread::Builder::new()
            .name("audio device".into())
            .spawn({
                let mixer = mixer.clone();
                let events = events.clone();
                move || device_thread(mixer, requests, events)
            })
            .inspect_err(|err| log::error!("can't start an audio output: {err}"))
            .ok();
        Self {
            mixer,
            device,
            route: Route::Unavailable(why.into()),
            wanted,
            generation: 0,
            watch: None,
        }
    }

    /// Play through `wanted`, or as near as there is, opening devices with
    /// `open`, as routing `generation`, which events about it name. Its
    /// device thread opens them, and until it says where it routed it
    /// (`routed`), it plays nowhere.
    fn route_to(&mut self, wanted: AudioOutputSettings, generation: u64, open: Open) {
        self.generation = generation;
        self.watch = None;
        let name = wanted
            .device
            .as_ref()
            .map_or_else(|| "the system default".into(), |device| device.name.clone());
        let asked = self.device.send(Request::Route {
            wanted: wanted.clone(),
            generation,
            open,
        });
        self.route = match asked {
            Ok(()) => Route::Opening(name),
            Err(_) => Route::Unavailable("the audio output didn't start".into()),
        };
        self.wanted = wanted;
    }

    /// Its device thread routed it, as `routed`: it looks for the device
    /// wanted, telling `events` once it is back, if it must.
    fn routed(&mut self, routed: Routed, events: &UnboundedSender<Event>) {
        self.route = routed.route;
        self.watch = routed
            .watch
            .map(|watch| Watch::start(watch, self.generation, events.clone()));
    }

    /// The system's default output changed, and the stream with it: its
    /// device thread says what it plays through now.
    fn follow_default(&self) {
        let _ = self.device.send(Request::FollowDefault(self.generation));
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
    fn start(watch: WatchFor, generation: u64, events: UnboundedSender<Event>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        // One that is there but wouldn't play is only tried again later.
        let WatchFor { id, missing } = watch;
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

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, Mutex};
    use std::thread::ThreadId;

    use gpui::TestAppContext;

    use super::*;
    use crate::ui::testing::wait_until;

    /// Whether the devices `open_slowly` opens are ready: each waits for
    /// it, or for `SLOW` at most.
    static READY: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
    const SLOW: Duration = Duration::from_secs(5);
    /// The threads devices were opened on.
    static OPENED_ON: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

    /// Opens any device, once it is ready, as some take a while to.
    fn open_slowly(
        _: &mut AudioOutput,
        id: Option<&str>,
        _: OnEvent,
    ) -> Result<Playing, OpenError> {
        OPENED_ON.lock().unwrap().push(std::thread::current().id());
        let (ready, readied) = &READY;
        let ready = ready.lock().unwrap();
        drop(readied.wait_timeout_while(ready, SLOW, |ready| !*ready));
        Ok(Playing {
            id: id.map(str::to_owned),
            name: id.unwrap_or("Speakers").into(),
            rate: 44_100,
            channels: 2,
        })
    }

    fn set_ready(now: bool) {
        let (ready, readied) = &READY;
        *ready.lock().unwrap() = now;
        readied.notify_all();
    }

    fn playing(cx: &TestAppContext) -> Option<String> {
        cx.read(|cx| match AudioEngine::route(cx) {
            Some(Route::Device { playing, .. }) => Some(playing.name.clone()),
            _ => None,
        })
    }

    #[gpui::test]
    fn opens_devices_without_the_app_waiting_for_them(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(Settings::default());
            AudioEngine::start_with(open_slowly, false, cx);
        });
        assert_eq!(playing(cx), None, "the app went on while it opened");
        set_ready(true);
        assert!(wait_until(cx, |cx| playing(cx).is_some()));
        assert_eq!(playing(cx).as_deref(), Some("Speakers"));

        set_ready(false);
        cx.update(|cx| {
            Settings::update(cx, false, |settings| {
                settings.audio_output.device = Some(AudioDevice {
                    id: "desk".into(),
                    name: "Desk".into(),
                });
            });
        });
        assert_eq!(playing(cx), None, "the app went on while it opened");
        let shown = cx.read(AudioEngine::device_description);
        assert_eq!(shown, "Opening Desk…");
        set_ready(true);
        assert!(wait_until(cx, |cx| playing(cx).is_some()));
        assert_eq!(playing(cx).as_deref(), Some("desk"));

        let ui = std::thread::current().id();
        let opened_on = OPENED_ON.lock().unwrap();
        assert_eq!(opened_on.len(), 2);
        assert!(
            opened_on.iter().all(|&thread| thread != ui),
            "opened on the UI thread"
        );
    }
}
