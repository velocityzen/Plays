//! By hand, against real devices on the network:
//!
//! ```text
//! cargo run -p chromecast --example chromecast -- list
//! cargo run -p chromecast --example chromecast -- play "Living Room TV" http://…/clip.mp4 video/mp4
//! ```
//!
//! `play` casts the URL and says what the device does with it, to the end
//! or for two minutes, then stops the cast.

use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use chromecast::{
    Browser, Device, Event, IdleReason, Load, Media, PlayerState, Session, StreamType,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = match args.as_slice() {
        ["list"] => list(),
        ["play", name, url, content_type] => play(name, url, content_type),
        _ => Err("list | play NAME URL CONTENT-TYPE".into()),
    };
    if let Err(err) = done {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn list() -> Result<()> {
    let (browser, _found) = Browser::start()?;
    thread::sleep(Duration::from_secs(4));
    for device in browser.devices() {
        println!(
            "{:?} ({}) at {:?}: video {}, sound {}, group {}, showing {:?}",
            device.name,
            device.model,
            device.addresses,
            device.shows_video(),
            device.plays_audio(),
            device.is_group(),
            device.showing
        );
    }
    Ok(())
}

/// The device called `name`, waited for up to five seconds.
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
    Err(format!("no device called {name:?}").into())
}

fn play(name: &str, url: &str, content_type: &str) -> Result<()> {
    let (session, mut events) = Session::connect(&find(name)?)?;
    let media = Media {
        url: url.to_owned(),
        content_type: content_type.to_owned(),
        stream_type: StreamType::Buffered,
        title: Some("Plays".into()),
        duration: None,
        text_tracks: Vec::new(),
    };
    println!("{:?}", session.load(&media, &Load::default())?);
    let deadline = Instant::now() + Duration::from_secs(120);
    'casting: while Instant::now() < deadline {
        thread::sleep(Duration::from_secs(1));
        while let Ok(event) = events.try_recv() {
            println!("{event:?}");
            match event {
                Event::Ended(_) => break 'casting,
                Event::Media(status)
                    if status.state == PlayerState::Idle(Some(IdleReason::Finished)) =>
                {
                    break 'casting;
                }
                _ => {}
            }
        }
    }
    session.stop();
    Ok(())
}
