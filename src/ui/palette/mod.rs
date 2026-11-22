//! The command palette: a sheet over a player window listing every
//! command that can run there, which ⇧⌘P (`window.command_palette`)
//! opens. Typing finds commands by their titles, groups and keywords
//! (`search`), those run from it lately first (`recents`); each row shows
//! a command's keys, and what it changes as it is (✓ Loop). Return runs
//! the one picked, through the API as from the palette, asking first for
//! the arguments it takes (`asking`), picked from a list or typed. ⌘Return,
//! or Change Shortcut… on the row, changes its keys in Settings → Hotkeys.
//! Escape, or a click outside, closes it.
//!
//! Go to… (⌘P, `window.go_to`) is the same sheet listing places instead
//! (`places`): the windows, their tiles, the files in the front tile's
//! folder and those played lately, found as commands are; Return goes
//! there, running the command each has.

mod asking;
#[cfg(test)]
mod go_to_tests;
mod places;
mod search;
#[cfg(test)]
mod tests;

use std::cmp::Reverse;
use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Div, Entity, EventEmitter, Focusable, FontWeight, HighlightStyle,
    Hsla, KeyBinding, MouseButton, ScrollStrategy, SharedString, StyledText, Subscription,
    UniformListScrollHandle, Window, actions, black, div, uniform_list, white,
};
use serde_json::{Map, Value};

use super::components::{Button, Panel, Sizable};
use super::text_input::TextInput;
use super::theme::palette::{
    CHECK_WIDTH, DETAIL_SELECTED, GROUP_WIDTH, ROW_HEIGHT, ROWS, WIDTH as PALETTE_WIDTH,
};
use super::theme::{Theme, dialog, radius};
use crate::api::commands::NAMESPACES;
use crate::api::{self, Invocation, Scope, Source, Spec, Target};
use crate::hotkeys;
use crate::recents;
use crate::settings::Settings;
use asking::{Ask, Choice, Question};
use places::Place;
use search::{Found, Haystack};

pub use places::places;

actions!(
    palette,
    [SelectPrevious, SelectNext, Confirm, Cancel, ChangeShortcut]
);

const CONTEXT: &str = "CommandPalette";

/// The command that opens it, which it doesn't list.
pub const COMMAND: &str = "window.command_palette";

/// The command that opens Go to….
pub const GO_TO: &str = "window.go_to";

/// What a command run lately counts for more, the latest most, as it
/// goes before others found as well.
const RECENT: i32 = 16;
const RECENT_STEP: i32 = 2;

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("escape", Cancel, context),
        KeyBinding::new("secondary-enter", ChangeShortcut, context),
    ]);
}

/// A command it lists, as it was when the palette opened.
#[derive(Clone)]
pub struct Entry {
    pub spec: &'static Spec,
    /// What its namespace is called: Playback, Audio.
    pub group: &'static str,
    /// Its keys, as the menus show them.
    pub keys: Vec<SharedString>,
    pub state: State,
}

impl Entry {
    fn haystack(&self) -> Haystack<'_> {
        Haystack {
            title: self.spec.title,
            group: self.group,
            keywords: self.spec.keywords,
        }
    }
}

/// What a command changes, as it is, shown with it.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum State {
    /// Nothing it can say.
    #[default]
    Nothing,
    /// It's on, as Loop can be: a check.
    On,
    /// What it's set to: an aspect ratio, an output.
    Is(SharedString),
}

impl State {
    fn of(value: &Value) -> Self {
        match value {
            Value::Bool(true) => Self::On,
            Value::String(text) if !text.is_empty() => Self::Is(text.clone().into()),
            Value::Number(number) => Self::Is(number.to_string().into()),
            Value::Object(fields) => fields
                .get("name")
                .and_then(Value::as_str)
                .map_or(Self::Nothing, |name| Self::Is(name.to_owned().into())),
            _ => Self::Nothing,
        }
    }
}

/// `window`, for a command that acts on a window or what's in it;
/// nothing, for the app's.
fn aimed(spec: &Spec, window: &Target) -> Target {
    if spec.scope == Scope::App {
        Target::default()
    } else {
        window.clone()
    }
}

/// What a namespace is called.
fn group(spec: &Spec) -> &'static str {
    let namespace = spec.namespace();
    NAMESPACES
        .iter()
        .find(|(name, _)| *name == namespace)
        .map_or(namespace, |(_, title)| title)
}

