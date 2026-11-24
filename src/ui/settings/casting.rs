//! Casting: the protocols that look for TVs and speakers to cast to, the
//! devices they have found, with what each shows, and the AirPlay TVs
//! paired with.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, Window, div};

use super::form::{group, row, set};
use super::{Pane, SettingsView};
use crate::app::{AudioEngine, airplay_id, forget_receiver};
use crate::cast::{AppPairings, Casting, Device, Looking, Shows, airplay};
use crate::media::output::DeviceInfo;
use crate::settings::{Settings, fields};
use crate::ui::components::{Button, Switch};
use crate::ui::sentence::capitalized;
use crate::ui::theme::Theme;
use crate::ui::theme::settings::CASTING_COLOR;

pub(super) struct CastingPane;

impl Pane for CastingPane {
    fn name(&self) -> &'static str {
        "Casting"
    }

    fn icon(&self) -> &'static str {
        "cast"
    }

    fn color(&self) -> u32 {
        CASTING_COLOR
    }

    fn content(
        &self,
        _: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let protocols = Casting::protocols(cx);
        let looking = protocols
            .iter()
            .any(|(_, _, looking)| *looking != Looking::Off);
        let switches = protocols
            .into_iter()
            .map(|(kind, finds, looking)| {
                let hint = match looking {
                    Looking::Failed(why) => capitalized(&why),
                    Looking::Off | Looking::On => finds.to_owned(),
                };
                row(
                    kind,
                    Some(hint.into()),
                    theme,
                    div()
                        .debug_selector(move || format!("look-with-{kind}"))
                        .child(
                            Switch::new(kind, s.casting.looks_with(kind))
                                .on_toggle(move |&on, _, cx| set(fields::look_with(kind), on, cx)),
                        ),
                )
            })
            .collect();

        let airplay_looks = s.casting.looks_with(airplay::KIND);
        let mut found: Vec<AnyElement> = found_devices(
            Casting::devices(cx),
            AudioEngine::devices(cx),
            airplay_looks,
        )
        .into_iter()
        .map(|device| {
            let Found {
                name,
                kind,
                shows: what,
            } = device;
            let tag = format!("found-{kind}-{name}");
            div()
                .debug_selector(move || tag)
                .child(row(
                    name,
                    Some(kind.into()),
                    theme,
                    div()
                        .flex_none()
                        .text_sm()
                        .text_color(theme.text_muted)
                        .child(shows(what)),
                ))
                .into_any_element()
        })
        .collect();
        if found.is_empty() {
            let (label, hint) = if looking {
                (
                    "None yet",
                    "TVs and speakers on this network show here as they're found.",
                )
            } else {
                (
                    "Not looking",
                    "Turn a protocol on to find TVs and speakers.",
                )
            };
            found.push(row(label, Some(hint.into()), theme, div()));
        }

        let mut groups = vec![
            group(Some("Look for devices with"), theme, switches),
            group(Some("Found"), theme, found),
        ];
        let paired = paired(theme, cx);
        if !paired.is_empty() {
            groups.push(group(Some("Paired"), theme, paired));
        }
        groups
    }
}

/// The AirPlay TVs paired with, each of which can be forgotten, to ask for
/// its PIN again and be played through no longer (`forget_receiver`).
fn paired(theme: &Theme, cx: &mut Context<SettingsView>) -> Vec<AnyElement> {
    let Some(pairings) = cx
        .try_global::<AppPairings>()
        .map(|kept| Arc::clone(&kept.0))
    else {
        return Vec::new();
    };
    pairings
        .paired()
        .into_iter()
        .map(|(id, name)| {
            let selector = format!("forget-{name}");
            row(
                name,
                Some("AirPlay: asks for its PIN again once forgotten".into()),
                theme,
                div().debug_selector(move || selector).child(
                    Button::new(SharedString::from(format!("forget-{id}")), "Forget").on_click(
                        cx.listener(move |_, _, _, cx| {
                            forget_receiver(&id, cx);
                            cx.notify();
                        }),
                    ),
                ),
            )
        })
        .collect()
}

/// What a device found shows.
/// A device found, as Settings lists it.
#[derive(Clone, PartialEq, Debug)]
struct Found {
    name: String,
    kind: &'static str,
    shows: Shows,
}

/// What the protocols found, by name: the devices to cast to, and while
/// AirPlay looks, the AirPlay speakers among the `outputs`, which play as
/// outputs instead (an Apple TV, cast to as well, listed once). So a
/// device found by two protocols, as a speaker with AirPlay and DLNA, is
/// listed with each.
fn found_devices(devices: &[Device], outputs: &[DeviceInfo], airplay_looks: bool) -> Vec<Found> {
    let cast_to = |id: &str| {
        devices
            .iter()
            .any(|device| device.kind == airplay::KIND && device.id == id)
    };
    let speakers = outputs
        .iter()
        .filter(|_| airplay_looks)
        .filter_map(|output| {
            let id = airplay_id(&output.id)?;
            (!cast_to(id)).then(|| Found {
                name: output.name.clone(),
                kind: airplay::KIND,
                shows: Shows::Sound,
            })
        });
    let mut found: Vec<Found> = devices
        .iter()
        .map(|device| Found {
            name: device.name.clone(),
            kind: device.kind,
            shows: device.shows,
        })
        .chain(speakers)
        .collect();
    found.sort_by_cached_key(|device| (device.name.to_lowercase(), device.kind));
    found
}

fn shows(shows: Shows) -> &'static str {
    match shows {
        Shows::Video => "Video",
        Shows::Sound => "Sound only",
    }
}

#[cfg(test)]
#[path = "tests/casting.rs"]
mod tests;
