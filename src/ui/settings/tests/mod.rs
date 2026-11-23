//! The Settings window, and the settings the menus change, driven as
//! someone at the screen would (see `ui::testing`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Weak};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use airplay::PinPairing;
use airplay::fake::{FakeReceiver, Kind};
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, point, px, size};
use parking_lot::Mutex;

use super::SettingsView;
use crate::api::commands::output;
use crate::app::AudioEngine;
use crate::cast::fake::{FakeTarget, KIND};
use crate::cast::target::{Device, Error, Session};
use crate::cast::{AppPairings, Casting, Looking, Protocol, Shows, Target};
use crate::files::Folder;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, MusicMode, PanelLook, Settings, Visualizer};
use crate::ui::about::AboutView;
use crate::ui::testing::{self, click, press, shows, wait_until};
use crate::ui::theme::layout::SETTINGS_WINDOW_SIZE;
use crate::ui::workspace::forget_settings_place;

mod commands;

/// Open the Settings window and bring it to the front.
fn open_settings(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.update(|_, cx| SettingsView::open_window(cx));
    cx.run_until_parked();
    let handle = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<SettingsView>())
        .expect("a Settings window");
    let mut settings = VisualTestContext::from_window(handle.into(), cx);
    settings.update(|window, _| window.activate_window());
    settings.run_until_parked();
    settings
}

/// Open the About window and bring it to the front.
fn open_about(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.update(|_, cx| AboutView::open_window(cx));
    cx.run_until_parked();
    let handle = cx
        .windows()
        .into_iter()
        .find_map(|window| window.downcast::<AboutView>())
        .expect("an About window");
    let mut about = VisualTestContext::from_window(handle.into(), cx);
    about.update(|window, _| window.activate_window());
    about.run_until_parked();
    about
}

/// Move and resize the window to `bounds`, as someone dragging it would.
fn drag_to(window: &mut VisualTestContext, bounds: Bounds<Pixels>) {
    window.update(|window, _| window.set_frame(bounds));
    window.simulate_resize(bounds.size);
    window.run_until_parked();
}

fn close(window: &mut VisualTestContext) {
    window.update(|window, _| window.remove_window());
    window.run_until_parked();
}

fn bounds_of(window: &mut VisualTestContext) -> Bounds<Pixels> {
    window.update(|window, _| window.bounds())
}

#[gpui::test]
fn settings_opens_where_it_was_left_this_launch_or_the_last(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let folder = Folder::for_test("settings-window");
    cx.update(|_, cx| cx.set_global(folder.clone()));
    let main = cx.update(|window, cx| window.display(cx).unwrap().bounds());

    // The first time, in the middle of the display, at its own size.
    let mut settings = open_settings(&mut cx);
    let first = bounds_of(&mut settings);
    assert_eq!(first.size, SETTINGS_WINDOW_SIZE);
    assert_eq!(first.center(), main.center(), "in the middle");

    let left = Bounds::new(point(px(60.), px(80.)), size(px(900.), px(640.)));
    drag_to(&mut settings, left);
    close(&mut settings);
    let mut settings = open_settings(&mut cx);
    assert_eq!(bounds_of(&mut settings), left, "where it was left");

    // Saved as it closes, the next launch opens it there too.
    let elsewhere = Bounds::new(point(px(300.), px(200.)), size(px(800.), px(600.)));
    drag_to(&mut settings, elsewhere);
    close(&mut settings);
    assert!(
        folder.file("settings-window.json").unwrap().exists(),
        "saved beside the settings"
    );
    cx.update(|_, cx| forget_settings_place(cx));
    let mut settings = open_settings(&mut cx);
    assert_eq!(bounds_of(&mut settings), elsewhere, "the next launch");
    close(&mut settings);

    // Left on a display since gone: in the middle of the one in use, at
    // the size it had.
    let gone = r#"{"display": 99, "name": "Gone", "x": 10, "y": 10, "width": 700, "height": 500}"#;
    std::fs::write(folder.file("settings-window.json").unwrap(), gone).unwrap();
    cx.update(|_, cx| forget_settings_place(cx));
    let mut settings = open_settings(&mut cx);
    let centered = bounds_of(&mut settings);
    assert_eq!(centered.size, size(px(700.), px(500.)));
    assert_eq!(centered.center(), main.center());

    // Still open as the app quits, where it is is saved.
    let quitting = Bounds::new(point(px(120.), px(140.)), size(px(760.), px(560.)));
    drag_to(&mut settings, quitting);
    TestAppContext::update(&cx, |cx| cx.shutdown());
    let saved = std::fs::read_to_string(folder.file("settings-window.json").unwrap()).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    let at = ["x", "y", "width", "height"].map(|key| saved[key].as_f64());
    assert_eq!(
        at,
        [120., 140., 760., 560.].map(Some),
        "saved as the app quit"
    );
}

