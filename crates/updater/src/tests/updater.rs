use std::collections::BTreeMap;

use minisign::KeyPair;

use super::*;
use crate::feed::tests::{key, keys, lines_of, sign, version};
use crate::feed::{Download, header_line, join};
use crate::support::{Answer, Scratch, Server, binary};

const PLATFORM: &str = "test-arm";

/// Version 0.1.0 installed at `install`, and a feed served with 0.2.0,
/// whole and as a delta from 0.1.0, signed with `key`.
struct Setup {
    scratch: Scratch,
    server: Server,
    install: &'static str,
    old: Vec<u8>,
    new: Vec<u8>,
    key: KeyPair,
}

impl Setup {
    /// A single binary installed at `bin/plays`.
    fn new() -> Self {
        Self::at("bin/plays")
    }

    /// Installed at `install`: an app bundle if it's a `.app`.
    fn at(install: &'static str) -> Self {
        let old = binary(1, 200_000);
        let mut new = old.clone();
        new[1_000..1_100].fill(7);
        let setup = Self {
            scratch: Scratch::new(),
            server: Server::new(),
            install,
            old,
            new,
            key: key(),
        };
        setup.put(&setup.scratch.join(install), &setup.old);
        let made = setup.scratch.join("made/new");
        setup.put(&made, &setup.new);
        let base = setup.scratch.join(install);
        let whole = setup.scratch.join("made/whole.update");
        let delta = setup.scratch.join("made/delta.update");
        package::make(&made, None, "0.2.0", 3, &whole).unwrap();
        package::make(&made, Some((&base, "0.1.0")), "0.2.0", 3, &delta).unwrap();
        setup
            .server
            .set("whole.update", Answer::Ok(fs::read(&whole).unwrap()));
        setup
            .server
            .set("delta.update", Answer::Ok(fs::read(&delta).unwrap()));
        setup.serve_feed(&setup.key);
        setup
    }

    fn is_bundle(&self) -> bool {
        self.install.ends_with(".app")
    }

    /// The executable's `bytes` put at `path`, in a bundle if it is one.
    fn put(&self, path: &Path, bytes: &[u8]) {
        let exe = if self.is_bundle() {
            path.join("Contents/MacOS/plays")
        } else {
            path.to_owned()
        };
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(exe, bytes).unwrap();
    }

    /// The installed executable's bytes.
    fn installed(&self) -> Vec<u8> {
        let path = self.scratch.join(self.install);
        fs::read(if self.is_bundle() {
            path.join("Contents/MacOS/plays")
        } else {
            path
        })
        .unwrap()
    }

    fn file(&self, name: &str) -> feed::File {
        let bytes = fs::read(self.scratch.join(&format!("made/{name}"))).unwrap();
        feed::File {
            url: name.into(),
            size: bytes.len() as u64,
            sha256: hash::of(&bytes),
        }
    }

    /// The feed, signed with `key`: 0.2.0, with this platform's downloads,
    /// then 0.1.0.
    fn serve_feed(&self, key: &KeyPair) {
        self.serve_feed_after(key, Vec::new());
    }

    /// The feed, signed with `key`: `newer`, then 0.2.0, with this
    /// platform's downloads, then 0.1.0.
    fn serve_feed_after(&self, key: &KeyPair, newer: Vec<Release>) {
        let mut latest = Release {
            version: version("0.2.0"),
            published: now(),
            notes: "## 0.2.0\n\n- Plays more.".into(),
            downloads: BTreeMap::new(),
        };
        let download = Download {
            whole: self.file("whole.update"),
            deltas: BTreeMap::from([("0.1.0".to_owned(), self.file("delta.update"))]),
        };
        latest.downloads.insert(PLATFORM.into(), download);
        // 0.1.0 was this platform's too; its download isn't served.
        let earlier = Release {
            version: version("0.1.0"),
            published: now() - 86_400,
            notes: "## 0.1.0".into(),
            downloads: BTreeMap::from([(
                PLATFORM.to_owned(),
                Download {
                    whole: self.file("whole.update"),
                    deltas: BTreeMap::new(),
                },
            )]),
        };
        let releases: Vec<Release> = newer.into_iter().chain([latest, earlier]).collect();
        let lines = lines_of(&releases);
        let header = header_line("plays", now(), now() + 3_600, &lines[0]).unwrap();
        let feed = join(&sign(key, &header), &header, &lines);
        self.server.set("feed.jsonl", Answer::Ok(feed.into_bytes()));
    }

    fn config(&self, running: &str) -> Config {
        Config {
            app: "plays".into(),
            version: version(running),
            feed: self.server.url("feed.jsonl"),
            keys: keys(&self.key),
            platform: PLATFORM.into(),
            downloads: self.scratch.join("downloads"),
        }
    }

    fn updater(&self, running: &str) -> Updater {
        Updater::new(self.config(running))
    }

