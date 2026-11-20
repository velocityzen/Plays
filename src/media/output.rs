//! Where the mixer's sound goes: an output device, through cpal, or nowhere.
//! With no device to play through (none connected, or the one wanted
//! isn't), the mixer is still run in time, so players keep playing,
//! silently, and pick up where they are once a device is back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample};

use super::audio::{Mixer, SOURCE_CHANNELS};

/// The rate a mixer starts at, before there is a device to ask.
const FIRST_RATE: u32 = 48_000;

/// How often the mixer runs while nothing is heard.
const SILENT_PERIOD: Duration = Duration::from_millis(10);
/// Frames an output callback has room for from the start: more than a
/// device asks for at once.
const MOST_CALLBACK_FRAMES: usize = 8192;

/// An output device, as the system names it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeviceInfo {
    /// What the system knows it by, the same from run to run (cpal's
    /// `DeviceId` as text).
    pub id: String,
    pub name: String,
}

/// Every output device connected now that can play. Slow on some systems
/// (ALSA opens each to see what it plays), so not for the UI thread.
pub fn devices() -> Vec<DeviceInfo> {
    match cpal::default_host().output_devices() {
        Ok(devices) => devices
            .filter(can_play)
            .filter_map(|device| {
                let id = device.id().ok()?.to_string();
                Some(DeviceInfo {
                    id,
                    name: name(&device),
                })
            })
            .collect(),
        Err(err) => {
            log::warn!("can't list the audio outputs: {err}");
            Vec::new()
        }
    }
}

/// Whether `device` has a format to play in, as ALSA lists plugins that
/// have nothing behind them. One busy (playing for another, or for Plays
/// itself) still can.
fn can_play(device: &cpal::Device) -> bool {
    match device.default_output_config() {
        Ok(_) => true,
        Err(err) => err.kind() == ErrorKind::DeviceBusy,
    }
}

/// Whether device `id` is connected, or with `None` whether there is a
/// default output.
pub fn is_connected(id: Option<&str>) -> bool {
    find(id).is_some()
}

/// Device `id`, or the system's default output.
fn find(id: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    match id {
        None => host.default_output_device(),
        Some(id) => host.device_by_id(&id.parse::<DeviceId>().ok()?),
    }
}

fn name(device: &cpal::Device) -> String {
    device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|_| device.to_string())
}