#[gpui::test]
fn about_opens_in_the_middle_wherever_it_was_left(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let main = cx.update(|window, cx| window.display(cx).unwrap().bounds());
    let mut about = open_about(&mut cx);
    let middle = bounds_of(&mut about);
    assert_eq!(middle.center(), main.center());

    drag_to(
        &mut about,
        Bounds::new(point(px(40.), px(40.)), middle.size),
    );
    close(&mut about);
    let mut about = open_about(&mut cx);
    assert_eq!(bounds_of(&mut about), middle, "back in the middle");
}

fn autoplay(cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| cx.global::<Settings>().autoplay)
}

#[gpui::test]
fn reset_to_defaults_asks_first(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    cx.update(|_, cx| Settings::update(cx, false, |s| s.autoplay = false));
    let mut settings = open_settings(&mut cx);

    click(&mut settings, "settings-file");
    click(&mut settings, "settings-file-reset");
    let (question, _) = settings.pending_prompt().expect("it asks");
    assert!(question.contains("Reset all settings"), "{question}");
    settings.simulate_prompt_answer("Cancel");
    settings.run_until_parked();
    assert!(!autoplay(&mut settings), "Cancel leaves the settings be");

    click(&mut settings, "settings-file");
    click(&mut settings, "settings-file-reset");
    settings.simulate_prompt_answer("Reset");
    settings.run_until_parked();
    assert!(autoplay(&mut settings), "Reset puts the default back");
}

#[gpui::test]
fn picking_how_panels_show_the_video_sets_it(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    let panels = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<Settings>().panels);
    click(&mut settings, "pane-Appearance");
    assert_eq!(panels(&mut settings), PanelLook::Frosted, "the default");

    for (choice, look) in [
        ("panels-Glass", PanelLook::Glass),
        ("panels-Glow", PanelLook::Glow),
        ("panels-Tint", PanelLook::Tint),
        ("panels-Plain", PanelLook::Plain),
        ("panels-Frosted", PanelLook::Frosted),
    ] {
        click(&mut settings, choice);
        assert_eq!(panels(&mut settings), look, "{choice}");
    }
}

#[gpui::test]
fn music_mode_picks_the_visualizer_and_whether_the_quality_shows(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    let mode = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<Settings>().music_mode);
    click(&mut settings, "pane-Music Mode");
    assert_eq!(
        mode(&mut settings),
        MusicMode {
            visualizer: Visualizer::Speaker,
            show_quality: true,
        },
        "the default"
    );

    click(&mut settings, "visualizer-None");
    assert_eq!(mode(&mut settings).visualizer, Visualizer::None);
    click(&mut settings, "visualizer-Speaker");
    assert_eq!(mode(&mut settings).visualizer, Visualizer::Speaker);
    click(&mut settings, "show-quality");
    assert!(!mode(&mut settings).show_quality);
    click(&mut settings, "show-quality");
    assert!(mode(&mut settings).show_quality);
}

#[gpui::test]
fn the_screensaver_turns_off_and_waits_as_long_as_picked(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Appearance");
    // Tall enough for its rows, at the foot of the pane.
    settings.simulate_resize(gpui::size(px(720.), px(1400.)));
    let saver = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let s = cx.global::<Settings>();
            (s.screensaver, s.screensaver_minutes)
        })
    };
    assert_eq!(saver(&mut settings), (true, 5), "on, after 5 minutes");

    click(&mut settings, "screensaver-minutes-10 min");
    assert_eq!(saver(&mut settings), (true, 10));
    click(&mut settings, "screensaver");
    assert_eq!(saver(&mut settings), (false, 10));
    assert!(
        !shows(&mut settings, "screensaver-minutes-10 min"),
        "off, it waits for nothing"
    );
    click(&mut settings, "screensaver");
    click(&mut settings, "screensaver-minutes-1 min");
    assert_eq!(saver(&mut settings), (true, 1));
}

