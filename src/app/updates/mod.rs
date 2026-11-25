//! Updates: Plays checks its feed a minute after it starts and once a day
//! while Settings → About says to, or when asked; downloads what it finds
//! in the background, as a delta from this version where the feed has
//! one; and swaps it in as Plays restarts for it, or quits.
//!
//! The feed's URL and the keys it's signed with are in
//! `resources/updates.json` (`feed`); with no feed there, updates are off.

mod feed;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{App, BorrowAppContext, Global};
use updater::{Release, Version};

use crate::settings::Settings;

/// How long after starting Plays first checks.
pub const FIRST_CHECK: Duration = Duration::from_secs(60);
/// How long after a check the next is.
pub const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How often it looks whether the next check is due: a computer asleep
/// past it checks soon after it wakes.
pub const LOOK_EVERY: Duration = Duration::from_secs(60 * 60);

/// A version newer than this one, which the feed has an update to.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub version: Version,
    /// Each release since this one, newest first, its notes in Markdown.
    pub releases: Vec<Release>,
    /// How many bytes it takes to download.
    pub size: u64,
}

/// Where updates come from: the feed, or a test's. Each but `install` runs
/// off the main thread, and may take a while.
pub trait Source: Send + Sync + 'static {
    /// The newer version the feed has, if it has one.
    fn check(&self) -> Result<Option<Found>, String>;
    /// What `check` found last, downloaded and put together beside the
    /// install; `progress` told how many of how many bytes have come.
    fn download(
        &self,
        progress: &mut dyn FnMut(u64, u64),
        cancel: &AtomicBool,
    ) -> Result<(), String>;
    /// What `download` put together swapped in for the install: quick, as
    /// it's a rename.
    fn install(&self) -> Result<(), String>;
    /// Where Plays is installed, to start again from.
    fn installed_at(&self) -> PathBuf;
}

/// Where updating is.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Plays doesn't update itself here: why.
    Off(String),
    /// Not checked yet.
    Idle,
    Checking,
    UpToDate,
    Downloading {
        found: Found,
        done: u64,
    },
    /// Put together beside the install, to swap in as Plays restarts or
    /// quits.
    Ready(Found),
    /// Swapped in: Plays runs it once it starts again.
    Installed(Found),
    /// Why it couldn't check, download or install.
    Failed(String),
}

impl State {
    /// The update found, if there's one.
    pub fn found(&self) -> Option<&Found> {
        match self {
            Self::Downloading { found, .. } | Self::Ready(found) | Self::Installed(found) => {
                Some(found)
            }
            _ => None,
        }
    }

    /// Whether a check may start: none is under way, and nothing found
    /// waits to be installed.
    fn may_check(&self) -> bool {
        matches!(self, Self::Idle | Self::UpToDate | Self::Failed(_))
    }
}

/// Updating, which the app keeps.
pub struct Updates {
    source: Option<Arc<dyn Source>>,
    state: State,
    /// When it last checked, by the executor's clock.
    checked: Option<Instant>,
    cancel: Arc<AtomicBool>,
    /// How Plays starts again: its own way, or a test's.
    restart: fn(&mut App),
}

impl Global for Updates {}

/// This version of Plays.
pub fn version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("Plays' version is one")
}

/// Updates from the feed, if Plays has one and updates itself here.
pub fn start(cx: &mut App) {
    start_with(feed::open(), App::restart, cx);
}

/// Updates from `source`, or off for why it isn't one; started again with
/// `restart`.
pub(crate) fn start_with(
    source: Result<Arc<dyn Source>, String>,
    restart: fn(&mut App),
    cx: &mut App,
) {
    let (source, state) = match source {
        Ok(source) => (Some(source), State::Idle),
        Err(why) => (None, State::Off(why)),
    };
    let on = source.is_some();
    cx.set_global(Updates {
        source,
        state,
        checked: None,
        cancel: Arc::default(),
        restart,
    });
    if !on {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(FIRST_CHECK).await;
        loop {
            if cx
                .update(|cx| {
                    if due(cx) {
                        check(cx)
                    }
                })
                .is_err()
            {
                return;
            }
            cx.background_executor().timer(LOOK_EVERY).await;
        }
    })
    .detach();
    cx.on_app_quit(|cx| {
        install_on_quit(cx);
        async {}
    })
    .detach();
}

/// Where updating is now.
pub fn state(cx: &App) -> State {
    cx.try_global::<Updates>().map_or_else(
        || State::Off("this build of Plays doesn't update itself".into()),
        |updates| updates.state.clone(),
    )
}

