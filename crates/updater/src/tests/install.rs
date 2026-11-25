use super::*;
use crate::support::Scratch;

/// `os`'s install for `exe`, with the environment's variables `env`.
fn found(os: Os, exe: &str, env: &[(&str, &str)]) -> std::result::Result<PathBuf, NotInstalled> {
    let env = |name: &str| {
        env.iter()
            .find(|(set, _)| *set == name)
            .map(|(_, value)| OsString::from(value))
    };
    Install::find(os, Path::new(exe), &env).map(|install| install.path)
}

#[test]
fn on_macos_the_install_is_the_app_bundle() {
    assert_eq!(
        found(Os::Mac, "/Applications/Plays.app/Contents/MacOS/plays", &[]),
        Ok("/Applications/Plays.app".into())
    );
    assert_eq!(
        found(
            Os::Mac,
            "/Users/me/Apps/Plays Beta.app/Contents/MacOS/plays",
            &[]
        ),
        Ok("/Users/me/Apps/Plays Beta.app".into())
    );
    for exe in [
        "/usr/local/bin/plays",
        "/Applications/Plays/Contents/MacOS/plays",
        "/Plays.app/Contents/plays",
    ] {
        assert_eq!(
            found(Os::Mac, exe, &[]),
            Err(NotInstalled::NotAnApp),
            "{exe}"
        );
    }
}

#[test]
fn on_macos_an_app_run_where_it_was_downloaded_isnt_updated() {
    let exe = "/private/var/folders/x/T/AppTranslocation/0A1B/d/Plays.app/Contents/MacOS/plays";
    assert_eq!(found(Os::Mac, exe, &[]), Err(NotInstalled::Translocated));
}

#[test]
fn on_linux_the_install_is_the_binary_or_its_appimage() {
    assert_eq!(
        found(Os::Linux, "/home/me/.local/bin/plays", &[]),
        Ok("/home/me/.local/bin/plays".into())
    );
    assert_eq!(
        found(Os::Linux, "/opt/plays/plays", &[]),
        Ok("/opt/plays/plays".into())
    );
    let appimage = [("APPIMAGE", "/home/me/Apps/Plays.AppImage")];
    assert_eq!(
        found(Os::Linux, "/tmp/.mount_PlaysAb/usr/bin/plays", &appimage),
        Ok("/home/me/Apps/Plays.AppImage".into())
    );
}

#[test]
fn on_linux_what_a_package_manager_installed_isnt_updated() {
    for exe in [
        "/usr/bin/plays",
        "/usr/local/bin/plays",
        "/nix/store/abc-plays/bin/plays",
        "/gnu/store/abc-plays/bin/plays",
    ] {
        assert_eq!(
            found(Os::Linux, exe, &[]),
            Err(NotInstalled::Managed("a package manager")),
            "{exe}"
        );
    }
    let exe = "/app/bin/plays";
    assert_eq!(
        found(Os::Linux, exe, &[("FLATPAK_ID", "app.plays")]),
        Err(NotInstalled::Managed("Flatpak"))
    );
    assert_eq!(
        found(Os::Linux, exe, &[("SNAP", "/snap/plays/1")]),
        Err(NotInstalled::Managed("Snap"))
    );
}

#[test]
fn on_windows_the_install_is_the_exe_unless_the_store_installed_it() {
    let exe = r"C:\Users\me\AppData\Local\Programs\Plays\plays.exe";
    assert_eq!(found(Os::Windows, exe, &[]), Ok(exe.into()));
    let store = r"C:\Program Files\WindowsApps\Plays_0.1.0_x64\plays.exe";
    assert_eq!(
        found(Os::Windows, store, &[]),
        Err(NotInstalled::Managed("the Microsoft Store"))
    );
}

