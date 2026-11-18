//! The settings file, at the foot of the sidebar: where it is, and a menu
//! to show it in its folder, save a copy of it somewhere else, or take the
//! settings from such a copy.

use std::fs;
use std::path::Path;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Corner, PathPromptOptions, PromptLevel, Window, div, point, px, rgb,
};

use super::SettingsView;
use super::form::update_settings;
use crate::settings::Settings;
use crate::ui::components::{Badge, Icon, MenuRow, Panel, Popover};
use crate::ui::theme::settings::{
    FILE_CHEVRON, FILE_COLOR, FILE_MENU_GAP, FILE_MENU_WIDTH, SIDEBAR_ROW_HEIGHT,
};
use crate::ui::theme::{Theme, radius};

/// What a copy is called unless it is named otherwise.
const EXPORT_NAME: &str = "Plays Settings.json";

/// The sidebar's row for the settings file, which opens its menu.
pub(super) fn file_menu(
    view: &SettingsView,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let open = view.file_menu;
    let row = div()
        .id("settings-file")
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .h(SIDEBAR_ROW_HEIGHT)
        .px_1p5()
        .superellipse(radius::FRAME)
        .text_sm()
        .map(|this| {
            if open {
                this.bg(theme.surface_hover)
            } else {
                this.hover(|style| style.bg(theme.surface_hover))
            }
        })
        .on_click(cx.listener(|view, _, _, cx| {
            view.file_menu = true;
            cx.notify();
        }))
        .child(Badge::icon("file", rgb(FILE_COLOR)))
        .child(div().flex_1().min_w_0().truncate().child("Settings File"))
        .child(
            Icon::new("chevron-up-down")
                .size(FILE_CHEVRON)
                .color(theme.text_muted),
        );
    Popover::new("settings-file-backdrop", row)
        .anchor(Corner::BottomLeft, point(px(0.), -FILE_MENU_GAP))
        .when(open, |this| this.menu(menu(theme, cx)))
        .on_dismiss(cx.listener(|view, _, _, cx| {
            view.file_menu = false;
            cx.notify();
        }))
        .into_any_element()
}

/// Where the file is, and what can be done with it.
fn menu(theme: &Theme, cx: &mut Context<SettingsView>) -> impl IntoElement {
    let path = Settings::path();
    let location = path.as_ref().map_or_else(
        || "Settings can't be saved on this system".to_string(),
        |path| path.display().to_string(),
    );
    Panel::new().occlude().w(FILE_MENU_WIDTH).child(
        div()
            .py_1()
            .child(
                div()
                    .px_3()
                    .pt_1()
                    .pb_1p5()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(location),
            )
            .when_some(path, |this, path| {
                this.child(
                    MenuRow::new("settings-file-show", "Open Folder")
                        .mx_1()
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.file_menu = false;
                            show(&path, cx);
                            cx.notify();
                        })),
                )
            })
            .child(
                MenuRow::new("settings-file-export", "Export…")
                    .mx_1()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.file_menu = false;
                        export(window, cx);
                        cx.notify();
                    })),
            )
            .child(
                MenuRow::new("settings-file-import", "Import…")
                    .mx_1()
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.file_menu = false;
                        import(window, cx);
                        cx.notify();
                    })),
            ),
    )
}

/// Show the file in its folder, writing it first if nothing has been
/// changed yet to write it.
fn show(path: &Path, cx: &mut App) {
    if !path.exists() {
        cx.global::<Settings>().save();
    }
    cx.reveal_path(path);
}

/// Save a copy of the settings where the system's dialog says.
fn export(window: &mut Window, cx: &mut Context<SettingsView>) {
    let folder = dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default();
    let picked = cx.prompt_for_new_path(&folder, Some(EXPORT_NAME));
    let json = cx.global::<Settings>().to_json();
    cx.spawn_in(window, async move |_, cx| {
        let path = match picked.await {
            Ok(Ok(Some(path))) => path,
            Ok(Err(err)) => {
                log::warn!("no dialog to export the settings with: {err:#}");
                return;
            }
            _ => return,
        };
        let written = cx
            .background_spawn(async move { fs::write(&path, json) })
            .await;
        if let Err(err) = written {
            cx.update(|window, cx| {
                warn("Couldn't export the settings", &err.to_string(), window, cx)
            })
            .ok();
        }
    })
    .detach();
}

/// Take the settings from a file the system's dialog picks, and save them.
fn import(window: &mut Window, cx: &mut Context<SettingsView>) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Import".into()),
    });
    cx.spawn_in(window, async move |_, cx| {
        let path = match picked.await {
            Ok(Ok(Some(paths))) => paths.into_iter().next(),
            Ok(Err(err)) => {
                log::warn!("no dialog to import the settings with: {err:#}");
                None
            }
            _ => None,
        };
        let Some(path) = path else {
            return;
        };
        let read = cx
            .background_spawn(async move {
                let text = fs::read_to_string(&path)?;
                Settings::import(&text)
            })
            .await;
        cx.update(|window, cx| match read {
            Ok(settings) => update_settings(cx, true, |s| *s = settings),
            Err(err) => warn(
                "Couldn't import the settings",
                &format!("{err:#}"),
                window,
                cx,
            ),
        })
        .ok();
    })
    .detach();
}

fn warn(message: &str, detail: &str, window: &mut Window, cx: &mut App) {
    // Nothing to do with the answer: the one button closes it.
    drop(window.prompt(PromptLevel::Warning, message, Some(detail), &["OK"], cx));
}