/// Whether Settings have Plays check by itself, and it's a day since the
/// last check.
fn due(cx: &App) -> bool {
    let updates = cx.global::<Updates>();
    let now = cx.background_executor().now();
    cx.global::<Settings>().check_for_updates
        && updates.state.may_check()
        && updates
            .checked
            .is_none_or(|checked| now.duration_since(checked) >= EVERY)
}

/// Check for an update, and download it if there's one, unless a check is
/// under way or an update waits to be installed.
pub fn check(cx: &mut App) {
    let Some(updates) = cx.try_global::<Updates>() else {
        return;
    };
    let Some(source) = updates.source.clone().filter(|_| updates.state.may_check()) else {
        return;
    };
    let cancel = Arc::clone(&updates.cancel);
    let now = cx.background_executor().now();
    set(cx, |updates| {
        updates.state = State::Checking;
        updates.checked = Some(now);
    });
    let checking = cx.background_executor().spawn({
        let source = Arc::clone(&source);
        async move { source.check() }
    });
    cx.spawn(async move |cx| {
        let found = match checking.await {
            Ok(Some(found)) => found,
            Ok(None) => {
                cx.update(|cx| set(cx, |updates| updates.state = State::UpToDate))
                    .ok();
                return;
            }
            Err(why) => {
                cx.update(|cx| fail(why, cx)).ok();
                return;
            }
        };
        let downloading = State::Downloading {
            found: found.clone(),
            done: 0,
        };
        cx.update(|cx| set(cx, |updates| updates.state = downloading))
            .ok();
        let (told, mut progress) = mpsc::unbounded();
        let downloaded = cx.background_executor().spawn(async move {
            let mut percent = None;
            let mut tell = |done: u64, total: u64| {
                // Each percent, not each read.
                let now = done.saturating_mul(100).checked_div(total);
                if now != percent {
                    percent = now;
                    told.unbounded_send(done).ok();
                }
            };
            source.download(&mut tell, &cancel)
        });
        while let Some(done) = progress.next().await {
            let came = |updates: &mut Updates| {
                if let State::Downloading { done: was, .. } = &mut updates.state {
                    *was = done;
                }
            };
            if cx.update(|cx| set(cx, came)).is_err() {
                return;
            }
        }
        let downloaded = downloaded.await;
        cx.update(|cx| match downloaded {
            Ok(()) => set(cx, |updates| updates.state = State::Ready(found)),
            Err(why) => fail(why, cx),
        })
        .ok();
    })
    .detach();
}

/// Install the update that's ready, and start Plays again to run it.
pub fn restart(cx: &mut App) -> Result<(), String> {
    match state(cx) {
        State::Ready(_) => install(cx)?,
        State::Installed(_) => {}
        _ => return Err("no update is ready to install".into()),
    }
    let updates = cx.global::<Updates>();
    let restart = updates.restart;
    let installed_at = updates.source.as_ref().map(|source| source.installed_at());
    // macOS opens the app bundle again, wherever it was started from.
    if let Some(path) = installed_at.filter(|_| !cfg!(target_os = "macos")) {
        cx.set_restart_path(path);
    }
    restart(cx);
    Ok(())
}

/// Swap the update that's ready in for the install.
fn install(cx: &mut App) -> Result<(), String> {
    let updates = cx.global::<Updates>();
    let (Some(source), State::Ready(found)) = (updates.source.clone(), updates.state.clone())
    else {
        return Err("no update is ready to install".into());
    };
    match source.install() {
        Ok(()) => {
            log::info!("installed Plays {}", found.version);
            set(cx, |updates| updates.state = State::Installed(found));
            Ok(())
        }
        Err(why) => {
            fail(why.clone(), cx);
            Err(why)
        }
    }
}

/// As Plays quits: a download stopped, and an update that's ready swapped
/// in, for Plays to run once it starts again.
fn install_on_quit(cx: &mut App) {
    let updates = cx.global::<Updates>();
    updates.cancel.store(true, Ordering::Relaxed);
    if matches!(updates.state, State::Ready(_))
        && let Err(why) = install(cx)
    {
        log::warn!("{why}");
    }
}

fn fail(why: String, cx: &mut App) {
    log::warn!("{why}");
    set(cx, |updates| updates.state = State::Failed(why));
}

/// `change` made to updating, and the windows that show it drawn again.
fn set(cx: &mut App, change: impl FnOnce(&mut Updates)) {
    cx.update_global::<Updates, _>(|updates, _| change(updates));
    cx.refresh_windows();
}

#[cfg(test)]
#[path = "../tests/updates.rs"]
mod tests;