/// Whether the palette lists `spec`: not one that only reads, or that
/// must be told what to act on, or the palette's own.
fn listed(spec: &Spec) -> bool {
    !spec.query && !spec.needs_target && spec.id != COMMAND
}

/// Every command that can run on `window` (a target naming one), as it is
/// now, with its keys.
pub fn entries(window: &Target, cx: &mut App) -> Vec<Entry> {
    let hotkeys = cx.global::<Settings>().hotkeys.clone();
    let mut entries = Vec::new();
    for spec in api::all().filter(|spec| listed(spec)) {
        let target = aimed(spec, window);
        if !api::can_run(spec.id, &target, cx) {
            continue;
        }
        let state = api::state(spec.id, &target, cx).map_or(State::Nothing, |now| State::of(&now));
        let keys = hotkeys
            .keys(spec.id)
            .iter()
            .map(|key| hotkeys::display(key).into())
            .collect();
        entries.push(Entry {
            spec,
            group: group(spec),
            keys,
            state,
        });
    }
    entries
}

/// What a palette lists.
pub enum Listing {
    /// Commands (⇧⌘P), those run from it lately first.
    Commands {
        entries: Vec<Entry>,
        recents: Vec<String>,
    },
    /// Places to go to (Go to…, ⌘P): windows, tiles and files.
    Places(Vec<Place>),
}

impl Listing {
    /// What it lists that `query` finds, best first.
    fn order(&self, query: &str) -> Vec<Row> {
        match self {
            Self::Commands { entries, recents } => order(query, entries, recents),
            Self::Places(places) => places::order(query, places),
        }
    }
}

/// A row: an entry, a place or a choice, by its place among them, and
/// where the letters typed are in its name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub index: usize,
    pub at: Vec<usize>,
}

/// The entries `query` finds, best first, with those run lately going
/// before others found as well; with nothing typed, those run lately
/// first, the latest first, then the rest as the commands are listed.
pub fn order(query: &str, entries: &[Entry], recents: &[String]) -> Vec<Row> {
    let mut found: Vec<(i32, Option<usize>, Row)> = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let found = search::rank(query, &entry.haystack())?;
            let recent = recents.iter().position(|id| id == entry.spec.id);
            Some((
                found.score,
                recent,
                Row {
                    index,
                    at: found.at,
                },
            ))
        })
        .collect();
    if query.trim().is_empty() {
        found.sort_by_key(|(_, recent, row)| (recent.unwrap_or(usize::MAX), row.index));
    } else {
        found.sort_by_key(|(score, recent, row)| {
            let bonus = recent.map_or(0, |at| {
                RECENT - RECENT_STEP * i32::try_from(at).unwrap_or(i32::MAX / RECENT_STEP)
            });
            (Reverse(score + bonus.max(0)), row.index)
        });
    }
    found.into_iter().map(|(_, _, row)| row).collect()
}

/// The choices `query` finds by their names or details, best first; with
/// nothing typed, all of them, in their order.
pub fn order_choices(query: &str, choices: &[Choice]) -> Vec<Row> {
    let mut found: Vec<(i32, Row)> = choices
        .iter()
        .enumerate()
        .filter_map(|(index, choice)| {
            let in_label = search::find(query, &choice.label);
            let in_detail = choice
                .detail
                .as_ref()
                .and_then(|detail| search::find(query, detail));
            let in_label = match (in_label, in_detail) {
                (Some(found), None) => found,
                (Some(found), Some(detail)) if found.score >= detail.score => found,
                (_, Some(detail)) => Found {
                    score: detail.score,
                    at: Vec::new(),
                },
                (None, None) => return None,
            };
            Some((
                in_label.score,
                Row {
                    index,
                    at: in_label.at,
                },
            ))
        })
        .collect();
    if !query.trim().is_empty() {
        found.sort_by_key(|(score, row)| (Reverse(*score), row.index));
    }
    found.into_iter().map(|(_, row)| row).collect()
}

