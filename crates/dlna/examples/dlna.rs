//! By hand, against real renderers on the network:
//!
//! ```text
//! cargo run -p dlna --example dlna -- list
//! cargo run -p dlna --example dlna -- play "[TV] Living Room" http://…/clip.mp4 video/mp4
//! cargo run -p dlna --example dlna -- play "[TV] Living Room" http://…/clip.mp4 video/mp4 http://…/clip.srt
//! ```
//!
//! `list` says what each renderer plays too. `play` has the renderer play
//! the URL, with subtitles when given, and says what it does, to the end
//! or for two minutes, then stops it.

use std::error::Error;
use std::thread;
use std::time::{Duration, Instant};

use dlna::{Browser, Device, Event, Kind, Media, Session, Subtitles, TransportState};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let done = match args.as_slice() {
        ["list"] => list(),
        ["play", name, url, mime] => play(name, url, mime, None),
        ["play", name, url, mime, subtitles] => play(name, url, mime, Some(subtitles)),
        _ => Err("list | play NAME URL MIME [SUBTITLES-URL]".into()),
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
            "{:?} ({} {}) at {}",
            device.name, device.manufacturer, device.model, device.location
        );
        match device.capabilities() {
            Ok(capabilities) => {
                let mut mimes: Vec<&str> = capabilities
                    .iter()
                    .filter(|it| it.is_http())
                    .map(|it| it.mime.as_str())
                    .collect();
                mimes.dedup();
                println!("  plays {}", mimes.join(", "));
            }
            Err(err) => println!("  plays what it doesn't say: {err}"),
        }
    }
    Ok(())
}

/// The renderer called `name`, waited for up to five seconds.
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
    Err(format!("no renderer called {name:?}").into())
}

fn play(name: &str, url: &str, mime: &str, subtitles: Option<&str>) -> Result<()> {
    let (session, mut events) = Session::connect(&find(name)?)?;
    let kind = match mime.split('/').next() {
        Some("audio") => Kind::Audio,
        Some("image") => Kind::Image,
        _ => Kind::Video,
    };
    let media = Media {
        url: url.to_owned(),
        mime: mime.to_owned(),
        features: None,
        kind,
        title: Some("Plays".into()),
        duration: None,
        size: None,
        subtitles: subtitles.map(|url| Subtitles {
            url: url.to_owned(),
            mime: "text/srt".into(),
        }),
    };
    session.load(&media)?;
    session.play()?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut played = false;
    'playing: while Instant::now() < deadline {
        thread::sleep(Duration::from_secs(1));
        while let Ok(event) = events.try_recv() {
            println!("{event:?}");
            match event {
                Event::Ended(_) => break 'playing,
                Event::Status(status) => {
                    played |= status.state == TransportState::Playing;
                    if played && status.state == TransportState::Stopped {
                        break 'playing;
                    }
                }
                Event::Volume(_) => {}
            }
        }
    }
    session.stop();
    Ok(())
}
