//! The command palette: a sheet over a player window listing every
//! command that can run there, which ⇧⌘P (`window.command_palette`)
//! opens. With nothing typed, each group of commands is under its heading,
//! those run from it lately (`recents`) first under Recent; typing finds
//! commands by their titles, groups and keywords (`search`), best first
//! with no headings, each saying its group. A row shows what a command
//! changes as it is (✓ Loop) and its keys. The row picked is in the
//! accent color, and pointing at a row picks it. Return runs the one
//! picked, through the API as from the palette, asking first for the
//! arguments it takes (`asking`), picked from a list or typed. ⌘Return
//! changes its keys, there in the palette (`shortcut`). The footer says
//! what both do, and either can be clicked. Escape, or a click outside,
//! closes it.
//!
//! Go to… (⌘P, `window.go_to`) is the same sheet listing places instead
//! (`places`): the windows, their tiles, the files in the front tile's
//! folder and those played lately, under a heading for each, found as
//! commands are; Return goes there, running the command each has.

mod asking;
mod places;
mod search;
mod shortcut;
#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

use std::cmp::Reverse;
use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    HighlightStyle, Hsla, KeyBinding, MouseButton, ScrollStrategy, SharedString, Stateful,
    StyledText, Subscription, UniformListScrollHandle, Window, actions, black, div, uniform_list,
    white,
};
use serde_json::{Map, Value};

use super::components::{Icon, Panel};
use super::text_input::TextInput;
use super::theme::palette::{
    CHECK_WIDTH, DETAIL_SELECTED, FIELD_HEIGHT, FIELD_RADIUS, INSET, KEYCAP_RADIUS, LIST_PADDING,
    MARGIN, RADIUS, ROW_HEIGHT, ROW_RADIUS, ROWS, SEARCH_ICON, WIDTH as PALETTE_WIDTH,
};
use super::theme::text::TextStyle;
use super::theme::{Theme, dialog};
use crate::api::commands::NAMESPACES;
use crate::api::{self, Invocation, Scope, Source, Spec, Target};
use crate::hotkeys;
use crate::recents;
use crate::settings::Settings;
use asking::{Ask, Choice, Question};
use places::Place;
use search::{Found, Haystack};
use shortcut::Changing;

pub use places::places;

actions!(
    palette,
    [SelectPrevious, SelectNext, Confirm, Cancel, ChangeShortcut]
);

const CONTEXT: &str = "CommandPalette";

/// Its key context while it waits for keys to add to a command's: keys
/// that work anywhere or in a player then stay out of the way.
const RECORDING: &str = "CommandPalette Recording";

/// The command that opens it, which it doesn't list.
pub const COMMAND: &str = "window.command_palette";

/// The command that opens Go to….
pub const GO_TO: &str = "window.go_to";

/// What a command run lately counts for more, the latest most, as it
/// goes before others found as well.
const RECENT: i32 = 16;
const RECENT_STEP: i32 = 2;

/// The heading over the commands run lately.
const RECENT_HEADING: &str = "Recent";

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

/// Keys as the menus show them.
fn shown_keys(keys: &[String]) -> Vec<SharedString> {
    keys.iter()
        .map(|key| hotkeys::display(key).into())
        .collect()
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
        let keys = shown_keys(hotkeys.keys(spec.id));
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

    /// The heading what it lists at `index` goes under, with nothing
    /// typed: Recent, for a command run lately, else its group's; a
    /// place's kind's.
    fn heading(&self, index: usize) -> &'static str {
        match self {
            Self::Commands { entries, recents } => {
                let entry = &entries[index];
                if recents.iter().any(|id| id == entry.spec.id) {
                    RECENT_HEADING
                } else {
                    entry.group
                }
            }
            Self::Places(places) => places[index].kind.heading(),
        }
    }
}

/// A line of the list: a heading, or a row by its place among those
/// listed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Line {
    Heading(&'static str),
    Row(usize),
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
    /// What it's in, said after its name while there are no headings.
    group: Option<&'static str>,
    details: Div,
}

