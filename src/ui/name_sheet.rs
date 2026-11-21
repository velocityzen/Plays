//! Naming something: a modal sheet over the window with one field, holding
//! the name it has, if any, to type over. Return names it, a blank name
//! taking the name away; Escape, Cancel or a click outside leaves it as it
//! was. What it names says whether the name will do, the sheet showing why
//! not and staying up.

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, KeyBinding, MouseButton, SharedString,
    Subscription, Window, actions, black, div,
};

use super::components::{Button, Panel, Sizable};
use super::text_input::TextInput;
use super::theme::text::TextStyle;
use super::theme::{Theme, dialog, radius};

actions!(name_sheet, [Confirm, Cancel]);

const CONTEXT: &str = "NameSheet";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
    ]);
}

#[derive(Clone, Debug)]
pub enum NameEvent {
    /// The name typed, as it is: blank for none.
    Name(String),
    Dismiss,
}

pub struct NameSheet {
    /// What tests find it by, and its error by, with `-error`.
    id: &'static str,
    title: SharedString,
    input: Entity<TextInput>,
    error: Option<SharedString>,
    _input_changed: Subscription,
}

impl EventEmitter<NameEvent> for NameSheet {}

impl NameSheet {
    /// A sheet titled `title`, its field holding `name`, all selected.
    pub fn new(
        id: &'static str,
        title: impl Into<SharedString>,
        name: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("No name", cx).large();
            input.set_text(name.unwrap_or_default().to_owned(), cx);
            input
        });
        window.focus(&input.focus_handle(cx));
        let input_changed = cx.observe(&input, |this, _, cx| {
            if this.error.take().is_some() {
                cx.notify();
            }
        });
        Self {
            id,
            title: title.into(),
            input,
            error: None,
            _input_changed: input_changed,
        }
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.input.focus_handle(cx));
    }

    /// Why the name typed won't do; typing takes it away.
    pub fn refuse(&mut self, why: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(why.into());
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        cx.emit(NameEvent::Name(self.input.read(cx).text().to_owned()));
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.submit(cx);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(NameEvent::Dismiss);
    }
}

impl Render for NameSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let id = self.id;

        // It takes the mouse, so clicks inside it don't dismiss it.
        let card = Panel::new()
            .occlude()
            .superellipse(radius::DIALOG)
            .w(dialog::WIDTH)
            .max_w_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(div().text_title().child(self.title.clone()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(self.input.clone())
                    .when_some(self.error.clone(), |this, error| {
                        this.child(
                            div()
                                .debug_selector(move || format!("{id}-error"))
                                .text_xs()
                                .text_color(theme.danger)
                                .child(error),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("name-cancel", "Cancel")
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(NameEvent::Dismiss))),
                    )
                    .child(
                        Button::new("name-confirm", "Rename")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            );

        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            // Otherwise the card stretches to the bottom of the window.
            .items_start()
            .pt(dialog::TOP)
            .px_4()
            .bg(black().opacity(dialog::BACKDROP))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(NameEvent::Dismiss)),
            )
            .child(card)
    }
}
