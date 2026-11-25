use super::*;

#[test]
fn it_credits_ffmpeg_dav1d_gpui_by_the_zed_team_and_lua_each_with_its_license() {
    let credits = built_with();
    let names: Vec<&str> = credits.iter().map(|entry| entry.name.as_ref()).collect();
    assert_eq!(
        names,
        ["FFmpeg", "dav1d", "GPUI", "Lua"],
        "and no crate besides"
    );
    for entry in &credits {
        assert!(!entry.texts.is_empty(), "{}'s license", entry.name);
        let links: Vec<&str> = entry.links.iter().map(|(label, _)| *label).collect();
        assert_eq!(links, ["Website", "Source code"], "{}", entry.name);
    }

    let [ffmpeg, dav1d, gpui, lua] = credits.as_slice() else {
        unreachable!()
    };
    assert_eq!(ffmpeg.version, ffmpeg_version(), "the one it runs with");
    assert_eq!(ffmpeg.license.as_ref(), ffmpeg::util::license());
    assert!(
        ffmpeg
            .note
            .as_ref()
            .is_some_and(|note| note.contains("unchanged"))
    );
    assert_eq!(
        ffmpeg.hint,
        Some("Opens and decodes files and streams.".into())
    );
    assert_eq!(dav1d.hint, Some("Decodes AV1 video, in FFmpeg.".into()));
    assert!(dav1d.note.is_none() && gpui.note.is_none());
    assert!(
        gpui.hint
            .as_ref()
            .is_some_and(|hint| hint.contains("Zed team")),
        "{:?}",
        gpui.hint
    );
    assert!(
        gpui.texts
            .iter()
            .any(|text| text.contains("Zed Industries"))
    );
    assert_eq!(gpui.license.as_ref(), "Apache-2.0");
    assert_eq!(
        (lua.license.as_ref(), lua.hint.as_ref().map(AsRef::as_ref)),
        ("MIT", Some("Runs the scripts plays do runs."))
    );
    assert!(
        lua.texts
            .iter()
            .any(|text| text.contains("Lua.org, PUC-Rio"))
    );

    let plays = license();
    assert_eq!(plays.license.as_ref(), env!("CARGO_PKG_LICENSE"));
    assert_eq!(plays.texts, [SharedString::from(LICENSE)]);
    assert!(
        LICENSE.contains("\nCopyright (c) 2026 2dubs LLC\n"),
        "Plays' copyright holder"
    );
}

#[test]
fn ffmpeg_is_credited_as_it_runs_whatever_it_was_built_with() {
    let another: &'static Credit = Box::leak(Box::new(Credit {
        name: "FFmpeg".into(),
        version: "1.0".into(),
        license: "GPL-3.0-or-later".into(),
        url: "https://ffmpeg.org".into(),
        source: "https://ffmpeg.org/releases/ffmpeg-1.0.tar.xz".into(),
        texts: Vec::new(),
    }));
    let entry = credit(another, &[]);
    assert_eq!(entry.version, ffmpeg_version());
    assert_ne!(entry.version.as_ref(), "1.0");
    assert_eq!(entry.license.as_ref(), ffmpeg::util::license());
}

#[test]
fn where_updating_is_is_said_with_what_comes_next() {
    let found = Found {
        version: updater::Version::new(9, 0, 0),
        releases: Vec::new(),
        size: 12_345_678,
    };
    let version = updates::version();
    let says = |state: State| said(&state);
    assert_eq!(
        says(State::Off(
            "this build of Plays doesn't update itself".into()
        )),
        (
            "Plays doesn't update itself here".into(),
            Some("This build of Plays doesn't update itself".into())
        )
    );
    assert_eq!(
        says(State::UpToDate),
        (format!("Plays {version} is up to date"), None)
    );
    assert_eq!(
        says(State::Downloading {
            found: found.clone(),
            done: 2_500_000
        }),
        (
            "Downloading Plays 9.0.0…".into(),
            Some("2.5 MB of 12.3 MB".into())
        )
    );
    assert_eq!(
        says(State::Ready(found.clone())).0,
        "Plays 9.0.0 is ready to install"
    );
    assert_eq!(says(State::Installed(found)).0, "Plays 9.0.0 is installed");
    assert_eq!(
        says(State::Failed(
            "can't update: the feed is out of date".into()
        )),
        (
            "Plays couldn't update".into(),
            Some("Can't update: the feed is out of date".into())
        )
    );
}
