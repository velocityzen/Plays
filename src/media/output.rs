//! Where the mixer's sound goes: an output device, through cpal, an AirPlay
//! speaker, or nowhere. With no device to play through (none connected, or
//! the one wanted isn't), the mixer is still run in time, so players keep
//! playing, silently, and pick up where they are once a device is back.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use airplay::{AudioStream, Event as ReceiverEvent};
use anyhow::{Context as _, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, ErrorKind, FromSample, Sample as _, SampleFormat, SizedSample};
use futures::channel::mpsc::UnboundedReceiver;

use super::audio::{Mixer, SOURCE_CHANNELS};

/// The rate a mixer starts at, before there is a device to ask.
const FIRST_RATE: u32 = 48_000;

/// How often the mixer runs while nothing is heard.
const SILENT_PERIOD: Duration = Duration::from_millis(10);
/// Frames an output callback has room for from the start: more than a
/// device asks for at once.
const MOST_CALLBACK_FRAMES: usize = 8192;
/// The rate AirPlay speakers play at.
const SPEAKER_RATE: u32 = 44_100;
/// Frames in each packet sent to an AirPlay speaker, mixed at a time.
const SPEAKER_FRAMES: usize = 352;

/// An output device, as the system names it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeviceInfo {
    /// What the system knows it by, the same from run to run (cpal's
    /// `DeviceId` as text).
    pub id: String,
    pub name: String,
}

/// A mixer for an output, at `master_volume`, made before the output so
/// that players can play into it while the output is made elsewhere.
pub fn mixer(master_volume: f32) -> Arc<Mixer> {
    Mixer::new(FIRST_RATE, master_volume)
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
    /// It is there, but wants to be paired with the PIN it shows first, as
    /// an AirPlay receiver may.
    NeedsPin,
    /// It is there but wouldn't play.
    Failed(anyhow::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("not connected"),
            Self::NeedsPin => f.write_str("it needs to be paired with the PIN it shows"),
            Self::Failed(err) => write!(f, "{err:#}"),
        }
    }
}

/// The mixer and where its sound goes. Not `Send` (a cpal stream isn't
/// everywhere), so it stays on the thread that made it, which waits while
/// it opens a device: the app gives each a thread of its own.
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
    Speaker(Speaker),
    Silent(Silent),
}

impl AudioOutput {
    /// An output of `mixer` (from `mixer`), playing nowhere until told
    /// where.
    pub fn with_mixer(mixer: Arc<Mixer>) -> Self {
        let sink = Sink::Silent(Silent::start(mixer.clone()));
        Self { mixer, sink }
    }