#[gpui::test]
fn each_panel_over_the_video_is_switched_off_and_on(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Appearance");
    // Tall enough for every row.
    settings.simulate_resize(gpui::size(px(720.), px(1600.)));
    let shown = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let s = cx.global::<Settings>().over_video;
            [s.controls, s.top_bar, s.track_switcher, s.notes]
        })
    };
    let switches = [
        "over_video.controls",
        "over_video.top_bar",
        "over_video.track_switcher",
        "over_video.notes",
    ];
    for (at, switch) in switches.into_iter().enumerate() {
        click(&mut settings, switch);
        let mut wanted = [true; 4];
        wanted[at] = false;
        assert_eq!(shown(&mut settings), wanted, "{switch}");
        click(&mut settings, switch);
        assert_eq!(shown(&mut settings), [true; 4], "{switch} again");
    }
}

/// A pane's language list's buttons and menu rows, as tests click them:
/// its pane, then Add Language, Afrikaans and Arabic in its menu, and
/// the second's up, the first's down and the first's remove buttons.
type LanguageButtons = [&'static str; 7];

/// A pane's language list: its buttons, and the languages it has.
type LanguageList = (LanguageButtons, fn(&Settings) -> Vec<String>);

#[gpui::test]
fn languages_are_added_moved_and_taken_off_in_audio_and_subtitles(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    // Tall enough for every row.
    settings.simulate_resize(gpui::size(px(720.), px(1800.)));
    let lists: [LanguageList; 2] = [
        (
            [
                "pane-Audio",
                "audio-languages-add",
                "audio-languages-afr",
                "audio-languages-ara",
                "audio-languages-up-1",
                "audio-languages-down-0",
                "audio-languages-remove-0",
            ],
            |s| s.languages.audio.clone(),
        ),
        (
            [
                "pane-Subtitles",
                "subtitle-languages-add",
                "subtitle-languages-afr",
                "subtitle-languages-ara",
                "subtitle-languages-up-1",
                "subtitle-languages-down-0",
                "subtitle-languages-remove-0",
            ],
            |s| s.languages.subtitles.clone(),
        ),
    ];
    for ([pane, add, afrikaans, arabic, up, down, remove], list) in lists {
        let chosen = |cx: &mut VisualTestContext| cx.update(|_, cx| list(cx.global::<Settings>()));
        click(&mut settings, pane);
        assert!(chosen(&mut settings).is_empty(), "{pane}: none at first");
        assert!(!shows(&mut settings, afrikaans), "{pane}: the menu is shut");
        click(&mut settings, add);
        assert!(shows(&mut settings, afrikaans) && shows(&mut settings, arabic));
        click(&mut settings, afrikaans);
        assert_eq!(chosen(&mut settings), ["afr"], "{pane}");
        assert!(!shows(&mut settings, arabic), "{pane}: the menu shuts");

        click(&mut settings, add);
        assert!(!shows(&mut settings, afrikaans), "{pane}: chosen already");
        click(&mut settings, arabic);
        assert_eq!(chosen(&mut settings), ["afr", "ara"], "{pane}: added last");
        click(&mut settings, up);
        assert_eq!(chosen(&mut settings), ["ara", "afr"], "{pane}: up");
        click(&mut settings, down);
        assert_eq!(chosen(&mut settings), ["afr", "ara"], "{pane}: down");
        click(&mut settings, remove);
        assert_eq!(chosen(&mut settings), ["ara"], "{pane}: taken off");

        // Left open, the menu is shut by a click on another pane, which
        // the next shows, and stays shut there and back.
        click(&mut settings, add);
        assert!(shows(&mut settings, afrikaans), "{pane}: open");
        click(&mut settings, "pane-Hotkeys");
        assert!(!shows(&mut settings, afrikaans), "{pane}: shut");
        click(&mut settings, "pane-Hotkeys");
        assert!(
            shows(&mut settings, "restore-hotkeys"),
            "{pane}: Hotkeys shows"
        );
        click(&mut settings, pane);
        assert!(!shows(&mut settings, afrikaans), "{pane}: shut since");
    }
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn picking_how_windows_go_full_screen_sets_it(cx: &mut TestAppContext) {
    use crate::settings::FullScreen;
    use gpui::FullscreenStyle;

    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Appearance");
    // Tall enough for the row, at the foot of the pane.
    settings.simulate_resize(gpui::size(gpui::px(720.), gpui::px(1000.)));
    let picked = |settings: &mut VisualTestContext, cx: &mut VisualTestContext| {
        settings.run_until_parked();
        (
            settings.update(|_, cx| cx.global::<Settings>().full_screen),
            cx.update(|window, _| window.fullscreen_style()),
        )
    };
    assert_eq!(
        picked(&mut settings, &mut cx),
        (FullScreen::OwnSpace, FullscreenStyle::OwnSpace),
        "macOS's own, unless asked"
    );

    click(&mut settings, "full-screen-Over the display");
    assert_eq!(
        picked(&mut settings, &mut cx),
        (FullScreen::OverDisplay, FullscreenStyle::OverDisplay),
        "and the player window goes full screen that way"
    );
    click(&mut settings, "full-screen-Own Space");
    assert_eq!(
        picked(&mut settings, &mut cx),
        (FullScreen::OwnSpace, FullscreenStyle::OwnSpace)
    );
}

#[gpui::test]
fn picking_an_output_in_the_audio_menu_sets_it(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let output = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<Settings>().audio_output.device.clone())
    };
    let device = AudioDevice {
        id: "test-output".into(),
        name: "Test Output".into(),
    };

    cx.dispatch_action(output::set_device(Some(&device)));
    cx.run_until_parked();
    assert_eq!(output(&mut cx), Some(device));

    cx.dispatch_action(output::set_device(None));
    cx.run_until_parked();
    assert_eq!(output(&mut cx), None, "System Default");
}

