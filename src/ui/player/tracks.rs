//! Picking tracks: the menu of them from the controls, and the rows it
//! shares with the track reel.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, Hsla, MouseDownEvent, Window, div, white};

use super::PlayerView;
use crate::media::decoder::MediaInfo;
use crate::media::tracks::TrackKind;
use crate::ui::components::Panel;
use crate::ui::theme::player::{
    TRACK_CHECK_WIDTH, TRACK_DETAIL_HOVER, TRACK_MENU_MAX_HEIGHT, TRACK_MENU_WIDTH,
};
use crate::ui::theme::{Theme, menu, radius};

/// A track to pick, in the menu or on the reel.
pub(super) struct TrackChoice {
    /// `None` for no subtitles.
    pub(super) index: Option<usize>,
    label: String,
    /// Its codec and format.
    detail: Option<String>,
}

fn track_heading(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "Video",
        TrackKind::Audio => "Audio",
        TrackKind::Subtitle => "Subtitles",
    }
}

/// A kind's tracks, and for subtitles "Off" before them.
pub(super) fn track_choices(info: &MediaInfo, kind: TrackKind) -> Vec<TrackChoice> {
    let off = (kind == TrackKind::Subtitle).then(|| TrackChoice {
        index: None,
        label: "Off".to_owned(),
        detail: None,
    });
    off.into_iter()
        .chain(info.tracks(kind).enumerate().map(|(i, track)| TrackChoice {
            index: Some(track.index),
            label: track.label(i + 1),
            detail: Some(track.detail()),
        }))
        .collect()
}

/// Whether the file has tracks to pick from: subtitles, or more than one
/// video or audio track.
pub(super) fn has_track_choice(info: &MediaInfo) -> bool {
    info.tracks(TrackKind::Subtitle).next().is_some()
        || info.tracks(TrackKind::Audio).count() > 1
        || info.tracks(TrackKind::Video).count() > 1
}

impl PlayerView {
    /// The menu of tracks: a column each for audio and subtitles (when the
    /// file has any) and video (when it has more than one), each scrolling
    /// on its own, so a long list of subtitles leaves audio in view.
    pub(super) fn render_tracks_menu(
        &self,
        info: &MediaInfo,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut menu = Panel::new().occlude().flex();
        let mut first = true;
        for kind in [TrackKind::Video, TrackKind::Audio, TrackKind::Subtitle] {
            let heading = track_heading(kind);
            let choices = track_choices(info, kind);
            let tracks = choices.iter().filter(|c| c.index.is_some()).count();
            let listed = match kind {
                TrackKind::Video => tracks > 1,
                _ => tracks > 0,
            };
            if !listed {
                continue;
            }
            let selected = info.selected(kind);
            let mut list = div()
                .id(heading)
                .max_h(TRACK_MENU_MAX_HEIGHT)
                .overflow_y_scroll()
                .pb_1();
            for choice in &choices {
                let index = choice.index;
                list = list.child(
                    track_row(choice, selected == index, theme.text_muted)
                        .id(("track", index.map_or(0, |i| i + 1)))
                        .group("track-row")
                        .mx_1()
                        .cursor_pointer()
                        .hover(|style| style.bg(theme.accent).text_color(white()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.player.select_track(kind, index);
                            this.tracks_open = false;
                            cx.notify();
                        })),
                );
            }
            menu = menu.child(
                div()
                    .flex()
                    .flex_col()
                    .w(TRACK_MENU_WIDTH)
                    .when(!first, |this| this.border_l_1().border_color(theme.border))
                    .child(track_heading_row(heading, theme))
                    .child(list),
            );
            first = false;
        }
        menu.into_any_element()
    }
}

/// Clicks outside the menu close it, and do nothing else.
pub(super) fn close_tracks(
    view: &mut PlayerView,
    _: &MouseDownEvent,
    _: &mut Window,
    cx: &mut Context<PlayerView>,
) {
    view.tracks_open = false;
    cx.stop_propagation();
    cx.notify();
}

/// A track's row: a check if it's the one picked, its name, and what it is
/// (in `muted`).
pub(super) fn track_row(choice: &TrackChoice, checked: bool, muted: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .h(menu::ROW_HEIGHT)
        .px_2()
        .superellipse(radius::ITEM)
        // A character rather than an icon, so it turns white with the rest
        // of the row in the menu.
        .child(
            div()
                .flex_none()
                .w(TRACK_CHECK_WIDTH)
                .when(checked, |this| this.child("✓")),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(choice.label.clone()),
        )
        .when_some(choice.detail.clone(), |this, detail| {
            this.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .group_hover("track-row", |style| {
                        style.text_color(white().opacity(TRACK_DETAIL_HOVER))
                    })
                    .child(detail),
            )
        })
}

fn track_heading_row(heading: &'static str, theme: &Theme) -> Div {
    div()
        .px_3()
        .pt_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text_muted)
        .child(heading)
}
