//! By hand, against real receivers on the network:
//!
//! ```text
//! cargo run -p airplay --example airplay -- list
//! cargo run -p airplay --example airplay -- screen "Living Room"
//! cargo run -p airplay --example airplay -- pair "Living Room" tv.pairing
//! cargo run -p airplay --example airplay -- play "Living Room" http://…/clip.mp4 tv.pairing
//! cargo run -p airplay --example airplay -- tone Kitchen 0.2
//! ```
//!
//! `screen` says what the TV's `/info` says its screen shows; `pair` asks
//! for the PIN the TV shows and keeps what pairing leaves in the file;
//! `play` has the TV play the URL to the end, or for two minutes; `tone`
//! plays two seconds of a quiet tone at the volume given, then puts the
//! volume back.

use std::error::Error;
use std::io::BufRead;
use std::thread;
use std::time::{Duration, Instant};

use airplay::{
    AudioStream, Browser, Credentials, Device, PinPairing, PlaybackState, Screen, Sender,
    VideoSession,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = match args.as_slice() {
        ["list"] => list(),
        ["screen", name] => screen(name),
        ["pair", name, file] => pair(name, file),
        ["play", name, url] => play(name, url, None),
        ["play", name, url, file] => play(name, url, Some(file)),
        ["tone", name, volume] => volume
            .parse()
            .map_err(Into::into)
            .and_then(|volume| tone(name, volume)),
        _ => Err(
            "list | screen NAME | pair NAME FILE | play NAME URL [FILE] | tone NAME VOLUME".into(),
        ),
    };
    if let Err(err) = done {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn sender() -> Sender {
    Sender::new("Plays")
}

fn list() -> Result<()> {
    let (browser, _found) = Browser::start()?;
    thread::sleep(Duration::from_secs(4));
    for device in browser.devices() {
        println!(
            "{:?} ({}) at {:?}: video {}, sound {}, AirPlay 2 {}, pairing {:?}",
            device.name,
            device.model,
            device.addresses,
            device.shows_video(),
            device.plays_audio(),
            device.airplay2(),
            device.pairing()
        );
    }
    Ok(())
}

fn screen(name: &str) -> Result<()> {
    let device = find(name)?;
    println!("{:?}", Screen::ask(&device));
    Ok(())
}

/// The receiver called `name`, waited for up to five seconds.
fn find(name: &str) -> Result<Device> {
    let (browser, _found) = Browser::start()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(device) = browser
            .devices()
            .into_iter()
            .find(|device| device.name == name)
        {
            return Ok(device);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("no receiver called {name:?}").into())
}

fn pair(name: &str, file: &str) -> Result<()> {
    let pairing = PinPairing::start(&find(name)?)?;
    println!("The PIN {name} shows:");
    let mut pin = String::new();
    std::io::stdin().lock().read_line(&mut pin)?;
    let credentials = pairing.finish(pin.trim())?;
    std::fs::write(file, credentials.to_text())?;
    println!("paired; kept in {file}");
    Ok(())
}

fn play(name: &str, url: &str, file: Option<&str>) -> Result<()> {
    let device = find(name)?;
    let credentials = file
        .map(|file| -> Result<Credentials> {
            let text = std::fs::read_to_string(file)?;
            Ok(Credentials::from_text(&text).ok_or("not what pairing leaves")?)
        })
        .transpose()?;
    let (mut session, mut events) =
        VideoSession::connect(&device, credentials.as_ref(), &sender())?;
    session.play(url, 0.0, true)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        thread::sleep(Duration::from_secs(1));
        if let Ok(event) = events.try_recv() {
            println!("{event:?}");
            break;
        }
        let info = session.playback_info()?;
        println!("{info:?}");
        if info.state == PlaybackState::Ended {
            break;
        }
    }
    session.stop();
    Ok(())
}

fn tone(name: &str, volume: f32) -> Result<()> {
    let (mut stream, _events) = AudioStream::connect(&find(name)?, None, &sender())?;
    let before = stream.volume()?;
    stream.set_volume(volume, false)?;
    println!(
        "volume {before} → {volume}; heard {:?} after it's sent",
        stream.latency()
    );
    let tone: Vec<i16> = (0..2 * 44_100)
        .flat_map(|i| {
            let phase = i as f32 * 440.0 * std::f32::consts::TAU / 44_100.0;
            let sample = (1638.0 * phase.sin()) as i16;
            [sample, sample]
        })
        .collect();
    for chunk in tone.chunks(1024) {
        stream.write(chunk)?;
    }
    thread::sleep(stream.latency() + Duration::from_millis(500));
    stream.set_volume(before, false)?;
    stream.stop();
    Ok(())
}