/// Each protocol, and whether it's looking.
fn looking(cx: &mut VisualTestContext) -> Vec<Looking> {
    cx.update(|_, cx| {
        Casting::protocols(cx)
            .into_iter()
            .map(|(_, _, looking)| looking)
            .collect()
    })
}

fn found(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        Casting::devices(cx)
            .iter()
            .map(|device| device.name.clone())
            .collect()
    })
}

fn look_with(cx: &mut VisualTestContext, protocol: &str, on: bool) {
    cx.update(|_, cx| {
        Settings::update(cx, false, |s| {
            s.casting.look_with.insert(protocol.to_owned(), on);
        })
    });
    cx.run_until_parked();
}

#[gpui::test]
fn casting_lists_a_speaker_found_with_airplay_and_another_protocol_with_each(
    cx: &mut TestAppContext,
) {
    let (_, mut cx) = testing::start(cx);
    let fake = FakeTarget::default();
    fake.add("Phantom", Shows::Sound);
    cx.update(|_, cx| {
        cx.set_global(Casting::local(vec![Arc::new(fake.clone())]));
        let phantom = DeviceInfo {
            id: "airplay:ph".into(),
            name: "Phantom".into(),
        };
        AudioEngine::set_devices(vec![phantom], cx);
    });
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Casting");
    assert!(shows(&mut settings, "found-Fake-Phantom"));
    assert!(
        shows(&mut settings, "found-AirPlay-Phantom"),
        "and as AirPlay"
    );

    look_with(&mut settings, "AirPlay", false);
    assert!(shows(&mut settings, "found-Fake-Phantom"));
    assert!(
        !shows(&mut settings, "found-AirPlay-Phantom"),
        "not while AirPlay doesn't look"
    );
}

#[gpui::test]
fn casting_lists_the_devices_found_and_a_protocol_turned_off_finds_none(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let fake = FakeTarget::default();
    fake.add("Den TV", Shows::Video);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Casting");
    assert!(shows(&mut settings, "found-Fake-Den TV"));

    fake.add("Kitchen", Shows::Sound);
    settings.update(|_, cx| Casting::refresh(cx));
    settings.run_until_parked();
    assert!(
        // Drawn again by itself, as nothing else changed.
        settings.debug_bounds("found-Fake-Kitchen").is_some(),
        "a device shows as it's found"
    );

    click(&mut settings, "look-with-Fake");
    let set = settings.update(|_, cx| cx.global::<Settings>().casting.clone());
    assert!(!set.looks_with(KIND), "it's saved off");
    assert_eq!(looking(&mut settings), [Looking::Off]);
    assert!(found(&mut settings).is_empty());
    assert!(!shows(&mut settings, "found-Fake-Den TV"), "its devices go");

    click(&mut settings, "look-with-Fake");
    assert_eq!(looking(&mut settings), [Looking::On]);
    assert_eq!(found(&mut settings), ["Den TV", "Kitchen"]);
    assert!(shows(&mut settings, "found-Fake-Den TV"));
}

