//! Opening a stream by its URL: a form with one field, which an empty window
//! shows in its ring, and a modal sheet with the form for windows already
//! playing.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, KeyBinding, MouseButton, SharedString,
    Subscription, Window, actions, div, hsla, px,
};

use super::components::{Button, Icon};
use super::text_input::TextInput;
use super::theme::Theme;
use crate::media::Source;

actions!(open_url, [Confirm, Cancel]);

const CONTEXT: &str = "OpenUrl";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
    ]);
}

#[derive(Clone)]
pub enum OpenUrlEvent {
    Open(Source),
    Dismiss,
}

/// The URL field with its Cancel and Open buttons. Return opens, Escape
/// cancels.
pub struct UrlForm {
    input: Entity<TextInput>,
    error: Option<SharedString>,
    centered: bool,
    _input_changed: Subscription,
}

impl EventEmitter<OpenUrlEvent> for UrlForm {}

impl UrlForm {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("https://example.com/live/stream.m3u8", cx));
        let input_changed = cx.observe(&input, |this, _, cx| {
            if this.error.take().is_some() {
                cx.notify();
            }
        });
        Self {
            input,
            error: None,
            centered: false,
            _input_changed: input_changed,
        }
    }

    /// The buttons go in the middle, rather than at the end like a dialog's.
    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }

    /// Focus the field. An empty one takes the clipboard when it holds a
    /// stream URL, ready to confirm.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).text().is_empty()
            && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            && let Ok(Source::Url(url)) = Source::parse_url(&text)
        {
            self.input.update(cx, |input, cx| input.set_text(url, cx));
        }
        window.focus(&self.input.focus_handle(cx));
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.error = None;
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        match Source::parse_url(self.input.read(cx).text()) {
            Ok(source) => cx.emit(OpenUrlEvent::Open(source)),
            Err(message) => {
                self.error = Some(message.into());
                cx.notify();
            }
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.submit(cx);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(OpenUrlEvent::Dismiss);
    }
}

impl Render for UrlForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .w_full()
            .flex()
            .flex_col()
            .gap_3()
            .text_left()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(self.input.clone())
                    .when_some(self.error.clone(), |this, error| {
                        this.child(div().text_xs().text_color(theme.danger).child(error))
                    }),
            )
            .child(
                div()
                    .flex()
                    .map(|this| {
                        if self.centered {
                            this.justify_center()
                        } else {
                            this.justify_end()
                        }
                    })
                    .gap_2()
                    .child(
                        Button::new("open-url-cancel", "Cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenUrlEvent::Dismiss))),
                    )
                    .child(
                        Button::new("open-url-open", "Open")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            )
    }
}

/// The form in a modal sheet, for windows that are already playing.
pub struct OpenUrlDialog {
    form: Entity<UrlForm>,
    _form_events: Subscription,
}

impl EventEmitter<OpenUrlEvent> for OpenUrlDialog {}

impl OpenUrlDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let form = cx.new(UrlForm::new);
        form.update(cx, |form, cx| form.focus(window, cx));
        let form_events = cx.subscribe(&form, |_, _, event: &OpenUrlEvent, cx| {
            cx.emit(event.clone());
        });
        Self {
            form,
            _form_events: form_events,
        }
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form.update(cx, |form, cx| form.focus(window, cx));
    }
}

impl Render for OpenUrlDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);

        let card = div()
            .id("open-url-card")
            .w(px(520.))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded(px(12.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .shadow_lg()
            // Clicks inside the card don't dismiss it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new("link").color(theme.accent))
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Open a stream"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Video or audio over HTTP(S), HLS and DASH playlists, RTMP or RTSP."),
            )
            .child(self.form.clone());

        div()
            .id("open-url")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            // Otherwise the card stretches to the bottom of the window.
            .items_start()
            .pt(px(96.))
            .px_4()
            .bg(hsla(0., 0., 0., 0.45))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(OpenUrlEvent::Dismiss)),
            )
            .child(card)
    }
}
