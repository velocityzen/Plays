//! Audio: the volume of everything, the output it plays through, whether
//! a window sent to a display plays through its speakers, and the
//! languages to play files' audio in.

use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Corner, SharedString, Window, div, point};

use super::form::{group, percent_slider, row, set};
use super::languages::{List, languages_group};
use super::{Pane, SettingsView};
use crate::api::commands::output;
use crate::app::{AudioEngine, Route};
use crate::settings::{AudioDevice, Settings};
use crate::ui::components::{Button, MenuRow, Panel, Popover, Switch};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::{
    AUDIO_COLOR, DEVICE_BUTTON_WIDTH, DEVICE_MENU_MAX_HEIGHT, DEVICE_MENU_TOP, DEVICE_MENU_WIDTH,
    OUTPUT_MAX_WIDTH,
};

pub(super) struct AudioPane;

impl Pane for AudioPane {
    fn name(&self) -> &'static str {
        "Audio"
    }

    fn icon(&self) -> &'static str {
        "volume"
    }

    fn color(&self) -> u32 {
        AUDIO_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let output = &s.audio_output;
        let mut rows = vec![
            row(
                "Master volume",
                None,
                theme,
                percent_slider(
                    view,
                    "master-volume",
                    "master_volume",
                    s.master_volume,
                    theme,
                ),
            ),
            row("Output", None, theme, device_picker(view, s, theme, cx)),
        ];
        if let Some(device) = &output.device {
            rows.push(row(
                "Wait for it",
                Some(
                    format!(
                        "While {} isn't connected, play nothing rather than through the \
                         system's output.",
                        device.name
                    )
                    .into(),
                ),
                theme,
                Switch::new("wait-for-device", output.wait_for_device)
                    .on_toggle(|&on, _, cx| set("audio_output.wait_for_device", on, cx)),
            ));
        }
        let standing_in = match AudioEngine::route(cx) {
            Some(Route::Device {
                missing: Some(name),
                ..
            }) => Some(format!("Until {name} is connected.").into()),
            _ => None,
        };
        rows.push(row(
            "Playing through",
            standing_in,
            theme,
            div()
                .text_sm()
                .text_color(theme.text_muted)
                .max_w(OUTPUT_MAX_WIDTH)
                .truncate()
                .child(AudioEngine::device_description(cx)),
        ));
        rows.push(row(
            "Play on the display's speakers",
            Some(
                "A window Output Device puts full screen on another display, such as a \
                 TV, plays through its speakers until it comes back, unless it has an \
                 output of its own."
                    .into(),
            ),
            theme,
            Switch::new("sound-follows-display", s.sound_follows_display)
                .on_toggle(|&on, _, cx| set("sound_follows_display", on, cx)),
        ));
        vec![
            group(None, theme, rows),
            languages_group(view, List::Audio, s, theme, cx),
        ]
    }

    fn leave(&self, view: &mut SettingsView) {
        view.language_menu = None;
    }
}

/// The button that shows the output picked and opens the menu of them.
fn device_picker(
    view: &SettingsView,
    s: &Settings,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let name = s.audio_output.device.as_ref().map_or_else(
        || SharedString::from("System default"),
        |d| d.name.clone().into(),
    );
    Popover::new(
        "devices-backdrop",
        Button::new("audio-device", name)
            .icon_right("chevron-up-down")
            .w(DEVICE_BUTTON_WIDTH)
            .on_click(cx.listener(open_devices)),
    )
    .anchor(
        Corner::TopRight,
        point(DEVICE_BUTTON_WIDTH, DEVICE_MENU_TOP),
    )
    .when(view.device_menu, |this| {
        this.menu(device_menu(s, theme, cx))
    })
    .on_dismiss(cx.listener(|view, _, _, cx| {
        view.device_menu = false;
        cx.notify();
    }))
    .into_any_element()
}

/// "System default", every output connected, and the one picked if it
/// isn't.
fn device_menu(s: &Settings, theme: &Theme, cx: &mut Context<SettingsView>) -> impl IntoElement {
    let picked = s.audio_output.device.clone();
    let mut choices: Vec<(Option<AudioDevice>, Option<&'static str>)> = vec![(None, None)];
    choices.extend(AudioEngine::devices(cx).iter().map(|device| {
        let device = AudioDevice {
            id: device.id.clone(),
            name: device.name.clone(),
        };
        (Some(device), None)
    }));
    if let Some(device) = &picked
        && !choices
            .iter()
            .any(|(choice, _)| choice.as_ref().is_some_and(|c| c.id == device.id))
    {
        choices.push((Some(device.clone()), Some("Not connected")));
    }
    let rows = choices
        .into_iter()
        .enumerate()
        .map(|(ix, (device, detail))| {
            let checked = device.as_ref().map(|d| &d.id) == picked.as_ref().map(|d| &d.id);
            let label = device.as_ref().map_or_else(
                || SharedString::from("System default"),
                |d| d.name.clone().into(),
            );
            MenuRow::new(ix, label)
                .checked(checked)
                .when_some(detail, |this, detail| {
                    this.detail(div().text_color(theme.text_muted).child(detail))
                })
                .mx_1()
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.device_menu = false;
                    // As the Audio menu picks it.
                    window.dispatch_action(Box::new(output::set_device(device.as_ref())), cx);
                    cx.notify();
                }))
        })
        .collect::<Vec<_>>();
    Panel::new().occlude().w(DEVICE_MENU_WIDTH).child(
        div()
            .id("devices")
            .max_h(DEVICE_MENU_MAX_HEIGHT)
            .overflow_y_scroll()
            .py_1()
            .children(rows),
    )
}

fn open_devices(
    view: &mut SettingsView,
    _: &ClickEvent,
    _: &mut Window,
    cx: &mut Context<SettingsView>,
) {
    // Outputs come and go: list them again, and show them as they were
    // meanwhile.
    AudioEngine::list_devices(cx);
    view.device_menu = true;
    cx.notify();
}
