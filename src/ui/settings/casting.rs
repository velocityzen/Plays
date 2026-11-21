//! Casting: the protocols that look for TVs and speakers to cast to, the
//! devices they have found, with what each shows, and the AirPlay TVs
//! paired with.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, Window, div};

use super::form::{group, row, update_settings};
use super::{Pane, SettingsView};
use crate::app::{AudioEngine, airplay_id};
use crate::cast::{AppPairings, Casting, Device, Looking, Shows, airplay};
use crate::media::output::DeviceInfo;
use crate::settings::Settings;
use crate::ui::components::{Button, Switch};
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
                    Looking::Failed(why) => sentence(&why),
                    Looking::Off | Looking::On => finds.to_owned(),
                };
                row(
                    kind,
                    Some(hint.into()),
                    theme,
                    div()
                        .debug_selector(move || format!("look-with-{kind}"))
                        .child(Switch::new(kind, s.casting.looks_with(kind)).on_toggle(
                            move |&on, _, cx| {
                                update_settings(cx, true, |s| {
                                    s.casting.look_with.insert(kind.to_owned(), on);
                                })
                            },
                        )),
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
/// its PIN again.
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
            let pairings = Arc::clone(&pairings);
            row(
                name,
                Some("AirPlay: asks for its PIN again once forgotten".into()),
                theme,
                div().debug_selector(move || selector).child(
                    Button::new(SharedString::from(format!("forget-{id}")), "Forget").on_click(
                        cx.listener(move |_, _, _, cx| {
                            pairings.forget(&id);
                            cx.notify();
                        }),
                    ),
                ),
            )
        })
        .collect()
}

/// Why a protocol can't look, as it's logged, begun as a sentence.
fn sentence(why: &str) -> String {
    let mut chars = why.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
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
mod tests {
    use super::*;

    #[test]
    fn why_a_protocol_cant_look_begins_as_a_sentence() {
        assert_eq!(
            sentence("can't look for DLNA renderers: no network"),
            "Can't look for DLNA renderers: no network"
        );
        assert_eq!(sentence(""), "");
    }

    fn device(kind: &'static str, id: &str, name: &str, shows: Shows) -> Device {
        Device {
            kind,
            id: id.into(),
            name: name.into(),
            shows,
            address: std::net::Ipv4Addr::LOCALHOST.into(),
        }
    }

    fn output(id: &str, name: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: name.into(),
        }
    }

    fn listed(found: &[Found]) -> Vec<(&str, &str, Shows)> {
        found
            .iter()
            .map(|device| (device.name.as_str(), device.kind, device.shows))
            .collect()
    }

    #[test]
    fn a_device_found_by_two_protocols_is_listed_with_each() {
        let devices = [
            device(airplay::KIND, "tv", "Beamer", Shows::Video),
            device("DLNA", "uuid-1", "Phantom", Shows::Sound),
        ];
        // This computer's speakers, the Phantom over AirPlay, and the Apple
        // TV, which plays sound too.
        let outputs = [
            output("BuiltInSpeakerDevice", "MacBook Pro Speakers"),
            output("airplay:ph", "Phantom"),
            output("airplay:tv", "Beamer"),
        ];
        assert_eq!(
            listed(&found_devices(&devices, &outputs, true)),
            [
                ("Beamer", airplay::KIND, Shows::Video),
                ("Phantom", airplay::KIND, Shows::Sound),
                ("Phantom", "DLNA", Shows::Sound),
            ]
        );
        assert_eq!(
            listed(&found_devices(&devices, &outputs, false)),
            [
                ("Beamer", airplay::KIND, Shows::Video),
                ("Phantom", "DLNA", Shows::Sound),
            ],
            "AirPlay's speakers, while AirPlay looks"
        );
    }

    #[test]
    fn devices_found_say_what_they_show() {
        assert_eq!(shows(Shows::Video), "Video");
        assert_eq!(shows(Shows::Sound), "Sound only");
    }
}