#[derive(Clone, Debug)]
pub enum PaletteEvent {
    /// Run this, from the palette.
    Run(Invocation),
    /// Change the keys of this command, in Settings.
    ChangeShortcut(&'static str),
    Dismiss,
}

/// The command picked, while asking for its arguments.
struct Asking {
    spec: &'static Spec,
    /// Those given so far.
    args: Map<String, Value>,
    /// Those asked for so far, given or not.
    asked: Vec<&'static str>,
    question: Question,
}

/// What a row shows: a check, its name, and what goes after it.
struct RowParts {
    id: SharedString,
    check: bool,
    label: SharedString,
    details: Div,
    shortcut: Option<AnyElement>,
}

pub struct Palette {
    /// The window it's over, which what it runs is aimed at.
    window: Target,
    listing: Listing,
    input: Entity<TextInput>,
    /// What it lists now, best first.
    rows: Vec<Row>,
    /// The row Return picks.
    selected: usize,
    asking: Option<Asking>,
    /// Waiting to know what to ask next, which is read once the event
    /// that picked a command is handled.
    waiting: bool,
    /// Why what's typed won't do; typing takes it away.
    error: Option<SharedString>,
    scroll: UniformListScrollHandle,
    _input_changed: Subscription,
}

impl EventEmitter<PaletteEvent> for Palette {}

impl Palette {
    /// A palette listing `listing`, whose commands act on `window`.
    pub fn new(
        window_target: Target,
        listing: Listing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match listing {
            Listing::Commands { .. } => "Search commands",
            Listing::Places(_) => "Go to a window, tile or file",
        };
        let input = cx.new(|cx| TextInput::new(placeholder, cx).large());
        window.focus(&input.focus_handle(cx));
        let input_changed = cx.observe(&input, |this, _, cx| this.filter(cx));
        let rows = listing.order("");
        Self {
            window: window_target,
            listing,
            input,
            rows,
            selected: 0,
            asking: None,
            waiting: false,
            error: None,
            scroll: UniformListScrollHandle::new(),
            _input_changed: input_changed,
        }
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.input.focus_handle(cx));
    }

