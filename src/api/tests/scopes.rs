//! Block, window and app alike: a feature with commands for more than one
//! of a block, a window and every window has the same for each, titled
//! alike, taking the same arguments, and telling what the narrower one
//! tells.

use std::collections::{BTreeMap, BTreeSet};

use crate::api::registry::{self, Spec};

/// What each acts on, as its ids start, from the narrowest, and the end
/// of its commands' titles.
const SCOPES: [(&str, &str); 3] = [
    ("block", ""),
    ("window", " in Window"),
    ("app", " in Every Window"),
];

/// Features under more than one whose commands aren't alike, and why.
const UNLIKE: &[(&str, &str)] = &[(
    "move",
    "a block moves about its window's grid, and a window among the windows: neither moves what's \
     in it",
)];

/// Commands that the other scopes of their feature haven't, and why.
const ONLY: &[(&str, &str)] = &[
    (
        "block.playback.sync_all",
        "it brings every block in every window to where one is: to the block's",
    ),
    (
        "block.playback.skip",
        "an intro or credits are one file's, skipped as its own button shows them",
    ),
    (
        "block.subtitles.bigger",
        "how subtitles look is a setting every file's follow: it acts on every window",
    ),
    (
        "block.subtitles.smaller",
        "how subtitles look is a setting every file's follow: it acts on every window",
    ),
    (
        "block.subtitles.up",
        "how subtitles look is a setting every file's follow: it acts on every window",
    ),
    (
        "block.subtitles.down",
        "how subtitles look is a setting every file's follow: it acts on every window",
    ),
];

/// Every feature's commands: by feature, then scope, then name within the
/// feature, such as `playback` → `window` → `loop.on`. Commands that are
/// no feature's, as `block.close`, are left out.
fn features()
-> BTreeMap<&'static str, BTreeMap<&'static str, BTreeMap<&'static str, &'static Spec>>> {
    let mut features: BTreeMap<_, BTreeMap<_, BTreeMap<_, _>>> = BTreeMap::new();
    for spec in registry::all() {
        let Some((scope, rest)) = spec.id.split_once('.') else {
            continue;
        };
        if !SCOPES.iter().any(|&(name, _)| name == scope) {
            continue;
        }
        let Some((feature, name)) = rest.split_once('.') else {
            continue;
        };
        features
            .entry(feature)
            .or_default()
            .entry(scope)
            .or_default()
            .insert(name, spec);
    }
    features
}

/// What `spec` takes, as the others must take it.
fn args(spec: &Spec) -> Vec<(&str, bool, &str)> {
    spec.args
        .iter()
        .map(|arg| (arg.name, arg.required, arg.description))
        .collect()
}

#[test]
fn a_feature_for_more_than_one_scope_has_the_same_commands_for_each() {
    let mut wrong = Vec::new();
    for (feature, scopes) in features() {
        if scopes.len() < 2 || UNLIKE.iter().any(|&(unlike, _)| unlike == feature) {
            continue;
        }
        let names = |scope: &str| -> BTreeSet<&str> {
            scopes.get(scope).map_or_else(BTreeSet::new, |specs| {
                specs
                    .iter()
                    .filter(|(_, spec)| !ONLY.iter().any(|&(id, _)| id == spec.id))
                    .map(|(&name, _)| name)
                    .collect()
            })
        };
        let all: BTreeSet<&str> = SCOPES.iter().flat_map(|&(scope, _)| names(scope)).collect();
        for (scope, _) in SCOPES {
            let has = names(scope);
            let missing: Vec<_> = all.difference(&has).collect();
            if !missing.is_empty() {
                wrong.push(format!("{scope}.{feature} hasn't {missing:?}"));
            }
        }
        for name in &all {
            let found: Vec<(&str, &Spec)> = SCOPES
                .iter()
                .filter_map(|&(scope, end)| Some((end, *scopes.get(scope)?.get(name)?)))
                .collect();
            for pair in found.windows(2) {
                let [(_, narrow), (end, wide)] = pair else {
                    continue;
                };
                let (_, block) = found[0];
                if wide.title != format!("{}{end}", block.title) {
                    wrong.push(format!(
                        "{} is titled {:?}, not {:?}",
                        wide.id,
                        wide.title,
                        format!("{}{end}", block.title)
                    ));
                }
                if args(wide) != args(narrow) {
                    wrong.push(format!(
                        "{} takes other arguments than {}",
                        wide.id, narrow.id
                    ));
                }
                let untold: Vec<_> = narrow
                    .tells
                    .iter()
                    .filter(|kind| !wide.tells.contains(kind))
                    .collect();
                if !untold.is_empty() {
                    wrong.push(format!(
                        "{} doesn't tell {untold:?}, as {} does",
                        wide.id, narrow.id
                    ));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn what_isnt_alike_is_there_and_says_why() {
    let features = features();
    for (feature, why) in UNLIKE {
        let scopes = features.get(feature).map_or(0, BTreeMap::len);
        assert!(scopes > 1, "{feature} isn't under more than one scope");
        assert!(why.len() > 20, "why {feature} isn't alike");
    }
    for (id, why) in ONLY {
        assert!(registry::find(id).is_some(), "{id} isn't a command");
        assert!(why.len() > 20, "why {id} is only its scope's");
    }
}
