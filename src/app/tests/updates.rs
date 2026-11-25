//! Checking for updates, downloading them and installing one, from a
//! source of the test's own.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;

use gpui::TestAppContext;
use parking_lot::Mutex;

use super::*;
use crate::api::{self, Invocation};

/// A feed, as the test has it: what each check finds, how downloading and
/// installing go, and how many times each was asked.
struct Fake {
    finds: Mutex<Result<Option<Found>, String>>,
    downloads: Mutex<Result<(), String>>,
    installs: Mutex<Result<(), String>>,
    checked: AtomicUsize,
    downloaded: AtomicUsize,
    installed: AtomicUsize,
}

impl Fake {
    fn finding(found: Option<Found>) -> Arc<Self> {
        Arc::new(Self {
            finds: Mutex::new(Ok(found)),
            downloads: Mutex::new(Ok(())),
            installs: Mutex::new(Ok(())),
            checked: AtomicUsize::new(0),
            downloaded: AtomicUsize::new(0),
            installed: AtomicUsize::new(0),
        })
    }

    fn checked(&self) -> usize {
        self.checked.load(Ordering::Relaxed)
    }

    fn installed(&self) -> usize {
        self.installed.load(Ordering::Relaxed)
    }
}

impl Source for Fake {
    fn check(&self) -> Result<Option<Found>, String> {
        self.checked.fetch_add(1, Ordering::Relaxed);
        self.finds.lock().clone()
    }

    fn download(&self, progress: &mut dyn FnMut(u64, u64), _: &AtomicBool) -> Result<(), String> {
        self.downloaded.fetch_add(1, Ordering::Relaxed);
        for done in [0, 10, 500, 501, 1_000] {
            progress(done, 1_000);
        }
        self.downloads.lock().clone()
    }

    fn install(&self) -> Result<(), String> {
        self.installed.fetch_add(1, Ordering::Relaxed);
        self.installs.lock().clone()
    }

    fn installed_at(&self) -> PathBuf {
        "/Applications/Plays.app".into()
    }
}

fn found() -> Found {
    let release = Release {
        version: Version::new(9, 0, 0),
        published: 0,
        notes: "## 9.0.0\n\n- Plays more.".into(),
        downloads: BTreeMap::new(),
    };
    Found {
        version: release.version.clone(),
        releases: vec![release],
        size: 1_000,
    }
}

thread_local! {
    /// How many times Plays was started again, on the test's thread.
    static RESTARTS: Cell<usize> = const { Cell::new(0) };
}

fn restarted(_: &mut App) {
    RESTARTS.with(|restarts| restarts.set(restarts.get() + 1));
}

fn restarts() -> usize {
    RESTARTS.with(Cell::get)
}

/// Updates from `fake`, with `settings`.
fn start(fake: &Arc<Fake>, settings: Settings, cx: &mut TestAppContext) {
    let source: Arc<dyn Source> = fake.clone();
    cx.update(|cx| {
        cx.set_global(settings);
        start_with(Ok(source), restarted, cx);
    });
}

/// Check, and let it all run.
fn check_now(cx: &mut TestAppContext) {
    cx.update(check);
    cx.run_until_parked();
}

fn state_now(cx: &mut TestAppContext) -> State {
    cx.update(|cx| state(cx))
}

#[gpui::test]
fn without_a_source_updates_are_off_and_say_why(cx: &mut TestAppContext) {
    cx.update(|cx| {
        assert_eq!(
            state(cx),
            State::Off("this build of Plays doesn't update itself".into())
        );
        cx.set_global(Settings::default());
        start_with(
            Err("Flatpak installed it, and updates it".into()),
            restarted,
            cx,
        );
        check(cx);
        assert_eq!(
            state(cx),
            State::Off("Flatpak installed it, and updates it".into())
        );
    });
    cx.executor().advance_clock(EVERY * 2);
    cx.run_until_parked();
    assert!(matches!(state_now(cx), State::Off(_)));
}

#[gpui::test]
fn a_check_that_finds_nothing_is_up_to_date(cx: &mut TestAppContext) {
    let fake = Fake::finding(None);
    start(&fake, Settings::default(), cx);
    assert_eq!(state_now(cx), State::Idle);
    check_now(cx);
    assert_eq!(state_now(cx), State::UpToDate);
    assert_eq!(fake.checked(), 1);
    assert_eq!(fake.downloaded.load(Ordering::Relaxed), 0);
}

#[gpui::test]
fn an_update_found_downloads_saying_how_far_then_is_ready(cx: &mut TestAppContext) {
    let fake = Fake::finding(Some(found()));
    start(&fake, Settings::default(), cx);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let _watching = cx.update(|cx| {
        let seen = Rc::clone(&seen);
        cx.observe_global::<Updates>(move |cx| seen.borrow_mut().push(state(cx)))
    });
    check_now(cx);
    let downloading = |done| State::Downloading {
        found: found(),
        done,
    };
    assert_eq!(
        *seen.borrow(),
        [
            State::Checking,
            downloading(0),
            downloading(0),
            downloading(10),
            downloading(500),
            downloading(1_000),
            State::Ready(found()),
        ],
        "a percent at a time"
    );
    assert_eq!(state_now(cx).found(), Some(&found()));
}

