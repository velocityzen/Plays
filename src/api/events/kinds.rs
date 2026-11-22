//! What each event is: its type, who hears it, what its payload has, and
//! what tells it besides the commands that say they do (`Spec::tells`),
//! as the command reference lists them. Each namespace's events are beside
//! its commands, in `commands`.

use crate::api::commands;

/// An event Plays tells.
pub struct EventKind {
    /// Its `type`, such as `volume_changed`.
    pub name: &'static str,
    /// A sentence saying what changed.
    pub description: &'static str,
    pub heard: Heard,
    /// What its payload has, after the ids `heard` puts first.
    pub fields: &'static [EventField],
    /// What tells it besides the commands that say they do, if anything:
    /// a file playing to its end, say.
    pub causes: &'static [&'static str],
}

/// Who hears an event, and so which ids its payload starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// Those listening to the app's. Its payload names no window.
    App,
    /// Those listening to the app's, and to the window it names, by
    /// `window_id` and `window_title`.
    AppAndWindow,
    /// Those listening to the window it names.
    Window,
    /// Those listening to the window of the block it names, by `block_id`
    /// and `block_title` too.
    Block,
    /// Every listener, whatever it listens to.
    Everyone,
}

impl Heard {
    /// The ids its events' payloads start with.
    pub fn ids(self) -> &'static [&'static str] {
        match self {
            Self::App | Self::Everyone => &[],
            Self::AppAndWindow | Self::Window => &["window_id", "window_title"],
            Self::Block => &["window_id", "window_title", "block_id", "block_title"],
        }
    }
}

/// A field of an event's payload.
pub struct EventField {
    pub name: &'static str,
    /// What it is, and the values it takes.
    pub description: &'static str,
    /// Whether it's there only sometimes, not just none.
    pub optional: bool,
}

impl EventField {
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            optional: false,
        }
    }

    /// One that's there only sometimes.
    pub const fn optional(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            optional: true,
        }
    }
}

impl EventKind {
    const fn new(
        name: &'static str,
        description: &'static str,
        heard: Heard,
        fields: &'static [EventField],
    ) -> Self {
        Self {
            name,
            description,
            heard,
            fields,
            causes: &[],
        }
    }

    /// One of the app's.
    pub const fn app(
        name: &'static str,
        description: &'static str,
        fields: &'static [EventField],
    ) -> Self {
        Self::new(name, description, Heard::App, fields)
    }

    /// One of a window's that the app's listeners hear too.
    pub const fn of_window(
        name: &'static str,
        description: &'static str,
        fields: &'static [EventField],
    ) -> Self {
        Self::new(name, description, Heard::AppAndWindow, fields)
    }

    /// One of a window's.
    pub const fn window(
        name: &'static str,
        description: &'static str,
        fields: &'static [EventField],
    ) -> Self {
        Self::new(name, description, Heard::Window, fields)
    }

    /// One of a block's.
    pub const fn block(
        name: &'static str,
        description: &'static str,
        fields: &'static [EventField],
    ) -> Self {
        Self::new(name, description, Heard::Block, fields)
    }

    /// One every listener hears.
    pub const fn to_everyone(
        name: &'static str,
        description: &'static str,
        fields: &'static [EventField],
    ) -> Self {
        Self::new(name, description, Heard::Everyone, fields)
    }

    /// Told for `causes` too, besides the commands that say they tell it.
    pub const fn causes(mut self, causes: &'static [&'static str]) -> Self {
        self.causes = causes;
        self
    }
}

/// Every event, with its namespace, in the commands' order.
pub fn all() -> impl Iterator<Item = (&'static str, &'static EventKind)> {
    commands::EVENTS
        .iter()
        .flat_map(|&(namespace, kinds)| kinds.iter().map(move |kind| (namespace, kind)))
}

/// The event `name`.
#[cfg(test)]
pub fn find(name: &str) -> Option<&'static EventKind> {
    all().map(|(_, kind)| kind).find(|kind| kind.name == name)
}

/// Check that `event` is told as its kind says: to whom, and with what
/// payload. Each test that tells one checks it so.
#[cfg(test)]
pub fn check(event: &super::Event) {
    if let Err(why) = fits(event) {
        panic!(
            "{} isn't told as the events reference says: {why}",
            event.kind
        );
    }
}

#[cfg(test)]
fn fits(event: &super::Event) -> Result<(), String> {
    let kind = find(event.kind).ok_or("it isn't in it")?;
    let payload = event
        .payload
        .as_object()
        .ok_or("its payload isn't an object")?;
    let ids = kind.heard.ids();
    let names: Vec<&str> = ids
        .iter()
        .copied()
        .chain(kind.fields.iter().map(|field| field.name))
        .collect();
    if let Some(key) = payload.keys().find(|key| !names.contains(&key.as_str())) {
        return Err(format!("its payload has {key}"));
    }
    let needed = ids.iter().copied().chain(
        kind.fields
            .iter()
            .filter(|field| !field.optional)
            .map(|field| field.name),
    );
    if let Some(name) = needed.into_iter().find(|name| !payload.contains_key(*name)) {
        return Err(format!("its payload has no {name}"));
    }
    let reach = &event.reach;
    // The window it names, which hears it first.
    let window = payload.get("window_id").and_then(|id| id.as_str());
    let first = reach.windows.first().map(ToString::to_string);
    let heard = match kind.heard {
        Heard::App => reach.app && reach.windows.is_empty() && !reach.everyone,
        Heard::AppAndWindow => reach.app && !reach.everyone && first.as_deref() == window,
        Heard::Window | Heard::Block => {
            !reach.app && !reach.everyone && first.is_some() && first.as_deref() == window
        }
        Heard::Everyone => reach.everyone,
    };
    if heard {
        Ok(())
    } else {
        Err(format!("it's heard by {reach:?}"))
    }
}

#[cfg(test)]
#[path = "tests/kinds.rs"]
mod tests;