/// Frames of latency in `device`'s output streams: how long after the
/// device takes a sample it is heard, beyond its own latency and safety
/// offset, which cpal counts already when it says when a buffer plays.
/// Bluetooth and AirPlay outputs report most of their delay here, AirPlay
/// about two seconds of it. Only on macOS; 0 elsewhere.
fn stream_latency(device: &cpal::Device) -> u32 {
    #[cfg(target_os = "macos")]
    {
        device
            .id()
            .ok()
            .map_or(0, |id| core_audio::stream_latency(id.id()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = device;
        0
    }
}

/// Call `changed` whenever an output device is connected or disconnected,
/// from a thread of the system's, for as long as the app runs. Only on
/// macOS, where nothing else tells; elsewhere the list is read again when
/// it is shown.
pub fn watch_devices(changed: Box<dyn Fn() + Send + Sync>) {
    #[cfg(target_os = "macos")]
    core_audio::watch_devices(changed);
    #[cfg(not(target_os = "macos"))]
    drop(changed);
}

#[cfg(target_os = "macos")]
mod core_audio {
    use std::ffi::c_void;

    /// `kAudioObjectSystemObject`.
    const SYSTEM_OBJECT: u32 = 1;
    /// `kAudioHardwarePropertyDevices`, 'dev#'.
    const DEVICES: u32 = u32::from_be_bytes(*b"dev#");
    /// `kAudioObjectPropertyScopeGlobal`, 'glob'.
    const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    /// `kAudioObjectPropertyElementMain`.
    const ELEMENT_MAIN: u32 = 0;
    /// `kAudioObjectPropertyScopeOutput`, 'outp'.
    const SCOPE_OUTPUT: u32 = u32::from_be_bytes(*b"outp");
    /// `kAudioHardwarePropertyTranslateUIDToDevice`, 'uidd'.
    const UID_TO_DEVICE: u32 = u32::from_be_bytes(*b"uidd");
    /// `kAudioDevicePropertyStreams`, 'stm#'.
    const STREAMS: u32 = u32::from_be_bytes(*b"stm#");
    /// `kAudioStreamPropertyLatency`, 'ltnc'.
    const LATENCY: u32 = u32::from_be_bytes(*b"ltnc");

    #[repr(C)]
    struct PropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    type Listener = extern "C" fn(u32, u32, *const PropertyAddress, *mut c_void) -> i32;

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectAddPropertyListener(
            object: u32,
            address: *const PropertyAddress,
            listener: Listener,
            client_data: *mut c_void,
        ) -> i32;
        fn AudioObjectGetPropertyDataSize(
            object: u32,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    /// The most latency, in frames, of the output streams of the device
    /// whose UID is `uid`; 0 if it can't be read.
    pub fn stream_latency(uid: &str) -> u32 {
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        let uid = CFString::new(uid);
        let uid_ref = uid.as_concrete_TypeRef();
        let mut device = 0u32;
        let mut size = size_of::<u32>() as u32;
        let address = PropertyAddress {
            selector: UID_TO_DEVICE,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        };
        // SAFETY: the qualifier is a CFStringRef, alive while `uid` is, and
        // the answer an AudioObjectID, which `device` holds.
        let status = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &address,
                size_of_val(&uid_ref) as u32,
                (&raw const uid_ref).cast(),
                &mut size,
                (&raw mut device).cast(),
            )
        };
        if status != 0 || device == 0 {
            return 0;
        }

        let address = PropertyAddress {
            selector: STREAMS,
            scope: SCOPE_OUTPUT,
            element: ELEMENT_MAIN,
        };
        let mut size = 0u32;
        // SAFETY: only the size is written.
        let status = unsafe {
            AudioObjectGetPropertyDataSize(device, &address, 0, std::ptr::null(), &mut size)
        };
        if status != 0 {
            return 0;
        }
        let mut streams = vec![0u32; size as usize / size_of::<u32>()];
        // SAFETY: `streams` has room for `size` bytes of AudioStreamIDs.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                streams.as_mut_ptr().cast(),
            )
        };
        if status != 0 {
            return 0;
        }
        streams.truncate(size as usize / size_of::<u32>());

        let address = PropertyAddress {
            selector: LATENCY,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        };
        streams
            .into_iter()
            .filter_map(|stream| {
                let mut latency = 0u32;
                let mut size = size_of::<u32>() as u32;
                // SAFETY: the answer is a UInt32, which `latency` holds.
                let status = unsafe {
                    AudioObjectGetPropertyData(
                        stream,
                        &address,
                        0,
                        std::ptr::null(),
                        &mut size,
                        (&raw mut latency).cast(),
                    )
                };
                (status == 0).then_some(latency)
            })
            .max()
            .unwrap_or(0)
    }

    type Changed = Box<dyn Fn() + Send + Sync>;

    extern "C" fn devices_changed(
        _: u32,
        _: u32,
        _: *const PropertyAddress,
        client_data: *mut c_void,
    ) -> i32 {
        // SAFETY: the `Changed` leaked in `watch_devices`, never freed.
        let changed = unsafe { &*client_data.cast::<Changed>() };
        changed();
        0
    }

    pub fn watch_devices(changed: Changed) {
        let address = PropertyAddress {
            selector: DEVICES,
            scope: SCOPE_GLOBAL,
            element: ELEMENT_MAIN,
        };
        // Listened to for as long as the app runs, so never freed.
        let client_data = Box::into_raw(Box::new(changed)).cast::<c_void>();
        // SAFETY: `address` is read during the call; `client_data` lives
        // forever and is only read, by `devices_changed`.
        let status = unsafe {
            AudioObjectAddPropertyListener(SYSTEM_OBJECT, &address, devices_changed, client_data)
        };
        if status != 0 {
            log::warn!("can't follow audio outputs coming and going: OSStatus {status}");
        }
    }
}

/// What an output plays through.
#[derive(Clone, PartialEq, Debug)]
pub struct Playing {
    /// The device asked for; `None` for the system's default, which the
    /// stream follows as it changes where the system can.
    pub id: Option<String>,
    pub name: String,
    pub rate: u32,
    pub channels: u16,
}

/// Something that happened to the device an output plays through, for the
/// app to act on. Reported from an audio thread.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceEvent {
    /// It was disconnected or its stream broke: play through another, or
    /// open it again.
    Lost,
    /// The system's default output changed, and the stream went with it.
    Followed,
}

/// Why an output can't play through a device.
#[derive(Debug)]
pub enum OpenError {
    /// It isn't connected (or, for the default, there is no output at all).
    Missing,
    /// It is there but wouldn't play.
    Failed(anyhow::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("not connected"),
            Self::Failed(err) => write!(f, "{err:#}"),
        }
    }
}

/// The mixer and where its sound goes. Not `Send` (a cpal stream isn't
/// everywhere), so it lives on the UI thread.
pub struct AudioOutput {
    mixer: Arc<Mixer>,
    sink: Sink,
}