#[gpui::test]
fn what_fails_says_why_and_can_be_checked_again(cx: &mut TestAppContext) {
    let fake = Fake::finding(None);
    *fake.finds.lock() = Err("can't update: the feed is out of date".into());
    start(&fake, Settings::default(), cx);
    check_now(cx);
    assert_eq!(
        state_now(cx),
        State::Failed("can't update: the feed is out of date".into())
    );

    *fake.finds.lock() = Ok(Some(found()));
    *fake.downloads.lock() = Err("can't update: the update doesn't apply: it's cut short".into());
    check_now(cx);
    assert_eq!(
        state_now(cx),
        State::Failed("can't update: the update doesn't apply: it's cut short".into())
    );

    *fake.downloads.lock() = Ok(());
    check_now(cx);
    assert_eq!(state_now(cx), State::Ready(found()));
    assert_eq!(fake.checked(), 3);
}

#[gpui::test]
fn an_update_ready_isnt_checked_past(cx: &mut TestAppContext) {
    let fake = Fake::finding(Some(found()));
    start(&fake, Settings::default(), cx);
    check_now(cx);
    check_now(cx);
    cx.executor().advance_clock(EVERY * 3);
    cx.run_until_parked();
    assert_eq!(fake.checked(), 1);
    assert_eq!(state_now(cx), State::Ready(found()));
}

#[gpui::test]
fn plays_checks_a_minute_after_starting_then_once_a_day(cx: &mut TestAppContext) {
    let fake = Fake::finding(None);
    start(&fake, Settings::default(), cx);
    cx.executor()
        .advance_clock(FIRST_CHECK - Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fake.checked(), 0);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fake.checked(), 1);
    cx.executor().advance_clock(EVERY - LOOK_EVERY);
    cx.run_until_parked();
    assert_eq!(fake.checked(), 1, "not until a day's gone by");
    cx.executor().advance_clock(LOOK_EVERY);
    cx.run_until_parked();
    assert_eq!(fake.checked(), 2);
}

#[gpui::test]
fn a_check_by_hand_puts_the_next_off_a_day(cx: &mut TestAppContext) {
    let fake = Fake::finding(None);
    start(&fake, Settings::default(), cx);
    check_now(cx);
    cx.executor().advance_clock(FIRST_CHECK + LOOK_EVERY);
    cx.run_until_parked();
    assert_eq!(fake.checked(), 1);
}

#[gpui::test]
fn plays_doesnt_check_by_itself_once_settings_say_not_to(cx: &mut TestAppContext) {
    let fake = Fake::finding(None);
    let settings = Settings {
        check_for_updates: false,
        ..Settings::default()
    };
    start(&fake, settings, cx);
    cx.executor().advance_clock(EVERY * 2);
    cx.run_until_parked();
    assert_eq!(fake.checked(), 0);
    check_now(cx);
    assert_eq!(fake.checked(), 1, "but when asked");
}

#[gpui::test]
fn restarting_installs_the_update_and_starts_plays_again(cx: &mut TestAppContext) {
    let fake = Fake::finding(Some(found()));
    start(&fake, Settings::default(), cx);
    check_now(cx);
    let before = restarts();
    cx.update(restart).unwrap();
    assert_eq!(fake.installed(), 1);
    assert_eq!(restarts(), before + 1);
    assert_eq!(state_now(cx), State::Installed(found()));
    // And once it's installed, nothing is installed again as it quits.
    cx.quit();
    assert_eq!(fake.installed(), 1);
}

#[gpui::test]
fn restarting_with_no_update_ready_is_refused(cx: &mut TestAppContext) {
    let fake = Fake::finding(None);
    start(&fake, Settings::default(), cx);
    check_now(cx);
    let before = restarts();
    let restarting = cx.update(|cx| api::run(Invocation::new("app.restart_to_update"), cx));
    let refused = restarting.unwrap_err().to_string();
    assert!(
        refused.contains("No update is ready to install"),
        "{refused}"
    );
    assert_eq!((fake.installed(), restarts()), (0, before));
}

#[gpui::test]
fn an_update_that_cant_be_installed_says_why_and_plays_goes_on(cx: &mut TestAppContext) {
    let fake = Fake::finding(Some(found()));
    *fake.installs.lock() = Err("can't update: /Applications can't be written to".into());
    start(&fake, Settings::default(), cx);
    check_now(cx);
    let before = restarts();
    let restarting = cx.update(restart);
    assert_eq!(
        restarting,
        Err("can't update: /Applications can't be written to".into())
    );
    assert_eq!(restarts(), before);
    assert_eq!(
        state_now(cx),
        State::Failed("can't update: /Applications can't be written to".into())
    );
}

#[gpui::test]
fn an_update_ready_is_installed_as_plays_quits(cx: &mut TestAppContext) {
    let fake = Fake::finding(Some(found()));
    start(&fake, Settings::default(), cx);
    check_now(cx);
    cx.quit();
    assert_eq!(fake.installed(), 1);
}

#[gpui::test]
fn nothing_is_installed_as_plays_quits_with_no_update_ready(cx: &mut TestAppContext) {
    let fake = Fake::finding(None);
    start(&fake, Settings::default(), cx);
    check_now(cx);
    cx.quit();
    assert_eq!(fake.installed(), 0);
}