#[test]
fn an_app_built_from_source_isnt_updated() {
    let scratch = Scratch::new();
    fs::write(scratch.join("Cargo.toml"), "[package]").unwrap();
    let debug = scratch.join("target/debug/plays");
    let bundle = scratch.join("target/release/bundle/osx/Plays.app/Contents/MacOS/plays");
    for os in [Os::Mac, Os::Linux, Os::Windows] {
        for exe in [&debug, &bundle] {
            assert_eq!(
                found(os, exe.to_str().unwrap(), &[]),
                Err(NotInstalled::BuiltFromSource),
                "{os:?} {exe:?}"
            );
        }
    }
    // A folder named target is just a folder, with no project beside it.
    fs::remove_file(scratch.join("Cargo.toml")).unwrap();
    assert_eq!(found(Os::Linux, debug.to_str().unwrap(), &[]), Ok(debug));
}

#[test]
fn leftovers_are_kept_beside_the_install() {
    let install = Install::at("/Applications/Plays.app");
    assert_eq!(
        install.staging(),
        Path::new("/Applications/.Plays.app.update")
    );
    assert_eq!(install.old(), Path::new("/Applications/.Plays.app.old"));
}

#[test]
fn an_update_swaps_in_for_a_folder() {
    let scratch = Scratch::new();
    let install = Install::at(scratch.join("Plays.app"));
    fs::create_dir_all(scratch.join("Plays.app/Contents")).unwrap();
    fs::write(scratch.join("Plays.app/Contents/version"), "0.1.0").unwrap();
    fs::create_dir_all(install.staging().join("Contents")).unwrap();
    fs::write(install.staging().join("Contents/version"), "0.2.0").unwrap();
    install.swap(&install.staging()).unwrap();
    assert_eq!(
        fs::read_to_string(scratch.join("Plays.app/Contents/version")).unwrap(),
        "0.2.0"
    );
    assert_eq!(scratch.names(""), ["Plays.app"]);
}

#[test]
fn an_update_swaps_in_for_a_binary() {
    let scratch = Scratch::new();
    let install = Install::at(scratch.join("plays"));
    fs::write(scratch.join("plays"), "0.1.0").unwrap();
    fs::write(install.staging(), "0.2.0").unwrap();
    install.swap(&install.staging()).unwrap();
    assert_eq!(fs::read_to_string(scratch.join("plays")).unwrap(), "0.2.0");
    assert_eq!(scratch.names(""), ["plays"]);
}

#[test]
fn nothing_staged_swaps_nothing() {
    let scratch = Scratch::new();
    let install = Install::at(scratch.join("plays"));
    fs::write(scratch.join("plays"), "0.1.0").unwrap();
    assert!(matches!(
        install.swap(&install.staging()),
        Err(Error::Update(_))
    ));
    assert_eq!(fs::read_to_string(scratch.join("plays")).unwrap(), "0.1.0");
}

#[test]
fn nothing_installed_is_swapped_for_nothing() {
    let scratch = Scratch::new();
    let install = Install::at(scratch.join("plays"));
    fs::write(install.staging(), "0.2.0").unwrap();
    assert!(install.swap(&install.staging()).is_err());
    assert_eq!(scratch.names(""), [".plays.update"]);
}

#[test]
fn clean_takes_the_leftovers_and_leaves_the_install() {
    let scratch = Scratch::new();
    let install = Install::at(scratch.join("Plays.app"));
    fs::create_dir_all(scratch.join("Plays.app")).unwrap();
    fs::create_dir_all(install.staging().join("Contents")).unwrap();
    fs::write(install.old(), "").unwrap();
    install.clean();
    assert_eq!(scratch.names(""), ["Plays.app"]);
    // And with none, does nothing.
    install.clean();
    assert_eq!(scratch.names(""), ["Plays.app"]);
}

#[test]
fn a_folder_that_can_be_written_to_is_left_as_it_was() {
    let scratch = Scratch::new();
    let install = Install::at(scratch.join("plays"));
    install.check_writable().unwrap();
    assert!(scratch.names("").is_empty());
}

#[cfg(unix)]
#[test]
fn a_folder_that_cant_be_written_to_says_so() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new();
    fs::create_dir(scratch.join("bin")).unwrap();
    fs::set_permissions(scratch.join("bin"), fs::Permissions::from_mode(0o555)).unwrap();
    let install = Install::at(scratch.join("bin/plays"));
    assert!(
        matches!(install.check_writable(), Err(Error::Permission(folder)) if folder == scratch.join("bin"))
    );
}