enum Sink {
    Device {
        /// Plays while it is kept.
        _stream: cpal::Stream,
        playing: Playing,
        /// The device's stream latency, in frames, read when the stream
        /// works out when what it mixes is heard.
        stream_latency: Arc<AtomicU32>,
    },
    Silent(Silent),
}

impl AudioOutput {
    /// An output that plays nowhere until told where, at `master_volume`.
    pub fn new(master_volume: f32) -> Self {
        let mixer = Mixer::new(FIRST_RATE, master_volume);
        let sink = Sink::Silent(Silent::start(mixer.clone()));
        Self { mixer, sink }
    }

    pub fn mixer(&self) -> &Arc<Mixer> {
        &self.mixer
    }

    /// The device it plays through, if any.
    pub fn playing(&self) -> Option<&Playing> {
        match &self.sink {
            Sink::Device { playing, .. } => Some(playing),
            Sink::Silent(_) => None,
        }
    }

    /// Play through device `id`, or the system's default with `None`, in its
    /// own format: the mixer changes rate to it if it must. `on_event` hears
    /// what happens to the device from then on. If it can't, it plays
    /// nowhere.
    pub fn play_through(
        &mut self,
        id: Option<&str>,
        on_event: impl Fn(DeviceEvent) + Send + 'static,
    ) -> Result<&Playing, OpenError> {
        // Let go of the old device first: some can only be opened once.
        self.play_nowhere();
        let device = find(id).ok_or(OpenError::Missing)?;
        let stream_latency = Arc::new(AtomicU32::new(stream_latency(&device)));
        let (stream, playing) = open(&device, id, &self.mixer, stream_latency.clone(), on_event)
            .map_err(OpenError::Failed)?;
        log::info!(
            "audio output: {} @ {} Hz, {} channels, {} frames of stream latency",
            playing.name,
            playing.rate,
            playing.channels,
            stream_latency.load(Ordering::Relaxed)
        );
        // Two can't mix at once: each would take sound from the other.
        if let Sink::Silent(silent) = &mut self.sink {
            silent.stop();
        }
        if let Err(err) = stream.play() {
            self.sink = Sink::Silent(Silent::start(self.mixer.clone()));
            return Err(OpenError::Failed(
                anyhow::Error::new(err).context("starting the audio stream"),
            ));
        }
        self.sink = Sink::Device {
            _stream: stream,
            playing,
            stream_latency,
        };
        Ok(self.playing().expect("it was just set"))
    }

    /// Play nowhere, keeping time, until told where.
    pub fn play_nowhere(&mut self) {
        if matches!(self.sink, Sink::Device { .. }) {
            self.sink = Sink::Silent(Silent::start(self.mixer.clone()));
        }
    }

    /// The system's default output changed and the stream followed it:
    /// what it plays through is called something else now, and is heard
    /// that much later.
    pub fn followed_default(&mut self) {
        if let Sink::Device {
            playing,
            stream_latency: latency,
            ..
        } = &mut self.sink
            && playing.id.is_none()
            && let Some(device) = find(None)
        {
            playing.name = name(&device);
            latency.store(stream_latency(&device), Ordering::Relaxed);
            log::info!(
                "audio output followed the default to {}, {} frames of stream latency",
                playing.name,
                latency.load(Ordering::Relaxed)
            );
        }
    }
}