    fn install(&self) -> Install {
        Install::at(self.scratch.join(self.install))
    }

    /// The update found for `running`, downloaded and staged, stopped if
    /// `cancel`: how far each download was told it had come, and how it
    /// went.
    fn download(&self, running: &str, cancel: bool) -> (Vec<Progress>, Result<Staged>) {
        let updater = self.updater(running);
        let found = updater.check().unwrap().unwrap();
        let mut told = Vec::new();
        let staged = updater.download(
            &found,
            &self.install(),
            &mut |progress| told.push(progress),
            &AtomicBool::new(cancel),
        );
        (told, staged)
    }

    /// What's in the install's folder, and in the downloads'.
    fn leftovers(&self) -> (Vec<String>, Vec<String>) {
        let folder = Path::new(self.install).parent().unwrap().to_str().unwrap();
        let downloads = if self.scratch.join("downloads").exists() {
            self.scratch.names("downloads")
        } else {
            Vec::new()
        };
        (self.scratch.names(folder), downloads)
    }
}

fn versions(found: &Found) -> Vec<String> {
    found
        .releases
        .iter()
        .map(|release| release.version.to_string())
        .collect()
}

#[test]
fn an_update_downloads_as_a_delta_and_swaps_in() {
    let setup = Setup::new();
    let found = setup.updater("0.1.0").check().unwrap().unwrap();
    assert_eq!(found.version, version("0.2.0"));
    assert_eq!(versions(&found), ["0.2.0"]);
    assert_eq!(found.releases[0].notes, "## 0.2.0\n\n- Plays more.");
    assert!(found.has_delta());
    let delta = setup.file("delta.update").size;
    assert_eq!(found.size(), delta);
    assert!(delta * 10 < setup.file("whole.update").size);

    let (told, staged) = setup.download("0.1.0", false);
    let staged = staged.unwrap();
    assert_eq!(
        told.last(),
        Some(&Progress {
            done: delta,
            total: delta
        })
    );
    assert_eq!(
        setup.server.asked(),
        ["feed.jsonl", "feed.jsonl", "delta.update"]
    );
    assert_eq!(staged.version(), &version("0.2.0"));
    assert_eq!(staged.install_path(), setup.scratch.join("bin/plays"));
    assert_eq!(setup.installed(), setup.old, "not until it's installed");

    staged.install().unwrap();
    assert_eq!(setup.installed(), setup.new);
    assert_eq!(setup.leftovers(), (vec!["plays".to_owned()], vec![]));
}

#[test]
fn a_delta_that_doesnt_apply_to_the_install_falls_back_to_the_whole_update() {
    let setup = Setup::new();
    setup.put(&setup.scratch.join("bin/plays"), &binary(9, 200_000));
    let (told, staged) = setup.download("0.1.0", false);
    staged.unwrap().install().unwrap();
    assert_eq!(setup.installed(), setup.new);
    assert_eq!(
        setup.server.asked(),
        ["feed.jsonl", "delta.update", "whole.update"]
    );
    let whole = setup.file("whole.update").size;
    assert_eq!(
        told.last(),
        Some(&Progress {
            done: whole,
            total: whole
        })
    );
    assert_eq!(setup.leftovers(), (vec!["plays".to_owned()], vec![]));
}

#[test]
fn a_delta_that_doesnt_come_falls_back_to_the_whole_update() {
    let setup = Setup::new();
    setup.server.set("delta.update", Answer::Status(404));
    setup.download("0.1.0", false).1.unwrap().install().unwrap();
    assert_eq!(setup.installed(), setup.new);
    assert_eq!(
        setup.server.asked(),
        ["feed.jsonl", "delta.update", "whole.update"]
    );
}

#[test]
fn with_no_delta_from_the_running_version_the_whole_update_comes() {
    let setup = Setup::new();
    let found = setup.updater("0.0.9").check().unwrap().unwrap();
    assert_eq!(versions(&found), ["0.2.0", "0.1.0"]);
    assert!(!found.has_delta());
    assert_eq!(found.size(), setup.file("whole.update").size);
    setup.download("0.0.9", false).1.unwrap().install().unwrap();
    assert_eq!(setup.installed(), setup.new);
    assert_eq!(
        setup.server.asked(),
        ["feed.jsonl", "feed.jsonl", "whole.update"]
    );
}

#[test]
fn the_latest_version_running_finds_nothing() {
    let setup = Setup::new();
    assert_eq!(setup.updater("0.2.0").check().unwrap(), None);
    assert_eq!(setup.updater("0.3.0").check().unwrap(), None);
}

#[test]
fn a_feed_with_nothing_for_this_platform_has_no_update_for_it() {
    let setup = Setup::new();
    let updater = Updater::new(Config {
        platform: "linux-riscv64".into(),
        ..setup.config("0.1.0")
    });
    assert_eq!(updater.check().unwrap(), None);
}

