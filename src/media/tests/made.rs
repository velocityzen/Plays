//! Media made for the tests: kept in `plays-tests` in the temporary
//! folder, made once, and again only when made another way. Tests run at
//! once, and so do test runs, so each is made whole before it's where they
//! look.

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Where media made for the tests is kept.
pub fn kept() -> PathBuf {
    std::env::temp_dir().join("plays-tests")
}

/// A test file the `ffmpeg` tool makes.
pub fn media(name: &str, args: &[&str]) -> PathBuf {
    made_by("ffmpeg", name, args)
}

/// A test file `program` makes, as `ffmpeg` does. The test fails if it
/// can't.
pub fn made_by(program: &str, name: &str, args: &[&str]) -> PathBuf {
    made_from(program, name, &[], args)
}

/// As `made_by`, from `inputs` too, each named and what it holds.
pub fn made_from(program: &str, name: &str, inputs: &[(&str, &str)], args: &[&str]) -> PathBuf {
    ffmpeg_next::init().unwrap();
    made_in(
        &kept(),
        name,
        &recipe(program, inputs, args),
        running(program, name, inputs, args),
    )
}

/// How `program` makes something from `inputs` and `args`: made another
/// way, it's made again.
fn recipe(program: &str, inputs: &[(&str, &str)], args: &[&str]) -> String {
    format!("{program} {args:?} {inputs:?}")
}

/// What runs `program` to make `name`. Its `inputs` are written where it's
/// made, for it alone, and an argument naming one is given its path.
fn running<'a>(
    program: &'a str,
    name: &'a str,
    inputs: &'a [(&str, &str)],
    args: &'a [&str],
) -> impl FnOnce(&Path) -> Result<(), String> + 'a {
    move |out| {
        let folder = out.parent().unwrap();
        for (input, contents) in inputs {
            fs::write(folder.join(input), contents).map_err(|err| err.to_string())?;
        }
        let args = args.iter().map(|&arg| {
            if inputs.iter().any(|(input, _)| *input == arg) {
                folder.join(arg).into_os_string()
            } else {
                arg.into()
            }
        });
        let output = Command::new(program)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .arg(out)
            .output()
            .map_err(|err| {
                format!(
                    "`{program}` isn't installed ({err}), and the media tests make their files \
                     with it: install it, or leave them out with `cargo test -- --skip media::tests`"
                )
            })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "`{program}` couldn't make {name}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }
}

/// `name` in `folder`, made by `make` unless it's there, made by the same
/// `recipe`. The test fails if it can't be.
///
/// `make` is told where to make it: in a folder of its own, under the same
/// name, as ffmpeg goes by the extension. It's then put in place whole, so
/// no test opens one half made, and one cut short is never kept. One test
/// makes it while the others wait; `name.made` beside it says how.
pub fn made_in(
    folder: &Path,
    name: &str,
    recipe: &str,
    make: impl FnOnce(&Path) -> Result<(), String>,
) -> PathBuf {
    let path = folder.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut how = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_added_extension("made"))
        .unwrap();
    how.lock().unwrap();
    let mut was = String::new();
    how.read_to_string(&mut was).unwrap();
    if was == recipe && path.exists() {
        return path;
    }
    let making = making(folder);
    let into = making.join(path.file_name().unwrap());
    let made = make(&into).map(|()| put(&into, &path));
    fs::remove_dir_all(&making).unwrap();
    if let Err(err) = made {
        panic!("{err}");
    }
    how.set_len(0).unwrap();
    how.rewind().unwrap();
    how.write_all(recipe.as_bytes()).unwrap();
    path
}