#[gpui::test]
fn a_tv_paired_with_is_listed_and_forgotten_there(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    let device = tv.device();
    let credentials = PinPairing::start(&device)
        .and_then(|pairing| pairing.finish("1234"))
        .expect("paired with the fake");
    let pairings = cx.update(|_, cx| Arc::clone(&cx.global::<AppPairings>().0));
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Casting");
    assert!(!shows(&mut settings, "forget-Fake TV"), "none paired yet");

    pairings.keep(&device.id, &device.name, &credentials);
    settings.update(|window, _| window.refresh());
    settings.run_until_parked();
    assert!(shows(&mut settings, "forget-Fake TV"));

    click(&mut settings, "forget-Fake TV");
    assert!(pairings.paired().is_empty(), "it asks for its PIN again");
    assert!(pairings.credentials(&device.id).is_none());
    assert!(!shows(&mut settings, "forget-Fake TV"), "it goes");
}

#[gpui::test]
fn a_protocol_looks_only_while_its_on(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let started = Rc::new(Cell::new(0));
    let last: Rc<RefCell<Weak<FakeTarget>>> = Rc::default();
    let start = {
        let (started, last) = (Rc::clone(&started), Rc::clone(&last));
        move || {
            started.set(started.get() + 1);
            let target = Arc::new(FakeTarget::default());
            target.add("Den TV", Shows::Video);
            *last.borrow_mut() = Arc::downgrade(&target);
            Ok(target as Arc<dyn Target>)
        }
    };
    let other = || Ok(Arc::new(FakeTarget::default()) as Arc<dyn Target>);
    look_with(&mut cx, KIND, false);
    cx.update(|_, cx| {
        cx.set_global(Casting::new(vec![
            Protocol::new(KIND, "fakes", start),
            Protocol::new("Other", "others", other),
        ]));
        Casting::watch(cx);
    });
    assert_eq!(started.get(), 0, "off from the start, it never looks");
    assert_eq!(looking(&mut cx), [Looking::Off, Looking::On]);

    look_with(&mut cx, KIND, true);
    assert_eq!(started.get(), 1);
    assert_eq!(looking(&mut cx), [Looking::On, Looking::On]);
    assert_eq!(found(&mut cx), ["Den TV"], "what it found is there at once");

    cx.update(|_, cx| Settings::update(cx, false, |s| s.autoplay = false));
    cx.run_until_parked();
    look_with(&mut cx, "Other", false);
    assert_eq!(
        started.get(),
        1,
        "other settings and protocols leave it looking as it is"
    );
    assert_eq!(looking(&mut cx), [Looking::On, Looking::Off]);

    look_with(&mut cx, KIND, false);
    assert!(found(&mut cx).is_empty());
    assert!(
        wait_until(&mut cx, |_| last.borrow().upgrade().is_none()),
        "turned off, it stops looking"
    );
}

/// How long `SlowToStop` takes to stop, as long as a real protocol may.
const STOPS_IN: Duration = Duration::from_millis(500);

/// A protocol that takes a while to stop looking, as the real ones wait
/// for their threads to end: the thread it stopped on, once it has.
struct SlowToStop {
    fake: FakeTarget,
    stopped: Arc<Mutex<Option<ThreadId>>>,
}

impl Target for SlowToStop {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn devices(&self) -> Vec<Device> {
        self.fake.devices()
    }

    fn connect(&self, device: &Device) -> Result<Box<dyn Session>, Error> {
        self.fake.connect(device)
    }
}

impl Drop for SlowToStop {
    fn drop(&mut self) {
        thread::sleep(STOPS_IN);
        *self.stopped.lock() = Some(thread::current().id());
    }
}

#[gpui::test]
fn a_protocol_turned_off_stops_looking_without_holding_up_the_windows(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let stopped = Arc::<Mutex<Option<ThreadId>>>::default();
    let start = {
        let stopped = Arc::clone(&stopped);
        move || {
            let fake = FakeTarget::default();
            fake.add("Den TV", Shows::Video);
            let stopped = Arc::clone(&stopped);
            Ok(Arc::new(SlowToStop { fake, stopped }) as Arc<dyn Target>)
        }
    };
    cx.update(|_, cx| {
        cx.set_global(Casting::new(vec![Protocol::new(KIND, "fakes", start)]));
        Casting::watch(cx);
    });
    assert_eq!(found(&mut cx), ["Den TV"]);

    let began = Instant::now();
    look_with(&mut cx, KIND, false);
    assert!(began.elapsed() < STOPS_IN, "the windows don't wait for it");
    assert!(found(&mut cx).is_empty(), "its devices go at once");
    assert!(
        wait_until(&mut cx, |_| stopped.lock().is_some()),
        "it stops all the same"
    );
    assert_ne!(
        *stopped.lock(),
        Some(thread::current().id()),
        "it stops away from the windows"
    );
}