pub struct Palette {
    /// The window it's over, which what it runs is aimed at.
    window: Target,
    listing: Listing,
    input: Entity<TextInput>,
    /// What it lists now, best first.
    rows: Vec<Row>,
    /// The rows, under their headings while nothing is typed.
    lines: Vec<Line>,
    /// The row Return picks.
    selected: usize,
    asking: Option<Asking>,
    /// The command whose keys are changing, in place of the list.
    changing: Option<Changing>,
    /// Its own, which has the focus while keys are changing, so they
    /// aren't typed.
    focus_handle: FocusHandle,
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
        let input = cx.new(|cx| TextInput::new(placeholder, cx).bare());
        window.focus(&input.focus_handle(cx));
        let input_changed = cx.observe(&input, |this, _, cx| this.filter(cx));
        let rows = listing.order("");
        let mut palette = Self {
            window: window_target,
            listing,
            input,
            rows,
            lines: Vec::new(),
            selected: 0,
            asking: None,
            changing: None,
            focus_handle: cx.focus_handle(),
            waiting: false,
            error: None,
            scroll: UniformListScrollHandle::new(),
            _input_changed: input_changed,
        };
        palette.lines = palette.lay_out("");
        palette
    }

    /// The rows, under a heading for each group while nothing is typed
    /// and nothing asked for.
    fn lay_out(&self, query: &str) -> Vec<Line> {
        let headed = query.trim().is_empty() && self.asking.is_none();
        let mut lines = Vec::with_capacity(self.rows.len());
        let mut under = None;
        for (at, row) in self.rows.iter().enumerate() {
            if headed {
                let heading = self.listing.heading(row.index);
                if under != Some(heading) {
                    lines.push(Line::Heading(heading));
                    under = Some(heading);
                }
            }
            lines.push(Line::Row(at));
        }
        lines
    }

    /// Whether its rows are under headings.
    fn headed(&self) -> bool {
        matches!(self.lines.first(), Some(Line::Heading(_)))
    }

    /// The line to scroll into view for the row `row`: the heading over
    /// it, when it's the first under one and the list goes up.
    fn line_of(&self, row: usize, upward: bool) -> usize {
        let line = self
            .lines
            .iter()
            .position(|line| *line == Line::Row(row))
            .unwrap_or(0);
        match line.checked_sub(1).map(|above| self.lines[above]) {
            Some(Line::Heading(_)) if upward => line - 1,
            _ => line,
        }
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.changing.is_some() {
            window.focus(&self.focus_handle);
        } else {
            window.focus(&self.input.focus_handle(cx));
        }
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
        self.lines = self.lay_out(&query);
        self.scroll
            .scroll_to_item(self.line_of(self.selected, true), ScrollStrategy::Center);
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
        let upward = row < self.selected;
        self.selected = row;
        self.scroll
            .scroll_to_item(self.line_of(row, upward), ScrollStrategy::Top);
        cx.notify();
    }

    /// The command of the row picked, while commands are listed.
    fn selected_spec(&self) -> Option<&'static Spec> {
        let Listing::Commands { entries, .. } = &self.listing else {
            return None;
        };
        if self.asking.is_some() || self.changing.is_some() {
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
    /// what it takes next; remove the key, while keys are changing.
    fn pick(&mut self, cx: &mut Context<Self>) {
        if self.waiting {
            return;
        }
        if self.changing.is_some() {
            self.remove_picked(cx);
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

    /// Back to the commands, while keys are changing; else close.
    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.changing.is_some() {
            self.stop_changing(window, cx);
        } else {
            cx.emit(PaletteEvent::Dismiss);
        }
    }

    /// Change the keys of the command picked; while they are changing,
    /// ⌘Return is a key like any other, to add.
    fn change_shortcut(&mut self, _: &ChangeShortcut, window: &mut Window, cx: &mut Context<Self>) {
        if self.changing.is_some() {
            cx.propagate();
        } else if let Some(spec) = self.selected_spec().filter(|spec| spec.takes_keys()) {
            self.start_changing(spec, window, cx);
        }
    }

    /// What a command's row shows.
    fn command_row(entry: &Entry, muted: Hsla) -> RowParts {
        let spec = entry.spec;
        let state = match &entry.state {
            State::Is(value) => Some(value.clone()),
            State::On | State::Nothing => None,
        };
        let keys = (!entry.keys.is_empty()).then(|| SharedString::from(entry.keys.join("  ")));
        let details = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_3()
            .text_color(muted)
            .when_some(state, |this, state| this.child(state))
            .when_some(keys, |this, keys| this.child(keys));
        RowParts {
            id: format!("palette-row-{}", spec.id).into(),
            check: entry.state == State::On,
            label: spec.title.into(),
            group: Some(entry.group),
            details,
        }
    }

    /// What a choice's row shows.
    fn choice_row(choice: &Choice, muted: Hsla) -> RowParts {
        RowParts {
            id: format!("palette-choice-{}", choice.label).into(),
            check: choice.current,
            label: choice.label.clone(),
            group: None,
            details: div()
                .flex_none()
                .text_color(muted)
                .when_some(choice.detail.clone(), |this, detail| this.child(detail)),
        }
    }

    /// The field, filled, with the magnifier and, while a command asks
    /// for what it takes or its keys change, the command, before what's
    /// typed, or what to press.
    fn header(&self, theme: &Theme) -> Div {
        let command = match (&self.asking, &self.changing) {
            (Some(asking), _) => Some(asking.spec.title),
            (None, Some(changing)) => Some(changing.spec.title),
            (None, None) => None,
        };
        let field = div()
            .debug_selector(|| "palette-field".into())
            .flex()
            .items_center()
            .gap_2()
            .h(FIELD_HEIGHT)
            .px_3()
            .superellipse(FIELD_RADIUS)
            .bg(theme.surface_hover)
            .child(
                div()
                    .debug_selector(|| "palette-search-icon".into())
                    .flex_none()
                    .child(
                        Icon::new("search")
                            .size(SEARCH_ICON)
                            .color(theme.text_muted),
                    ),
            )
            .when_some(command, |this, command| {
                this.child(
                    div()
                        .debug_selector(|| "palette-asking".into())
                        .flex_none()
                        .px_2()
                        .py_0p5()
                        .superellipse(ROW_RADIUS)
                        .bg(theme.surface_active)
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .child(command),
                )
            })
            .map(|this| match &self.changing {
                Some(changing) => this.child(
                    div()
                        .debug_selector(|| "palette-press".into())
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.text_muted)
                        .child(format!("Press a shortcut for {}", changing.spec.title)),
                ),
                None => this.child(div().flex_1().min_w_0().h_full().child(self.input.clone())),
            });
        div()
            .p(INSET)
            .border_b_1()
            .border_color(theme.border)
            .child(field)
    }

    /// What Return does and, for a command that takes keys, ⌘Return, or
    /// while its keys change, Escape; each with its keys in a keycap, and
    /// each to be clicked as well.
    fn footer(&self, theme: &Theme, typing: bool, cx: &mut Context<Self>) -> Div {
        let changing = self.changing.is_some();
        let returns = match (&self.asking, &self.listing) {
            _ if changing => "Remove",
            (Some(asking), _) => match asking.question.ask {
                Ask::Choose(_) => "Pick",
                Ask::Type(_) => "OK",
            },
            (None, Listing::Commands { .. }) => "Run",
            (None, Listing::Places(_)) => "Go",
        };
        let changes = self.selected_spec().is_some_and(Spec::takes_keys);
        div()
            .debug_selector(|| "palette-footer".into())
            .flex()
            .items_center()
            .justify_end()
            .gap_4()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.text_muted)
            .when(changes, |this| {
                this.child(
                    hint(
                        "palette-change-shortcut",
                        "Change Shortcut",
                        hotkeys::display("secondary-enter"),
                        theme,
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.change_shortcut(&ChangeShortcut, window, cx);
                    })),
                )
            })
            .when(changing, |this| {
                this.child(
                    hint("palette-back", "Back", hotkeys::display("escape"), theme).on_click(
                        cx.listener(|this, _, window, cx| this.stop_changing(window, cx)),
                    ),
                )
            })
            .when(typing || !self.rows.is_empty(), |this| {
                this.child(
                    hint("palette-return", returns, hotkeys::display("enter"), theme)
                        .on_click(cx.listener(|this, _, _, cx| this.pick(cx))),
                )
            })
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
        let lines = self.lines.len();
        if lines > 0 {
            let theme = theme.clone();
            uniform_list(
                "palette-rows",
                lines,
                cx.processor(move |this, range: Range<usize>, _, cx| {
                    range
                        .map(|line| match this.lines[line] {
                            Line::Heading(heading) => this.render_heading(heading, &theme),
                            Line::Row(row) => this.render_row(row, &theme, cx),
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(self.scroll.clone())
            .w_full()
            .h(ROW_HEIGHT * lines.min(ROWS) as f32 + LIST_PADDING * 2.)
            .py(LIST_PADDING)
            .into_any_element()
        } else if typing {
            div().into_any_element()
        } else {
            div()
                .debug_selector(|| "palette-empty".into())
                .px_4()
                .py_3()
                .text_color(theme.text_muted)
                .child(empty)
                .into_any_element()
        }
    }

    /// What a place's row shows: what it is, and where.
    fn place_row(place: &Place, muted: Hsla) -> RowParts {
        RowParts {
            id: format!("palette-place-{}", place.label).into(),
            check: place.current,
            label: place.label.clone(),
            group: Some(place.kind.name()),
            details: div()
                .flex_none()
                .text_color(muted)
                .when_some(place.detail.clone(), |this, detail| {
                    this.child(div().max_w(PALETTE_WIDTH / 3.).truncate().child(detail))
                }),
        }
    }

    /// A row or a heading, which start their names in the same place.
    fn line() -> Div {
        div().h(ROW_HEIGHT).px_2().flex().gap_2()
    }

    /// `line` across the list, as far from its sides as the field is from
    /// the card's. A list lays out each line on its own, where a flex box
    /// is only as wide as what's in it and its margins are dropped; a
    /// plain box fills the list, and so does a line in it.
    fn inset(line: impl IntoElement) -> AnyElement {
        div().px(INSET).child(line).into_any_element()
    }

    fn render_heading(&self, heading: &'static str, theme: &Theme) -> AnyElement {
        Self::inset(
            Self::line()
                .debug_selector(move || format!("palette-heading-{heading}"))
                .items_end()
                .pb_1()
                .text_label()
                .text_color(theme.text_muted)
                .child(div().flex_none().w(CHECK_WIDTH))
                .child(heading),
        )
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
            group,
            details,
        } = match (&self.asking, &self.listing) {
            _ if let Some(changing) = &self.changing => Self::key_row(changing, row.index, muted),
            (None, Listing::Commands { entries, .. }) => {
                Self::command_row(&entries[row.index], muted)
            }
            (None, Listing::Places(places)) => Self::place_row(&places[row.index], muted),
            (Some(asking), _) => {
                let Ask::Choose(choices) = &asking.question.ask else {
                    return div().into_any_element();
                };
                Self::choice_row(&choices[row.index], muted)
            }
        };
        let group = group.filter(|_| !self.headed());
        let accent = theme.accent;
        let selector = id.clone();
        let line = Self::line()
            .id(id)
            .debug_selector(move || selector.to_string())
            .items_center()
            .superellipse(ROW_RADIUS)
            .cursor_pointer()
            .when(selected, |this| this.bg(accent).text_color(white()))
            // What's pointed at is picked: what Return runs.
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.selected != ix {
                    this.selected = ix;
                    cx.notify();
                }
            }))
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
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(highlighted(label, &row.at)),
                    )
                    .when_some(group, |this, group| {
                        this.child(div().flex_none().text_color(muted).child(group))
                    }),
            )
            .child(details);
        Self::inset(line)
    }
}

