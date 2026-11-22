//! Pairing with a TV or speaker that asks for the PIN it shows, as an
//! AirPlay receiver does the first time: a modal sheet over the window
//! whose tile was cast to it, or in front as it's played through. It shows
//! a PIN once the sheet opens; a wrong one has it show another. Each step
//! waits on the network, so pairing goes on on a thread of its own, which
//! the sheet looks at as it goes.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, EventEmitter, Focusable, KeyBinding, MouseButton, SharedString, Task,
    Window, actions, black, div,
};
use parking_lot::Mutex;

use super::components::{Button, Icon, Panel, Sizable};
use super::text_input::TextInput;
use super::theme::motion::PAIRING_POLL;
use super::theme::text::TextStyle;
use super::theme::{Theme, dialog, radius};
use crate::cast::Error;
use crate::cast::target::Pairing;

actions!(pairing, [Confirm, Cancel]);

const CONTEXT: &str = "Pairing";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Confirm, Some(CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(CONTEXT)),
    ]);
}

/// Starts pairing, having the device show a PIN; waits on the network.
pub type StartPairing = Arc<dyn Fn() -> Result<Box<dyn Pairing>, Error> + Send + Sync>;

#[derive(Clone, Debug)]
pub enum PairingEvent {
    /// Paired: what wanted it goes on.
    Paired,
    Dismiss,
}

/// How pairing goes, on its thread.
#[derive(Clone, PartialEq, Debug)]
enum Step {
    /// Asking the TV to show a PIN.
    Asking,
    /// It shows one, to be typed; `wrong` is why the last didn't pair.
    Showing {
        wrong: Option<String>,
    },
    /// Checking the PIN typed with the TV.
    Checking,
    Paired,
    /// It can't be paired with, for this reason.
    Failed(String),
}

pub struct PairingDialog {
    /// What it pairs with is called.
    name: String,
    input: Entity<TextInput>,
    step: Step,
    /// What the pairing thread last said.
    going: Arc<Mutex<Step>>,
    /// The PINs typed, to the pairing thread, which stops once the sheet
    /// lets go of it.
    pins: Sender<String>,
    /// The field has been focused for the PIN it shows.
    focused: bool,
    _follow: Task<()>,
}

impl EventEmitter<PairingEvent> for PairingDialog {}

impl PairingDialog {
    /// Pair with what's called `name`, starting with `start`.
    pub fn new(name: String, start: StartPairing, cx: &mut Context<Self>) -> Self {
        let (pins, typed) = mpsc::channel();
        let going = Arc::new(Mutex::new(Step::Asking));
        let spawned = {
            let name = name.clone();
            let going = Arc::clone(&going);
            thread::Builder::new()
                .name(format!("pair with {name}"))
                .spawn(move || pair(&*start, &name, &typed, &going))
        };
        if let Err(err) = spawned {
            *going.lock() = Step::Failed(err.to_string());
        }
        let follow = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PAIRING_POLL).await;
                let going = this.update(cx, |this, cx| this.follow(cx));
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        });
        Self {
            name,
            input: cx.new(|cx| TextInput::new("PIN", cx).large()),
            step: Step::Asking,
            going,
            pins,
            focused: false,
            _follow: follow,
        }
    }

    /// What it says now.
    #[cfg(test)]
    pub fn says(&self) -> String {
        self.message().0.to_string()
    }

    /// The TV shows a PIN, and the sheet waits for it to be typed.
    #[cfg(test)]
    pub fn wants_pin(&self) -> bool {
        matches!(self.step, Step::Showing { .. })
    }

    /// Show how pairing goes, and say so once it's paired. Whether it goes
    /// on.
    fn follow(&mut self, cx: &mut Context<Self>) -> bool {
        let step = self.going.lock().clone();
        if step != self.step {
            if matches!(step, Step::Showing { .. }) {
                // A PIN for the new one it shows.
                self.input.update(cx, |input, cx| input.set_text("", cx));
                self.focused = false;
            }
            self.step = step;
            cx.notify();
        }
        match &self.step {
            Step::Paired => {
                cx.emit(PairingEvent::Paired);
                false
            }
            Step::Failed(_) => false,
            Step::Asking | Step::Showing { .. } | Step::Checking => true,
        }
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.step, Step::Showing { .. }) {
            return;
        }
        let pin = self.input.read(cx).text().trim().to_owned();
        if pin.is_empty() {
            return;
        }
        *self.going.lock() = Step::Checking;
        self.step = Step::Checking;
        let _ = self.pins.send(pin);
        cx.notify();
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.submit(cx);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PairingEvent::Dismiss);
    }

    /// What the sheet says, and whether it's a failure.
    fn message(&self) -> (SharedString, bool) {
        let name = &self.name;
        match &self.step {
            Step::Asking => (format!("Asking {name} for a PIN…").into(), false),
            Step::Showing { wrong: None } => (format!("Enter the PIN {name} shows.").into(), false),
            Step::Showing { wrong: Some(why) } => (
                format!("{}. Enter the new PIN {name} shows.", sentence(why)).into(),
                true,
            ),
            Step::Checking | Step::Paired => (format!("Pairing with {name}…").into(), false),
            Step::Failed(why) => (format!("Can't pair with {name}: {why}").into(), true),
        }
    }
}