#[gpui::test]
fn a_protocol_that_cant_look_says_why_and_tries_again_when_turned_on(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let tries = Rc::new(Cell::new(0));
    let start = {
        let tries = Rc::clone(&tries);
        move || {
            tries.set(tries.get() + 1);
            if tries.get() == 1 {
                Err("can't look: no network".to_owned())
            } else {
                Ok(Arc::new(FakeTarget::default()) as Arc<dyn Target>)
            }
        }
    };
    cx.update(|_, cx| {
        cx.set_global(Casting::new(vec![Protocol::new(KIND, "fakes", start)]));
        Casting::watch(cx);
    });
    assert_eq!(
        looking(&mut cx),
        [Looking::Failed("can't look: no network".into())]
    );
    cx.update(|_, cx| Casting::look_as_set(cx));
    assert_eq!(tries.get(), 1, "it isn't tried again while on");

    look_with(&mut cx, KIND, false);
    look_with(&mut cx, KIND, true);
    assert_eq!(tries.get(), 2);
    assert_eq!(looking(&mut cx), [Looking::On]);
}

#[gpui::test]
fn about_credits_what_plays_is_built_with_each_opening_to_its_license(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-About");
    let credits = [
        "credit-plays",
        "credit-library FFmpeg",
        "credit-library dav1d",
        "credit-library GPUI",
    ];
    for credit in credits {
        assert!(shows(&mut settings, credit), "{credit}");
    }
    assert!(!shows(&mut settings, "credit-texts"));
    let top = |cx: &mut VisualTestContext, selector| testing::find(cx, selector).origin.y;

    // GPUI's license opens under it.
    click(&mut settings, "credit-library GPUI");
    assert!(top(&mut settings, "credit-texts") > top(&mut settings, "credit-library GPUI"));
    // Plays' own in its place, one at a time.
    click(&mut settings, "credit-plays");
    let texts = top(&mut settings, "credit-texts");
    assert!(texts > top(&mut settings, "credit-plays"));
    assert!(texts < top(&mut settings, "credit-library FFmpeg"));
    click(&mut settings, "credit-plays");
    assert!(
        !shows(&mut settings, "credit-texts"),
        "and closes, GPUI's too"
    );

    // Closed on leaving the pane.
    click(&mut settings, "credit-library dav1d");
    assert!(shows(&mut settings, "credit-texts"));
    click(&mut settings, "pane-Playback");
    click(&mut settings, "pane-About");
    assert!(
        !shows(&mut settings, "credit-texts"),
        "closed on coming back"
    );
}

#[gpui::test]
fn hotkeys_lists_every_command_that_takes_keys_and_records_any_key(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Hotkeys");
    // Tall enough to show them all.
    let at = bounds_of(&mut settings).origin;
    drag_to(&mut settings, Bounds::new(at, size(px(900.), px(12_000.))));
    let add = |command: &str| -> &'static str {
        Box::leak(format!("hotkey-add-{command}").into_boxed_str())
    };
    let keys = |command: &'static str, cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<Settings>().hotkeys.keys(command).to_vec())
    };
    for spec in crate::hotkeys::commands() {
        assert!(shows(&mut settings, add(spec.id)), "{} is listed", spec.id);
    }
    assert!(!shows(&mut settings, add("window.focus")), "it takes none");

    // A key for one that works anywhere.
    let cmd_shift_k = crate::hotkeys::normalize("secondary-shift-k").unwrap();
    click(&mut settings, add("window.open_files"));
    press(&mut settings, "secondary-shift-k");
    assert!(keys("window.open_files", &mut settings).contains(&cmd_shift_k));
    // While it waits for one, a key that works anywhere is recorded
    // rather than run (asking for files would fail here), and taken from
    // the command that had it.
    let cmd_o = crate::hotkeys::normalize("secondary-o").unwrap();
    click(&mut settings, add("app.settings"));
    press(&mut settings, "secondary-o");
    assert!(keys("app.settings", &mut settings).contains(&cmd_o));
    assert!(!keys("window.open_files", &mut settings).contains(&cmd_o));
    let note = format!("{} was for “Open…”", crate::hotkeys::display(&cmd_o));
    assert!(
        testing::painted(&mut settings)
            .iter()
            .any(|(text, _)| *text == note),
        "{note}"
    );

    let remove: &'static str =
        Box::leak(format!("hotkey-remove-window.open_files-{cmd_shift_k}").into_boxed_str());
    click(&mut settings, remove);
    assert!(!keys("window.open_files", &mut settings).contains(&cmd_shift_k));
}