#[test]
fn a_release_for_other_platforms_alone_is_passed_over() {
    let setup = Setup::new();
    let fix = Release {
        version: version("0.2.1"),
        published: now(),
        notes: "## 0.2.1\n\n- A fix for another OS.".into(),
        downloads: BTreeMap::from([(
            "other-os".to_owned(),
            Download {
                whole: setup.file("whole.update"),
                deltas: BTreeMap::new(),
            },
        )]),
    };
    setup.serve_feed_after(&setup.key, vec![fix]);

    let found = setup.updater("0.1.0").check().unwrap().unwrap();
    assert_eq!(found.version, version("0.2.0"));
    assert_eq!(
        versions(&found),
        ["0.2.0"],
        "0.2.1's notes aren't this OS's"
    );
    assert!(found.has_delta());
    let (_, staged) = setup.download("0.1.0", false);
    staged.unwrap().install().unwrap();
    assert_eq!(setup.installed(), setup.new);

    assert_eq!(setup.updater("0.2.0").check().unwrap(), None);
}

#[test]
fn a_feed_signed_with_another_key_isnt_taken() {
    let setup = Setup::new();
    setup.serve_feed(&key());
    assert!(matches!(
        setup.updater("0.1.0").check(),
        Err(Error::Signature(_))
    ));
}

#[test]
fn a_feed_not_served_says_how_it_was_answered() {
    let setup = Setup::new();
    setup.server.set("feed.jsonl", Answer::Status(502));
    let checked = setup.updater("0.1.0").check();
    assert!(matches!(&checked, Err(err @ Error::Http { status: 502, .. }) if err.is_transient()));
}

#[test]
fn a_whole_update_that_isnt_what_the_feed_says_stages_nothing() {
    let setup = Setup::new();
    let mut whole = fs::read(setup.scratch.join("made/whole.update")).unwrap();
    whole[100] ^= 1;
    setup.server.set("whole.update", Answer::Ok(whole));
    let staged = setup.download("0.0.9", false).1;
    assert!(matches!(staged, Err(Error::Mismatch(_))), "{staged:?}");
    assert_eq!(setup.installed(), setup.old);
    assert_eq!(setup.leftovers(), (vec!["plays".to_owned()], vec![]));
}

#[test]
fn a_download_stopped_stages_nothing_and_downloads_nothing_else() {
    let setup = Setup::new();
    let staged = setup.download("0.1.0", true).1;
    assert!(matches!(staged, Err(Error::Cancelled)), "{staged:?}");
    assert_eq!(setup.server.asked(), ["feed.jsonl", "delta.update"]);
    assert_eq!(setup.installed(), setup.old);
    assert_eq!(setup.leftovers(), (vec!["plays".to_owned()], vec![]));
}

#[test]
fn an_update_thrown_away_leaves_the_install_as_it_was() {
    let setup = Setup::new();
    let staged = setup.download("0.1.0", false).1.unwrap();
    staged.discard();
    assert_eq!(setup.installed(), setup.old);
    assert_eq!(setup.leftovers().0, ["plays"]);
}

#[test]
fn an_update_downloaded_again_is_taken_from_the_downloads() {
    let setup = Setup::new();
    setup.download("0.1.0", false).1.unwrap().discard();
    setup.download("0.1.0", false).1.unwrap().install().unwrap();
    assert_eq!(setup.installed(), setup.new);
    let asked = setup.server.asked();
    assert_eq!(
        asked.iter().filter(|path| *path == "delta.update").count(),
        1,
        "{asked:?}"
    );
}

#[cfg(unix)]
#[test]
fn an_install_in_a_folder_that_cant_be_written_to_isnt_updated() {
    use std::os::unix::fs::PermissionsExt;

    let setup = Setup::new();
    let bin = setup.scratch.join("bin");
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o555)).unwrap();
    let staged = setup.download("0.1.0", false).1;
    assert!(
        matches!(&staged, Err(Error::Permission(folder)) if *folder == bin),
        "{staged:?}"
    );
    assert_eq!(setup.server.asked(), ["feed.jsonl"]);
}

#[test]
fn an_app_bundle_updates_as_a_delta() {
    let setup = Setup::at("Applications/Plays.app");
    let staged = setup.download("0.1.0", false).1;
    if cfg!(target_os = "macos") {
        // Its code signature, which it hasn't, doesn't hold.
        assert!(matches!(staged, Err(Error::CodeSignature(_))), "{staged:?}");
        assert_eq!(setup.installed(), setup.old);
        assert_eq!(setup.leftovers().0, ["Plays.app"]);
    } else {
        staged.unwrap().install().unwrap();
        assert_eq!(setup.installed(), setup.new);
        assert_eq!(setup.leftovers(), (vec!["Plays.app".to_owned()], vec![]));
    }
}