    /// The device it plays through, if any.
    pub fn playing(&self) -> Option<&Playing> {
        match &self.sink {
            Sink::Device { playing, .. } => Some(playing),
            Sink::Speaker(speaker) => Some(&speaker.playing),
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

    /// Play through an AirPlay speaker, connected as `stream`, which says
    /// through `events` if it goes; as device `id`, called `name`. The
    /// mixer changes rate to the speaker's 44.1 kHz, and mixes a packet at a
    /// time as each is due. `on_event` hears it go.
    pub fn play_to_speaker(
        &mut self,
        stream: AudioStream,
        events: UnboundedReceiver<ReceiverEvent>,
        id: String,
        name: String,
        on_event: impl Fn(DeviceEvent) + Send + 'static,
    ) -> Result<&Playing, OpenError> {
        self.play_nowhere();
        let playing = Playing {
            id: Some(id),
            name,
            rate: SPEAKER_RATE,
            channels: SOURCE_CHANNELS as u16,
        };
        self.mixer.set_sample_rate(SPEAKER_RATE);
        // Two can't mix at once: each would take sound from the other.
        if let Sink::Silent(silent) = &mut self.sink {
            silent.stop();
        }
        match Speaker::start(self.mixer.clone(), stream, events, playing, on_event) {
            Ok(speaker) => {
                log::info!(
                    "audio output: AirPlay speaker {} @ {SPEAKER_RATE} Hz",
                    speaker.playing.name
                );
                self.sink = Sink::Speaker(speaker);
                Ok(self.playing().expect("it was just set"))
            }
            Err(err) => {
                self.sink = Sink::Silent(Silent::start(self.mixer.clone()));
                Err(OpenError::Failed(
                    anyhow::Error::new(err).context("starting to send to the speaker"),
                ))
            }
        }
    }

    /// Play nowhere, keeping time, until told where.
    pub fn play_nowhere(&mut self) {
        if !matches!(self.sink, Sink::Silent(_)) {
            // The old one stops before the new one starts, so the two
            // never mix at once.
            self.sink = Sink::Silent(Silent::stopped());
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

/// Mixes into an AirPlay speaker's stream on a thread of its own, a
/// packet at a time as each is due, telling the mixer when what it mixes
/// is heard; stops when dropped, ending the stream.
struct Speaker {
    playing: Playing,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Speaker {
    fn start(
        mixer: Arc<Mixer>,
        mut stream: AudioStream,
        mut events: UnboundedReceiver<ReceiverEvent>,
        playing: Playing,
        on_event: impl Fn(DeviceEvent) + Send + 'static,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let name = playing.name.clone();
        let thread = std::thread::Builder::new()
            .name("airplay speaker".into())
            .spawn({
                let stop = stop.clone();
                move || {
                    if let Err(why) = speak(&mixer, &mut stream, &mut events, &stop) {
                        log::warn!("audio output lost: AirPlay speaker {name}: {why}");
                        on_event(DeviceEvent::Lost);
                    }
                    stream.stop();
                }
            })?;
        Ok(Self {
            playing,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Mix into `stream` until `stop`: each packet as it's due, which `write`
/// waits for, so the mixer keeps the speaker's time. Why it went, if it
/// did.
fn speak(
    mixer: &Mixer,
    stream: &mut AudioStream,
    events: &mut UnboundedReceiver<ReceiverEvent>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let mut mix = vec![0.0f32; SPEAKER_FRAMES * SOURCE_CHANNELS];
    let mut samples = vec![0i16; SPEAKER_FRAMES * SOURCE_CHANNELS];
    while !stop.load(Ordering::Relaxed) {
        if let Ok(ReceiverEvent::Gone) = events.try_recv() {
            return Err("it went away".into());
        }
        let now = Instant::now();
        // Mixed now, it's heard as the next frame written is: the
        // speaker's latency, and what it says it adds, from when it's due.
        let heard = stream
            .heard_at(stream.frames())
            .unwrap_or_else(|| now + stream.latency());
        mixer.mix(
            &mut mix,
            heard.saturating_duration_since(now).as_secs_f64(),
            now,
        );
        for (sample, mixed) in samples.iter_mut().zip(&mix) {
            *sample = i16::from_sample(mixed.clamp(-1.0, 1.0));
        }
        stream.write(&samples).map_err(|err| err.to_string())?;
    }
    Ok(())
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
    /// One that doesn't run: what an output plays through while the last
    /// is stopped and the next not started.
    fn stopped() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(true)),
            thread: None,
        }
    }

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
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc;

    use airplay::Sender;
    use airplay::fake::{FakeReceiver, Kind};

    use super::*;
    use crate::media::audio::{AudioChunk, AudioSource};
    use crate::media::clock::Clock;

    /// A player in `mixer` playing two seconds of `level` from `pts`, at
    /// 48 kHz.
    fn playing_in(mixer: &Mixer, level: f32, pts: f64) -> Arc<AudioSource> {
        let clock = Arc::new(Clock::new());
        clock.set(pts);
        clock.start();
        let source = Arc::new(AudioSource::new(
            clock,
            Arc::new(AtomicU64::new(0)),
            48_000,
            1.0,
        ));
        source.push(AudioChunk {
            pts,
            samples: vec![level; 96_000 * SOURCE_CHANNELS],
            pos: 0,
            rate: 48_000,
        });
        source.playing.store(true, Ordering::Relaxed);
        mixer.add_source(source.clone());
        source
    }

    /// `speaker`, connected to for sound.
    fn connect(speaker: &FakeReceiver) -> (AudioStream, UnboundedReceiver<ReceiverEvent>) {
        AudioStream::connect(&speaker.device(), None, &Sender::new("Plays"))
            .expect("connected to the fake")
    }

    #[test]
    fn a_speaker_plays_the_mix_and_what_it_says_it_adds_holds_the_clock_back() {
        let speaker = FakeReceiver::start(Kind::OldSpeaker);
        // A second of its own, on top of AirPlay's second and a half.
        speaker.state().audio_latency = 44_100;
        let (stream, events) = connect(&speaker);
        let mixer = Mixer::new(48_000, 1.0);
        let source = playing_in(&mixer, 0.25, 10.0);
        let mut output = AudioOutput::with_mixer(mixer.clone());
        let playing = output
            .play_to_speaker(stream, events, "airplay:old".into(), "Old".into(), |_| {})
            .cloned()
            .expect("playing through it");
        assert_eq!(playing.rate, 44_100);
        assert_eq!(mixer.sample_rate(), 44_100, "the speaker's rate");

        assert!(speaker.wait_for(|state| state.packets.len() >= 10));
        let heard = source.clock.now();
        // Sent from 10 s a moment ago, heard 2.5 s after it's sent.
        assert!(
            (heard - 7.5).abs() < 0.3,
            "the clock is at {heard}, not 2.5 s behind what's sent"
        );
        let samples = speaker.state().samples();
        let level = i16::from_sample(0.25f32);
        assert!(
            samples[..4000].iter().all(|&sample| sample == level),
            "the mix, as it is: {:?}",
            &samples[..8]
        );
        drop(output);
        assert!(
            speaker.wait_for(|state| state.closed == state.connections),
            "the stream ends with the output"
        );
        let state = speaker.state();
        assert!(
            state
                .requests
                .iter()
                .any(|request| request.method == "TEARDOWN"),
            "it's told the session ends"
        );
    }

    #[test]
    fn a_speaker_going_away_is_heard_and_one_let_go_is_not() {
        let speaker = FakeReceiver::start(Kind::Speaker);
        let (stream, events) = connect(&speaker);
        let mut output = AudioOutput::with_mixer(Mixer::new(48_000, 1.0));
        let (told, heard) = mpsc::channel();
        output
            .play_to_speaker(
                stream,
                events,
                "airplay:s".into(),
                "S".into(),
                move |event| {
                    let _ = told.send(event);
                },
            )
            .expect("playing through it");
        // On the event connection too, which the session's end comes by.
        assert!(
            speaker.wait_for(|state| { !state.packets.is_empty() && state.event_reply.is_some() })
        );
        speaker.end_session();
        assert_eq!(
            heard.recv_timeout(Duration::from_secs(5)),
            Ok(DeviceEvent::Lost)
        );

        let again = FakeReceiver::start(Kind::Speaker);
        let (stream, events) = connect(&again);
        let (told, heard) = mpsc::channel();
        output
            .play_to_speaker(
                stream,
                events,
                "airplay:s".into(),
                "S".into(),
                move |event| {
                    let _ = told.send(event);
                },
            )
            .expect("playing through it");
        assert!(again.wait_for(|state| !state.packets.is_empty()));
        output.play_nowhere();
        assert!(output.playing().is_none());
        assert!(
            heard.recv_timeout(Duration::from_millis(500)).is_err(),
            "let go, it isn't lost"
        );
    }

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
        let mixer = mixer(0.0);
        let mut output = AudioOutput::with_mixer(mixer.clone());
        let playing = output
            .play_through(None, |_| {})
            .cloned()
            .expect("a default output");
        eprintln!("default: {playing:?}");
        assert_eq!(mixer.sample_rate(), playing.rate);
        for device in &devices {
            assert!(is_connected(Some(&device.id)), "{device:?} by its id");
            let latency = find(Some(&device.id)).map(|device| stream_latency(&device));
            eprintln!("{}: {latency:?} frames of stream latency", device.name);
            match output.play_through(Some(&device.id), |_| {}).cloned() {
                Ok(playing) => {
                    assert_eq!(playing.id.as_deref(), Some(device.id.as_str()));
                    assert_eq!(mixer.sample_rate(), playing.rate);
                }
                Err(err) => eprintln!("{}: {err}", device.name),
            }
        }
        let gone = output.play_through(Some("coreaudio:no-such-device"), |_| {});
        assert!(matches!(gone, Err(OpenError::Missing)), "{gone:?}");
        assert!(output.playing().is_none());
    }
}