/// A hint in the footer: what `keys` do, and the keys in a keycap.
fn hint(id: &'static str, does: &'static str, keys: String, theme: &Theme) -> Stateful<Div> {
    let text = theme.text;
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .items_center()
        .gap_1p5()
        .cursor_pointer()
        .hover(move |style| style.text_color(text))
        .child(does)
        .child(
            div()
                .px_1p5()
                .border_1()
                .border_color(theme.border)
                .superellipse(KEYCAP_RADIUS)
                .child(keys),
        )
}

#[cfg(test)]
impl Palette {
    /// What it lists, in order: the commands' ids, or the places' or
    /// choices' names.
    /// While keys are changing, the keys, as saved.
    pub fn listed(&self) -> Vec<String> {
        if let Some(changing) = &self.changing {
            return self
                .rows
                .iter()
                .map(|row| changing.keys[row.index].clone())
                .collect();
        }
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

    /// Each heading and what it lists under it, as `listed` names them;
    /// with no headings, all of it under "".
    pub fn sections(&self) -> Vec<(String, Vec<String>)> {
        let listed = self.listed();
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        for line in &self.lines {
            match *line {
                Line::Heading(heading) => sections.push((heading.to_owned(), Vec::new())),
                Line::Row(row) => match sections.last_mut() {
                    Some((_, under)) => under.push(listed[row].clone()),
                    None => sections.push((String::new(), vec![listed[row].clone()])),
                },
            }
        }
        sections
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
        let taken = self
            .changing
            .as_ref()
            .and_then(|changing| changing.note.clone());
        let note = match (&self.error, asking) {
            _ if let Some(taken) = taken => Some((taken, theme.text_muted, "palette-taken")),
            (Some(error), _) => Some((error.clone(), theme.danger, "palette-error")),
            (None, Some(asking)) if typing => Some((
                asking.question.prompt.clone(),
                theme.text_muted,
                "palette-prompt",
            )),
            _ => None,
        };
        let empty = match (asking, &self.listing) {
            _ if self.changing.is_some() => "It has no shortcut",
            (None, Listing::Commands { .. }) => "No commands found",
            (None, Listing::Places(_)) => "Nowhere found",
            (Some(_), _) => "Nothing to pick",
        };

        let header = self.header(&theme);
        let list = self.list(&theme, typing, empty, cx);
        let footer = self.footer(&theme, typing, cx);

        let card = Panel::new()
            .occlude()
            .superellipse(RADIUS)
            .w(PALETTE_WIDTH)
            .max_w_full()
            // No taller than the window has room for: the list gives
            // way, and scrolls.
            .max_h_full()
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
            })
            .child(footer);

        div()
            .id("palette")
            .debug_selector(|| "palette".into())
            .key_context(if self.changing.is_some() {
                RECORDING
            } else {
                CONTEXT
            })
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::record))
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
            .px(MARGIN)
            .pb(MARGIN)
            .bg(black().opacity(dialog::BACKDROP))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(PaletteEvent::Dismiss)),
            )
            .child(card)
    }
}
