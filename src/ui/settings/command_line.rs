//! Command Line: whether a command line, `plays` and scripts, may run
//! commands and hear what changes, which is off until turned on here; and,
//! on macOS, installing `plays` where a terminal finds it.

use gpui::{AnyElement, Context, Window};

use super::form::{group, row, set};
use super::{Pane, SettingsView};
use crate::api::{self, Invocation};
use crate::settings::Settings;
use crate::ui::components::{Button, Switch};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::COMMAND_LINE_COLOR;

pub(super) struct CommandLinePane;

impl Pane for CommandLinePane {
    fn name(&self) -> &'static str {
        "Command Line"
    }

    fn icon(&self) -> &'static str {
        "terminal"
    }

    fn color(&self) -> u32 {
        COMMAND_LINE_COLOR
    }

    fn content(
        &self,
        _: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        _: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let mut rows = vec![row(
            "Allow control from the command line",
            Some(
                "Lets plays in a terminal, and scripts, run any command, list the \
                 windows and blocks, and hear what changes. plays FILE opens files \
                 either way."
                    .into(),
            ),
            theme,
            Switch::new("command-line-control", s.command_line_control)
                .on_toggle(|&on, _, cx| set("command_line_control", on, cx)),
        )];
        // Elsewhere, Plays is installed where the system keeps programs.
        if api::find("cli.install").is_some_and(|spec| (spec.available)()) {
            rows.push(row(
                "Command line tool",
                Some("Links plays into /usr/local/bin, where a terminal finds it.".into()),
                theme,
                Button::new("install-cli", "Install")
                    .on_click(|_, _, cx| api::dispatch(Invocation::new("cli.install"), cx)),
            ));
        }
        vec![group(None, theme, rows)]
    }
}