/// Pair on the pairing thread with what's called `name`: have it show a
/// PIN (`start`), and check what's typed with it, until one pairs, it
/// can't be reached, or the sheet lets go.
fn pair(
    start: &(dyn Fn() -> Result<Box<dyn Pairing>, Error> + Send + Sync),
    name: &str,
    typed: &Receiver<String>,
    going: &Mutex<Step>,
) {
    let mut wrong = None;
    loop {
        let pairing = match start() {
            Ok(pairing) => pairing,
            Err(err) => {
                *going.lock() = Step::Failed(err.to_string());
                return;
            }
        };
        *going.lock() = Step::Showing {
            wrong: wrong.take(),
        };
        let Ok(pin) = typed.recv() else {
            return;
        };
        match pairing.finish(&pin) {
            Ok(()) => {
                *going.lock() = Step::Paired;
                return;
            }
            Err(err @ Error::Gone) => {
                *going.lock() = Step::Failed(err.to_string());
                return;
            }
            Err(err) => {
                log::info!("pairing with {name}: {err}");
                wrong = Some(err.to_string());
                *going.lock() = Step::Asking;
            }
        }
    }
}

/// `why`, begun as a sentence.
fn sentence(why: &str) -> String {
    let mut chars = why.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

impl Render for PairingDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let showing = matches!(self.step, Step::Showing { .. });
        if showing && !self.focused {
            self.focused = true;
            window.focus(&self.input.focus_handle(cx));
        }
        let (says, failure) = self.message();
        let failed = matches!(self.step, Step::Failed(_));
        let field = matches!(self.step, Step::Showing { .. } | Step::Checking);

        let buttons = div()
            .flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("pairing-cancel", if failed { "Close" } else { "Cancel" })
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PairingEvent::Dismiss))),
            )
            .when(!failed, |this| {
                this.child(
                    Button::new("pairing-pair", "Pair")
                        .primary()
                        .disabled(!showing)
                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                )
            });

        // It takes the mouse, so clicks inside it don't dismiss it.
        let body = div()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new("cast").color(theme.accent))
                    .child(div().text_title().child(format!("Pair with {}", self.name))),
            )
            .child(
                div()
                    .debug_selector(|| "pairing-says".into())
                    .text_sm()
                    .text_color(if failure {
                        theme.danger
                    } else {
                        theme.text_muted
                    })
                    .child(says),
            )
            .when(field, |this| this.child(self.input.clone()))
            .child(buttons);
        let card = Panel::new()
            .occlude()
            .superellipse(radius::DIALOG)
            .w(dialog::WIDTH)
            .max_w_full()
            .p_4()
            .child(body);

        div()
            .id("pairing")
            .debug_selector(|| "pairing".into())
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
                cx.listener(|_, _, _, cx| cx.emit(PairingEvent::Dismiss)),
            )
            .child(card)
    }
}

#[cfg(test)]
#[path = "tests/pairing.rs"]
mod tests;
