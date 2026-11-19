//! The Settings window, and the settings the menus change, driven as
//! someone at the screen would (see `ui::testing`).

use gpui::{TestAppContext, VisualTestContext};

use super::SettingsView;
use crate::app::SetAudioDevice;
use crate::settings::{AudioDevice, Settings};
use crate::ui::testing::{self, click};

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
