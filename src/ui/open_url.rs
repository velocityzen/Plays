//! The "Open URL" dialog: a modal sheet over the main window with one field.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding, MouseButton,
    SharedString, Subscription, Window, actions, div, hsla, px,
};

use super::text_input::TextInput;
use super::theme::Theme;
use super::widgets::{button, icon};
use crate::media::Source;

actions!(open_url, [Confirm, Cancel]);

const CONTEXT: &str = "OpenUrl";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
    ]);
}

pub enum OpenUrlEvent {
    Open(Source),
    Dismiss,
}

pub struct OpenUrlDialog {
    input: Entity<TextInput>,
    error: Option<SharedString>,
    _input_changed: Subscription,
}

impl EventEmitter<OpenUrlEvent> for OpenUrlDialog {}

impl OpenUrlDialog {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("https://example.com/live/stream.m3u8", cx));
        // Start with the clipboard when it holds a stream URL, ready to confirm.
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
            && let Ok(Source::Url(url)) = Source::parse_url(&text)
        {
            input.update(cx, |input, cx| input.set_text(url, cx));
        }
        window.focus(&input.focus_handle(cx));
        let input_changed = cx.observe(&input, |this, _, cx| {
            if this.error.take().is_some() {
                cx.notify();
            }
        });
        Self {
            input,
            error: None,
            _input_changed: input_changed,
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &App) {
        window.focus(&self.input.focus_handle(cx));
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        match Source::parse_url(self.input.read(cx).text()) {
            Ok(source) => cx.emit(OpenUrlEvent::Open(source)),
            Err(message) => {
                self.error = Some(message.into());
                cx.notify();
            }
        }
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(OpenUrlEvent::Dismiss);
    }
}

impl Focusable for OpenUrlDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for OpenUrlDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let accent = theme.accent;

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
                    .child(icon("link").size(px(16.)).text_color(theme.accent))
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
            .child(self.input.clone())
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_xs().text_color(theme.danger).child(error))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(button("open-url-cancel", None, "Cancel", &theme).on_click(
                        cx.listener(|this, _, window, cx| this.cancel(&Cancel, window, cx)),
                    ))
                    .child(
                        button("open-url-open", None, "Open", &theme)
                            .bg(accent)
                            .border_color(accent)
                            .text_color(gpui::white())
                            .hover(move |style| style.bg(accent.opacity(0.85)))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm(&Confirm, window, cx)
                            })),
                    ),
            );

        div()
            .id("open-url")
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
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