/// A stream from `mixer` to `device` (asked for as `id`), not started yet,
/// heard `stream_latency` frames after cpal says it plays.
fn open(
    device: &cpal::Device,
    id: Option<&str>,
    mixer: &Arc<Mixer>,
    stream_latency: Arc<AtomicU32>,
    on_event: impl Fn(DeviceEvent) + Send + 'static,
) -> Result<(cpal::Stream, Playing)> {
    let supported = device
        .default_output_config()
        .context("asking the output for its format")?;
    let sample_format = supported.sample_format();
    let config = supported.config();
    let playing = Playing {
        id: id.map(str::to_owned),
        name: name(device),
        rate: config.sample_rate,
        channels: config.channels,
    };
    mixer.set_sample_rate(config.sample_rate);
    let on_error = move |err: cpal::Error| match err.kind() {
        ErrorKind::DeviceChanged => on_event(DeviceEvent::Followed),
        ErrorKind::DeviceNotAvailable
        | ErrorKind::StreamInvalidated
        | ErrorKind::HostUnavailable => {
            log::warn!("audio output lost: {err}");
            on_event(DeviceEvent::Lost);
        }
        _ => log::error!("audio stream error: {err}"),
    };
    let mixer = mixer.clone();
    let stream = match sample_format {
        SampleFormat::F32 => build::<f32>(device, config, mixer, stream_latency, on_error),
        SampleFormat::F64 => build::<f64>(device, config, mixer, stream_latency, on_error),
        SampleFormat::I16 => build::<i16>(device, config, mixer, stream_latency, on_error),
        SampleFormat::I32 => build::<i32>(device, config, mixer, stream_latency, on_error),
        SampleFormat::U16 => build::<u16>(device, config, mixer, stream_latency, on_error),
        SampleFormat::U8 => build::<u8>(device, config, mixer, stream_latency, on_error),
        SampleFormat::I8 => build::<i8>(device, config, mixer, stream_latency, on_error),
        other => Err(anyhow!("unsupported output sample format {other:?}")),
    }?;
    Ok((stream, playing))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mixer: Arc<Mixer>,
    stream_latency: Arc<AtomicU32>,
    on_error: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = f64::from(config.sample_rate);
    // Made ready for more than a device asks for at once, so the callback
    // doesn't allocate.
    let mut mix = Vec::<f32>::with_capacity(MOST_CALLBACK_FRAMES * SOURCE_CHANNELS);

    let stream = device.build_output_stream::<T, _, _>(
        config,
        move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
            let frames = out.len() / channels;
            mix.resize(frames * SOURCE_CHANNELS, 0.0);

            let timestamp = info.timestamp();
            let latency = timestamp
                .playback
                .checked_duration_since(timestamp.callback)
                .map(|d| d.as_secs_f64())
                .unwrap_or(frames as f64 / sample_rate)
                + f64::from(stream_latency.load(Ordering::Relaxed)) / sample_rate;
            mixer.mix(&mut mix, latency, Instant::now());

            for (frame, lr) in out
                .chunks_exact_mut(channels)
                .zip(mix.as_chunks::<SOURCE_CHANNELS>().0)
            {
                let l = lr[0].clamp(-1.0, 1.0);
                let r = lr[1].clamp(-1.0, 1.0);
                if channels == 1 {
                    frame[0] = T::from_sample((l + r) * 0.5);
                } else {
                    frame[0] = T::from_sample(l);
                    frame[1] = T::from_sample(r);
                    for extra in &mut frame[2..] {
                        *extra = T::from_sample(0.0f32);
                    }
                }
            }
        },
        on_error,
        None,
    )?;
    Ok(stream)
}

/// Runs a mixer in time with nothing to hear it, so its players keep
/// playing; stops when dropped.
struct Silent {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Silent {
    fn start(mixer: Arc<Mixer>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new()
            .name("silent audio".into())
            .spawn({
                let stop = stop.clone();
                move || {
                    let mut mix = Vec::new();
                    let mut since = Instant::now();
                    // Frames due, as the rate doesn't divide the period.
                    let mut due = 0.0;
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(SILENT_PERIOD);
                        let now = Instant::now();
                        due += (now - since).as_secs_f64() * f64::from(mixer.sample_rate());
                        let frames = due as usize;
                        due -= frames as f64;
                        mix.resize(frames * SOURCE_CHANNELS, 0.0);
                        // What it mixes went by since `since`.
                        mixer.mix(&mut mix, 0.0, since);
                        since = now;
                    }
                }
            })
            .inspect_err(|err| log::error!("can't keep time without audio: {err}"))
            .ok();
        Self { stop, thread }
    }
}

impl Silent {
    /// Stop mixing, once it has.
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Silent {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn no_stream_latency_for_a_device_not_there() {
        assert_eq!(core_audio::stream_latency("no-such-device"), 0);
    }

    /// Needs the machine's own audio outputs: `cargo test -- --ignored`.
    #[test]
    #[ignore = "plays through this machine's audio outputs"]
    fn plays_through_the_devices_here() {
        let devices = devices();
        eprintln!("outputs: {devices:#?}");
        let mut output = AudioOutput::new(0.0);
        let playing = output
            .play_through(None, |_| {})
            .cloned()
            .expect("a default output");
        eprintln!("default: {playing:?}");
        assert_eq!(output.mixer().sample_rate(), playing.rate);
        for device in &devices {
            assert!(is_connected(Some(&device.id)), "{device:?} by its id");
            let latency = find(Some(&device.id)).map(|device| stream_latency(&device));
            eprintln!("{}: {latency:?} frames of stream latency", device.name);
            match output.play_through(Some(&device.id), |_| {}).cloned() {
                Ok(playing) => {
                    assert_eq!(playing.id.as_deref(), Some(device.id.as_str()));
                    assert_eq!(output.mixer().sample_rate(), playing.rate);
                }
                Err(err) => eprintln!("{}: {err}", device.name),
            }
        }
        let gone = output.play_through(Some("coreaudio:no-such-device"), |_| {});
        assert!(matches!(gone, Err(OpenError::Missing)), "{gone:?}");
        assert!(output.playing().is_none());
    }
}
