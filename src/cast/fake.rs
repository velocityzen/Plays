//! A pretend protocol for tests: devices the test adds and takes away,
//! which do as they are told, keep what they were told, and say what the
//! test has them say.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use parking_lot::Mutex;
use stream_server::Profile;

use super::target::{Device, Error, Load, Session, Shows, State, Status, Subtitles, Target};

pub const KIND: &str = "Fake";

/// What a fake device was told.
#[derive(Clone, PartialEq, Debug)]
pub enum Call {
    Load(Load),
    Play,
    Pause,
    Seek(f64),
    Volume(f32, bool),
    Subtitles(Option<Subtitles>),
    Stop,
}

/// A fake device's side.
#[derive(Debug)]
pub struct Screen {
    pub calls: Vec<Call>,
    /// What it says it is doing; it plays from where it's told, and stays
    /// there until the test moves it.
    pub status: Status,
    /// Gone: its session says so.
    pub gone: bool,
    /// What it plays, as the stream server is to serve it.
    pub profile: Profile,
}

/// Each device, and its side.
type Found = Vec<(Device, Arc<Mutex<Screen>>)>;

#[derive(Clone, Default)]
pub struct FakeTarget {
    devices: Arc<Mutex<Found>>,
}

impl FakeTarget {
    /// A device that shows `shows`, on this computer.
    pub fn add(&self, name: &str, shows: Shows) -> Device {
        let mut devices = self.devices.lock();
        let device = Device {
            kind: KIND,
            id: format!("fake-{}", devices.len()),
            name: name.to_owned(),
            shows,
            address: IpAddr::V4(Ipv4Addr::LOCALHOST),
        };
        let screen = Screen {
            calls: Vec::new(),
            status: Status {
                state: State::Paused,
                position: 0.0,
                duration: None,
            },
            gone: false,
            profile: Profile::default(),
        };
        devices.push((device.clone(), Arc::new(Mutex::new(screen))));
        device
    }

    /// Take a device off the network: it is no longer found, and its
    /// session says it's gone.
    pub fn remove(&self, device: &Device) {
        let mut devices = self.devices.lock();
        devices.retain(|(found, screen)| {
            let keep = !found.is(device);
            if !keep {
                screen.lock().gone = true;
            }
            keep
        });
    }

    /// The device's side, to see what it was told and have it say things.
    pub fn screen(&self, device: &Device) -> Arc<Mutex<Screen>> {
        self.devices
            .lock()
            .iter()
            .find(|(found, _)| found.is(device))
            .map(|(_, screen)| Arc::clone(screen))
            .expect("a device the fake has")
    }
}

impl Target for FakeTarget {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn devices(&self) -> Vec<Device> {
        self.devices
            .lock()
            .iter()
            .map(|(device, _)| device.clone())
            .collect()
    }

    fn connect(&self, device: &Device) -> Result<Box<dyn Session>, Error> {
        let screen = self
            .devices
            .lock()
            .iter()
            .find(|(found, _)| found.is(device))
            .map(|(_, screen)| Arc::clone(screen))
            .ok_or(Error::Gone)?;
        Ok(Box::new(FakeSession { screen }))
    }
}

struct FakeSession {
    screen: Arc<Mutex<Screen>>,
}

impl FakeSession {
    /// Note the call, and change what the device says as `change` does.
    fn told(&mut self, call: Call, change: impl FnOnce(&mut Status)) -> Result<(), Error> {
        let mut screen = self.screen.lock();
        if screen.gone {
            return Err(Error::Gone);
        }
        screen.calls.push(call);
        change(&mut screen.status);
        Ok(())
    }
}

impl Session for FakeSession {
    fn profile(&self) -> Profile {
        self.screen.lock().profile.clone()
    }

    fn load(&mut self, load: &Load) -> Result<(), Error> {
        self.told(Call::Load(load.clone()), |status| {
            *status = Status {
                state: if load.autoplay {
                    State::Playing
                } else {
                    State::Paused
                },
                position: load.start,
                duration: load.duration,
            }
        })
    }

    fn play(&mut self) -> Result<(), Error> {
        self.told(Call::Play, |status| status.state = State::Playing)
    }

    fn pause(&mut self) -> Result<(), Error> {
        self.told(Call::Pause, |status| status.state = State::Paused)
    }

    fn seek(&mut self, time: f64) -> Result<(), Error> {
        self.told(Call::Seek(time), |status| status.position = time)
    }

    fn set_volume(&mut self, volume: f32, muted: bool) -> Result<(), Error> {
        self.told(Call::Volume(volume, muted), |_| {})
    }

    fn set_subtitles(&mut self, subtitles: Option<&Subtitles>) -> Result<(), Error> {
        self.told(Call::Subtitles(subtitles.cloned()), |_| {})
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.told(Call::Stop, |status| status.state = State::Paused)
    }

    fn status(&mut self) -> Result<Status, Error> {
        let screen = self.screen.lock();
        if screen.gone {
            Err(Error::Gone)
        } else {
            Ok(screen.status)
        }
    }
}
