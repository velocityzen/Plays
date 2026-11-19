//! The Settings window, and the settings the menus change, driven as
//! someone at the screen would (see `ui::testing`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use gpui::{TestAppContext, VisualTestContext};

use super::SettingsView;
use crate::app::SetAudioDevice;
use crate::cast::fake::{FakeTarget, KIND};
use crate::cast::{Casting, Looking, Protocol, Shows, Target};
use crate::settings::{AudioDevice, Settings};
use crate::ui::testing::{self, click, shows};

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
fn picking_an_output_in_the_audio_menu_sets_it(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let output = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<Settings>().audio_output.device.clone())
    };
    let device = AudioDevice {
        id: "test-output".into(),
        name: "Test Output".into(),
    };

    cx.dispatch_action(SetAudioDevice(Some(device.clone())));
    cx.run_until_parked();
    assert_eq!(output(&mut cx), Some(device));

    cx.dispatch_action(SetAudioDevice(None));
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
fn casting_lists_the_devices_found_and_a_protocol_turned_off_finds_none(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let fake = FakeTarget::default();
    fake.add("Den TV", Shows::Video);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Casting");
    assert!(shows(&mut settings, "found-Den TV"));

    fake.add("Kitchen", Shows::Sound);
    settings.update(|_, cx| Casting::refresh(cx));
    settings.run_until_parked();
    assert!(
        // Drawn again by itself, as nothing else changed.
        settings.debug_bounds("found-Kitchen").is_some(),
        "a device shows as it's found"
    );

    click(&mut settings, "look-with-Fake");
    let set = settings.update(|_, cx| cx.global::<Settings>().casting.clone());
    assert!(!set.looks_with(KIND), "it's saved off");
    assert_eq!(looking(&mut settings), [Looking::Off]);
    assert!(found(&mut settings).is_empty());
    assert!(!shows(&mut settings, "found-Den TV"), "its devices go");

    click(&mut settings, "look-with-Fake");
    assert_eq!(looking(&mut settings), [Looking::On]);
    assert_eq!(found(&mut settings), ["Den TV", "Kitchen"]);
    assert!(shows(&mut settings, "found-Den TV"));
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
    assert!(
        last.borrow().upgrade().is_none(),
        "turned off, it stops looking"
    );
    assert!(found(&mut cx).is_empty());
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