    /// List what's typed finds, the best picked.
    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_owned();
        self.error = None;
        self.rows = match &self.asking {
            None => self.listing.order(&query),
            Some(asking) => match &asking.question.ask {
                Ask::Choose(choices) => order_choices(&query, choices),
                Ask::Type(_) => Vec::new(),
            },
        };
        self.selected = 0;
        if query.trim().is_empty()
            && let Some(Asking {
                question:
                    Question {
                        ask: Ask::Choose(choices),
                        ..
                    },
                ..
            }) = &self.asking
            && let Some(at) = self.rows.iter().position(|row| choices[row.index].current)
        {
            self.selected = at;
        }
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Center);
        cx.notify();
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.select(
            self.selected
                .checked_sub(1)
                .unwrap_or(self.rows.len().saturating_sub(1)),
            cx,
        );
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let next = self.selected + 1;
        self.select(if next < self.rows.len() { next } else { 0 }, cx);
    }

    fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selected = row;
        self.scroll.scroll_to_item(row, ScrollStrategy::Top);
        cx.notify();
    }

    /// The command of the row picked, while commands are listed.
    fn selected_spec(&self) -> Option<&'static Spec> {
        let Listing::Commands { entries, .. } = &self.listing else {
            return None;
        };
        if self.asking.is_some() {
            return None;
        }
        let row = self.rows.get(self.selected)?;
        Some(entries[row.index].spec)
    }

    /// Whether it lists commands, not places.
    pub fn lists_commands(&self) -> bool {
        matches!(self.listing, Listing::Commands { .. })
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.pick(cx);
    }

    /// Take the row picked, or what's typed: run the command, or ask for
    /// what it takes next.
    fn pick(&mut self, cx: &mut Context<Self>) {
        if self.waiting {
            return;
        }
        let Some(asking) = &self.asking else {
            match &self.listing {
                Listing::Commands { .. } => {
                    if let Some(spec) = self.selected_spec() {
                        self.go_on(spec, Map::new(), Vec::new(), cx);
                    }
                }
                Listing::Places(places) => {
                    if let Some(row) = self.rows.get(self.selected) {
                        cx.emit(PaletteEvent::Run(places[row.index].run.clone()));
                    }
                }
            }
            return;
        };
        let mut args = asking.args.clone();
        let mut asked = asking.asked.clone();
        asked.push(asking.question.arg);
        let spec = asking.spec;
        match &asking.question.ask {
            Ask::Choose(choices) => {
                let Some(row) = self.rows.get(self.selected) else {
                    return;
                };
                args.extend(choices[row.index].args.clone());
            }
            Ask::Type(read) => match read(self.input.read(cx).text()) {
                Ok(value) => {
                    args.insert(asking.question.arg.to_owned(), value);
                }
                Err(why) => {
                    self.error = Some(why.into());
                    cx.notify();
                    return;
                }
            },
        }
        self.go_on(spec, args, asked, cx);
    }

    /// Run `spec` with `args`, if there's nothing more to ask for; else
    /// ask for what's next, once the event is handled, when what it's
    /// asked from can be read.
    fn go_on(
        &mut self,
        spec: &'static Spec,
        args: Map<String, Value>,
        asked: Vec<&'static str>,
        cx: &mut Context<Self>,
    ) {
        let all_asked = spec
            .args
            .iter()
            .all(|arg| asked.contains(&arg.name) || args.contains_key(arg.name));
        if all_asked {
            self.run(spec, args, cx);
            return;
        }
        self.waiting = true;
        let window = self.window.clone();
        let palette = cx.entity().downgrade();
        cx.defer(move |cx| {
            let question = asking::next(spec, &asked, &args, &window, cx);
            palette
                .update(cx, |palette, cx| {
                    palette.waiting = false;
                    match question {
                        Some(question) => palette.ask(spec, args, asked, question, cx),
                        None => palette.run(spec, args, cx),
                    }
                })
                .ok();
        });
    }

    /// Ask `question` before running `spec`.
    fn ask(
        &mut self,
        spec: &'static Spec,
        args: Map<String, Value>,
        asked: Vec<&'static str>,
        question: Question,
        cx: &mut Context<Self>,
    ) {
        let prompt = question.prompt.clone();
        self.asking = Some(Asking {
            spec,
            args,
            asked,
            question,
        });
        self.input.update(cx, |input, cx| {
            input.set_placeholder(prompt, cx);
            input.set_text("", cx);
        });
        // Setting the text tells `filter`, but only once this update ends.
        self.filter(cx);
    }

    /// Run `spec` with `args` on the palette's window, noting it among
    /// those run lately.
    fn run(&mut self, spec: &'static Spec, args: Map<String, Value>, cx: &mut Context<Self>) {
        recents::COMMANDS.note(spec.id, cx);
        let args = if args.is_empty() {
            Value::Null
        } else {
            Value::Object(args)
        };
        let mut invocation = Invocation::new(spec.id)
            .with_args(args)
            .from(Source::Palette);
        invocation.target = aimed(spec, &self.window);
        cx.emit(PaletteEvent::Run(invocation));
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PaletteEvent::Dismiss);
    }

    fn change_shortcut(&mut self, _: &ChangeShortcut, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(spec) = self.selected_spec().filter(|spec| spec.takes_keys()) {
            cx.emit(PaletteEvent::ChangeShortcut(spec.id));
        }
    }

    /// What a command's row shows.
    fn command_row(
        &self,
        entry: &Entry,
        selected: bool,
        muted: Hsla,
        cx: &mut Context<Self>,
    ) -> RowParts {
        let spec = entry.spec;
        let state = match &entry.state {
            State::Is(value) => Some(value.clone()),
            State::On | State::Nothing => None,
        };
        let keys = (!entry.keys.is_empty()).then(|| SharedString::from(entry.keys.join("  ")));
        let shortcut = (selected && spec.takes_keys()).then(|| {
            Button::new("palette-change-shortcut", "Change Shortcut…")
                .small()
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.stop_propagation();
                    cx.emit(PaletteEvent::ChangeShortcut(spec.id));
                }))
                .into_any_element()
        });
        let details = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .text_color(muted)
            .when_some(state, |this, state| this.child(state))
            .when_some(keys, |this, keys| this.child(keys))
            .child(
                div()
                    .w(GROUP_WIDTH)
                    .flex_none()
                    .truncate()
                    .text_right()
                    .child(entry.group),
            );
        RowParts {
            id: format!("palette-row-{}", spec.id).into(),
            check: entry.state == State::On,
            label: spec.title.into(),
            details,
            shortcut,
        }
    }

    /// What a choice's row shows.
    fn choice_row(choice: &Choice, muted: Hsla) -> RowParts {
        RowParts {
            id: format!("palette-choice-{}", choice.label).into(),
            check: choice.current,
            label: choice.label.clone(),
            details: div()
                .flex_none()
                .text_color(muted)
                .when_some(choice.detail.clone(), |this, detail| this.child(detail)),
            shortcut: None,
        }
    }

    /// The command asking, if one is, and the field.
    fn header(&self, theme: &Theme) -> Div {
        div()
            .flex()
            .items_center()
            .gap_2()
            .p_2()
            .when_some(self.asking.as_ref(), |this, asking| {
                this.child(
                    div()
                        .debug_selector(|| "palette-asking".into())
                        .flex_none()
                        .px_2()
                        .py_0p5()
                        .superellipse(radius::ITEM[2])
                        .bg(theme.surface_active)
                        .text_sm()
                        .child(asking.spec.title),
                )
            })
            .child(div().flex_1().min_w_0().child(self.input.clone()))
    }

    /// The rows, building only those in view; or, with none, `empty`,
    /// unless something is to be typed.
    fn list(
        &self,
        theme: &Theme,
        typing: bool,
        empty: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = self.rows.len();
        if rows > 0 {
            let theme = theme.clone();
            uniform_list(
                "palette-rows",
                rows,
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    range
                        .map(|ix| this.render_row(ix, &theme, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .w_full()
            .h(ROW_HEIGHT * rows.min(ROWS) as f32)
            .pb_1()
            .into_any_element()
        } else if typing {
            div().into_any_element()
        } else {
            div()
                .debug_selector(|| "palette-empty".into())
                .px_4()
                .pb_3()
                .text_color(theme.text_muted)
                .child(empty)
                .into_any_element()
        }
    }

    /// What a place's row shows: where it is, and what it is.
    fn place_row(place: &Place, muted: Hsla) -> RowParts {
        RowParts {
            id: format!("palette-place-{}", place.label).into(),
            check: place.current,
            label: place.label.clone(),
            details: div()
                .flex_none()
                .flex()
                .items_center()
                .gap_3()
                .text_color(muted)
                .when_some(place.detail.clone(), |this, detail| {
                    this.child(div().max_w(PALETTE_WIDTH / 3.).truncate().child(detail))
                })
                .child(
                    div()
                        .w(GROUP_WIDTH)
                        .flex_none()
                        .truncate()
                        .text_right()
                        .child(place.kind.name()),
                ),
            shortcut: None,
        }
    }

    fn render_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let row = &self.rows[ix];
        let selected = ix == self.selected;
        let muted = if selected {
            white().opacity(DETAIL_SELECTED)
        } else {
            theme.text_muted
        };
        let RowParts {
            id,
            check,
            label,
            details,
            shortcut,
        } = match (&self.asking, &self.listing) {
            (None, Listing::Commands { entries, .. }) => {
                self.command_row(&entries[row.index], selected, muted, cx)
            }
            (None, Listing::Places(places)) => Self::place_row(&places[row.index], muted),
            (Some(asking), _) => {
                let Ask::Choose(choices) = &asking.question.ask else {
                    return div().into_any_element();
                };
                Self::choice_row(&choices[row.index], muted)
            }
        };
        let accent = theme.accent;
        let hover = theme.surface_hover;
        let selector = id.clone();
        div()
            .id(id)
            .debug_selector(move || selector.to_string())
            .h(ROW_HEIGHT)
            .mx_1()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .superellipse(radius::ITEM[2])
            .cursor_pointer()
            .when(selected, |this| this.bg(accent).text_color(white()))
            .when(!selected, |this| this.hover(move |style| style.bg(hover)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = ix;
                this.pick(cx);
            }))
            // A character rather than an icon, so it turns white with the
            // rest of the row.
            .child(
                div()
                    .flex_none()
                    .w(CHECK_WIDTH)
                    .when(check, |this| this.child("✓")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(highlighted(label, &row.at)),
            )
            .children(shortcut)
            .child(details)
            .into_any_element()
    }
}

#[cfg(test)]
impl Palette {
    /// What it lists, in order: the commands' ids, or the places' or
    /// choices' names.
    pub fn listed(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| match (&self.asking, &self.listing) {
                (None, Listing::Commands { entries, .. }) => entries[row.index].spec.id.to_owned(),
                (None, Listing::Places(places)) => places[row.index].label.to_string(),
                (Some(asking), _) => match &asking.question.ask {
                    Ask::Choose(choices) => choices[row.index].label.to_string(),
                    Ask::Type(_) => String::new(),
                },
            })
            .collect()
    }

    /// The row Return picks, as `listed` names it.
    pub fn picked(&self) -> Option<String> {
        self.listed().into_iter().nth(self.selected)
    }

    /// The command and the argument asked for, while asking.
    pub fn asking(&self) -> Option<(&'static str, &'static str)> {
        self.asking
            .as_ref()
            .map(|asking| (asking.spec.id, asking.question.arg))
    }

    /// What `command` changes, as it was when the palette opened.
    pub fn state(&self, command: &str) -> Option<State> {
        let Listing::Commands { entries, .. } = &self.listing else {
            return None;
        };
        entries
            .iter()
            .find(|entry| entry.spec.id == command)
            .map(|entry| entry.state.clone())
    }

    /// The choices, or places, that are what's picked now, or where it
    /// is.
    pub fn current(&self) -> Vec<String> {
        match (
            self.asking.as_ref().map(|asking| &asking.question.ask),
            &self.listing,
        ) {
            (Some(Ask::Choose(choices)), _) => choices
                .iter()
                .filter(|choice| choice.current)
                .map(|choice| choice.label.to_string())
                .collect(),
            (None, Listing::Places(places)) => places
                .iter()
                .filter(|place| place.current)
                .map(|place| place.label.to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// What it lists, while places are: each one's kind and name.
    pub fn places_listed(&self) -> Vec<(places::Kind, String)> {
        let Listing::Places(places) = &self.listing else {
            return Vec::new();
        };
        self.rows
            .iter()
            .map(|row| {
                let place = &places[row.index];
                (place.kind, place.label.to_string())
            })
            .collect()
    }

    /// The place picked, as `places_listed` names it, and where it is.
    pub fn picked_place(&self) -> Option<(places::Kind, String, Option<String>)> {
        let Listing::Places(places) = &self.listing else {
            return None;
        };
        let place = &places[self.rows.get(self.selected)?.index];
        let detail = place.detail.as_ref().map(ToString::to_string);
        Some((place.kind, place.label.to_string(), detail))
    }
}

/// `text`, its letters at `at` bold.
fn highlighted(text: SharedString, at: &[usize]) -> StyledText {
    let bold = HighlightStyle {
        font_weight: Some(FontWeight::BOLD),
        ..HighlightStyle::default()
    };
    let ranges = runs(&text, at).into_iter().map(|range| (range, bold));
    StyledText::new(text.clone()).with_highlights(ranges)
}

/// The letters of `text` at `at`, those side by side run together.
fn runs(text: &str, at: &[usize]) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for &start in at {
        let Some(letter) = text.get(start..).and_then(|rest| rest.chars().next()) else {
            continue;
        };
        let end = start + letter.len_utf8();
        match runs.last_mut() {
            Some(run) if run.end == start => run.end = end,
            _ => runs.push(start..end),
        }
    }
    runs
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let asking = self.asking.as_ref();
        let typing = asking.is_some_and(|asking| matches!(asking.question.ask, Ask::Type(_)));
        let note = match (&self.error, asking) {
            (Some(error), _) => Some((error.clone(), theme.danger, "palette-error")),
            (None, Some(asking)) if typing => Some((
                asking.question.prompt.clone(),
                theme.text_muted,
                "palette-prompt",
            )),
            _ => None,
        };
        let empty = match (asking, &self.listing) {
            (None, Listing::Commands { .. }) => "No commands found",
            (None, Listing::Places(_)) => "Nowhere found",
            (Some(_), _) => "Nothing to pick",
        };

        let header = self.header(&theme);
        let list = self.list(&theme, typing, empty, cx);

        let card = Panel::new()
            .occlude()
            .superellipse(radius::DIALOG)
            .w(PALETTE_WIDTH)
            .max_w_full()
            .flex()
            .flex_col()
            .child(header)
            .child(list)
            .when_some(note, |this, (note, color, selector)| {
                this.child(
                    div()
                        .debug_selector(move || selector.into())
                        .px_4()
                        .pb_3()
                        .text_xs()
                        .text_color(color)
                        .child(note),
                )
            });

        div()
            .id("palette")
            .debug_selector(|| "palette".into())
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::change_shortcut))
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
                cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Dismiss)),
            )
            .child(card)
    }
}