/// A folder in `folder` to make something in, of its own.
fn making(folder: &Path) -> PathBuf {
    static MADE: AtomicUsize = AtomicUsize::new(0);
    let making = folder.join(".making").join(format!(
        "{}-{}",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    // One left by a run cut short, with the same number.
    fs::remove_dir_all(&making).ok();
    fs::create_dir_all(&making).unwrap();
    making
}

/// What was `made` put at `path`, in place of what was there: a folder
/// can't be renamed over another.
fn put(made: &Path, path: &Path) {
    if path.is_dir() {
        fs::remove_dir_all(path).unwrap();
    }
    fs::rename(made, path).unwrap();
}

/// `clip` in a folder of `test`'s own, with `files` beside it, each named
/// and what it holds: no other test changes them, nor another run. The
/// clip is linked there, not made again.
pub fn in_own_folder(clip: &Path, test: &str, files: &[(&str, impl AsRef<[u8]>)]) -> PathBuf {
    let folder = crate::files::test_folder(test);
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(clip.file_name().unwrap());
    fs::hard_link(clip, &path).unwrap();
    for (name, contents) in files {
        fs::write(folder.join(name), contents).unwrap();
    }
    path
}

/// Without the tool that makes their media, the tests fail rather than
/// pass having checked nothing.
#[test]
#[should_panic(expected = "isn't installed")]
fn media_that_cant_be_made_fails_the_test() {
    made_by("plays-no-such-tool", "never-made.mp4", &[]);
}

/// So do media the tool fails to make, as one without an encoder would.
#[test]
#[should_panic(expected = "couldn't make never-made.mkv")]
fn media_the_tool_fails_to_make_fails_the_test() {
    made_by("false", "never-made.mkv", &[]);
}

/// What makes `text`, as `made_in` asks.
fn writing(text: &'static str) -> impl FnOnce(&Path) -> Result<(), String> {
    move |out| fs::write(out, text).map_err(|err| err.to_string())
}

/// What's made goes where tests look only once it's whole, and a test
/// wanting it as it's made waits for it, rather than make it too.
#[test]
fn made_once_and_whole_as_others_wait() {
    use std::sync::mpsc;
    let folder = &crate::files::test_folder("made-once");
    let path = folder.join("clip.txt");
    let (halfway, at_halfway) = mpsc::channel();
    let (go_on, going_on) = mpsc::channel();
    std::thread::scope(|scope| {
        let first = scope.spawn(move || {
            made_in(folder, "clip.txt", "whole", move |out| {
                fs::write(out, "half").unwrap();
                halfway.send(()).unwrap();
                going_on.recv().unwrap();
                fs::write(out, "whole").map_err(|err| err.to_string())
            })
        });
        at_halfway.recv().unwrap();
        let half_made_seen = path.exists();
        let second =
            scope.spawn(|| made_in(folder, "clip.txt", "whole", |_| Err("made twice".into())));
        // Long enough for it to wait.
        std::thread::sleep(std::time::Duration::from_millis(50));
        go_on.send(()).unwrap();
        for made in [first, second] {
            assert_eq!(fs::read_to_string(made.join().unwrap()).unwrap(), "whole");
        }
        assert!(!half_made_seen, "half made where tests look");
    });
}

/// Made another way, or not said how, as a run cut short before left it,
/// it's made again, and so is one gone since; made the same way, it isn't.
#[test]
fn made_another_way_its_made_again() {
    let folder = crate::files::test_folder("made-again");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("clip.txt"), "cut short").unwrap();
    let path = made_in(&folder, "clip.txt", "the first way", writing("first"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "first");
    made_in(&folder, "clip.txt", "another", writing("another"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "another");
    fs::remove_file(&path).unwrap();
    made_in(&folder, "clip.txt", "another", writing("again"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "again");
    made_in(&folder, "clip.txt", "another", |_| Err("made again".into()));
}

/// One cut short is never kept: none is where tests look, nor where it
/// was made, and the next test makes it again.
#[test]
fn one_cut_short_is_never_kept() {
    let folder = crate::files::test_folder("cut-short");
    let cut = std::panic::catch_unwind(|| {
        made_in(&folder, "clip.txt", "whole", |out| {
            fs::write(out, "half").unwrap();
            Err("cut short".into())
        })
    });
    assert!(cut.is_err());
    assert!(!folder.join("clip.txt").exists());
    assert_eq!(fs::read_dir(folder.join(".making")).unwrap().count(), 0);
    let path = made_in(&folder, "clip.txt", "whole", writing("whole"));
    assert_eq!(fs::read_to_string(path).unwrap(), "whole");
}

/// A folder made another way takes the place of the last, all of it.
#[test]
fn a_folder_made_another_way_takes_the_place_of_the_last() {
    let folder = crate::files::test_folder("folder-again");
    let holding = |file: &'static str| {
        move |out: &Path| {
            fs::create_dir(out)
                .and_then(|()| fs::write(out.join(file), ""))
                .map_err(|err| err.to_string())
        }
    };
    made_in(&folder, "hls", "one", holding("one.ts"));
    let path = made_in(&folder, "hls", "two", holding("two.ts"));
    let held: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(held, ["two.ts"]);
}

/// Inputs are written for it alone, where it's made, and are part of how
/// it's made: others make it again.
#[test]
fn its_inputs_are_its_own_and_others_make_it_again() {
    let folder = crate::files::test_folder("made-from");
    let subtitles = |text: &str| {
        let srt = format!("1\n00:00:01,000 --> 00:00:02,000\n{text}\n");
        let inputs = [("in.srt", srt.as_str())];
        let args = ["-i", "in.srt"];
        let path = made_in(
            &folder,
            "out.srt",
            &recipe("ffmpeg", &inputs, &args),
            running("ffmpeg", "out.srt", &inputs, &args),
        );
        fs::read_to_string(path).unwrap()
    };
    assert!(subtitles("Hello").contains("Hello"));
    assert!(subtitles("Bonjour").contains("Bonjour"));
    assert!(!folder.join("in.srt").exists());
}
