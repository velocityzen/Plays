//! Linux: the app the desktop opens each MIME type with, as `xdg-mime`
//! says, and Plays' desktop file made it, which writes
//! `~/.config/mimeapps.list` and asks nothing. Built for tests everywhere,
//! with a stand-in for `xdg-mime`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use file_types::{FORMATS, Format};
#[cfg(target_os = "linux")]
use gpui::{App, AppContext as _, Task};

use super::{Made, Opener};

/// Plays' desktop file, as MIME defaults name it.
const DESKTOP_FILE: &str = "plays.desktop";

/// `xdg-mime`, and the folders desktop files are found in.
pub struct Xdg {
    pub program: PathBuf,
    /// Each data folder's `applications`: `$XDG_DATA_HOME`'s, then
    /// `$XDG_DATA_DIRS`'.
    pub applications: Vec<PathBuf>,
}

impl Xdg {
    /// As the environment has them: `xdg-mime` on the `PATH`, and the
    /// data folders, by default `~/.local/share`, `/usr/local/share` and
    /// `/usr/share`.
    #[cfg(target_os = "linux")]
    fn from_env() -> Result<Self, String> {
        use std::env;

        let program = env::var_os("PATH")
            .iter()
            .flat_map(env::split_paths)
            .map(|folder| folder.join("xdg-mime"))
            .find(|program| program.is_file())
            .ok_or("xdg-mime isn't installed: it comes with xdg-utils")?;
        let home = env::var_os("XDG_DATA_HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| Path::new(&home).join(".local/share")));
        let dirs = env::var_os("XDG_DATA_DIRS")
            .filter(|dirs| !dirs.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let applications = home
            .into_iter()
            .chain(env::split_paths(&dirs))
            .map(|folder| folder.join("applications"))
            .collect();
        Ok(Self {
            program,
            applications,
        })
    }

    /// The formats Plays offers itself for on Linux: those with a MIME
    /// type.
    fn offered() -> impl Iterator<Item = &'static Format> {
        FORMATS.iter().filter(|format| !format.mime.is_empty())
    }

    /// The desktop file `id` (`vlc.desktop`), where one by that name is.
    fn desktop_file(&self, id: &str) -> Option<PathBuf> {
        self.applications
            .iter()
            .map(|folder| folder.join(id))
            .find(|path| path.is_file())
    }

    /// Why Plays can't be made the default player: its desktop file isn't
    /// installed, so there's nothing for the MIME types to name.
    pub fn cant(&self) -> Option<String> {
        self.desktop_file(DESKTOP_FILE).is_none().then(|| {
            "Plays' desktop file isn't installed: script/install-linux installs it".to_owned()
        })
    }

    /// The desktop file that opens files of the MIME type `mime` now, if
    /// one does.
    fn default_for(&self, mime: &str) -> Result<Option<String>, String> {
        let output = Command::new(&self.program)
            .args(["query", "default", mime])
            .output()
            .map_err(|err| format!("can't run xdg-mime: {err}"))?;
        if !output.status.success() {
            return Err(failed(&output.stderr));
        }
        let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok((!id.is_empty()).then_some(id))
    }

    /// What the desktop file `id` calls its app: its `Name`, or else its
    /// id without `.desktop`.
    fn name_of(&self, id: &str) -> String {
        let named = self.desktop_file(id).and_then(|path| desktop_name(&path));
        named.unwrap_or_else(|| id.trim_end_matches(".desktop").to_owned())
    }

    /// The app that opens `format`'s files now: Plays if it does for each
    /// of its MIME types, or else the app that opens the first it doesn't.
    pub fn opener(&self, format: &'static Format) -> Result<Opener, String> {
        let mut other = None;
        for mime in format.mime {
            let id = self.default_for(mime)?;
            if id.as_deref() != Some(DESKTOP_FILE) {
                other = Some(id);
                break;
            }
        }
        Ok(Opener {
            format,
            app: match &other {
                Some(id) => id.as_deref().map(|id| self.name_of(id)),
                None => Some("Plays".to_owned()),
            },
            plays: other.is_none(),
        })
    }

    /// Each format Plays opens, and its app now.
    pub fn read(&self) -> Result<Vec<Opener>, String> {
        Self::offered().map(|format| self.opener(format)).collect()
    }

    /// Make Plays open `formats`, all at once; then what each opens in now
    /// says what came of it.
    pub fn make(&self, formats: &[&'static Format]) -> Made {
        let mime = formats.iter().flat_map(|format| format.mime);
        let ran = Command::new(&self.program)
            .args(["default", DESKTOP_FILE])
            .args(mime)
            .output();
        let failed = match ran {
            Ok(output) if output.status.success() => None,
            Ok(output) => Some(failed(&output.stderr)),
            Err(err) => Some(format!("can't run xdg-mime: {err}")),
        };
        let mut made = Made::default();
        for &format in formats {
            match self.opener(format) {
                Ok(now) if now.plays => made.set.push(format),
                Ok(now) => {
                    let app = now.app.unwrap_or_else(|| "no app".to_owned());
                    let why = failed.clone();
                    made.not_set
                        .push((format, why.unwrap_or(format!("{app} still opens them"))));
                }
                Err(why) => made.not_set.push((format, failed.clone().unwrap_or(why))),
            }
        }
        made
    }
}

/// What `xdg-mime` said it couldn't do.
fn failed(stderr: &[u8]) -> String {
    let said = String::from_utf8_lossy(stderr).trim().to_owned();
    if said.is_empty() {
        "xdg-mime failed".to_owned()
    } else {
        format!("xdg-mime: {said}")
    }
}

/// The `Name` of the app in the desktop file at `path`, from its
/// `[Desktop Entry]` group.
fn desktop_name(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(name) = line.strip_prefix("Name=") {
            return Some(name.trim().to_owned());
        }
    }
    None
}

/// Why Plays can't be made the default player here, if it can't.
#[cfg(target_os = "linux")]
pub fn cant() -> Option<String> {
    match Xdg::from_env() {
        Ok(xdg) => xdg.cant(),
        Err(why) => Some(why),
    }
}

/// Each format Plays opens, and its app now: `xdg-mime` run once a MIME
/// type, off the main thread.
#[cfg(target_os = "linux")]
pub fn read(cx: &mut App) -> Task<super::Openers> {
    cx.background_spawn(async { Xdg::from_env()?.read() })
}

/// Make Plays open `formats`, off the main thread.
#[cfg(target_os = "linux")]
pub fn make(formats: Vec<&'static Format>, cx: &mut App) -> Task<Made> {
    if cfg!(test) {
        panic!("a test would change the apps this computer opens files with");
    }
    cx.background_spawn(async move {
        match Xdg::from_env() {
            Ok(xdg) => xdg.make(&formats),
            Err(why) => Made {
                set: Vec::new(),
                not_set: formats
                    .into_iter()
                    .map(|format| (format, why.clone()))
                    .collect(),
            },
        }
    })
}
