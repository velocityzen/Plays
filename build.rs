//! Links the static FFmpeg that `script/build-ffmpeg` builds with what it
//! needs in turn: dav1d, zlib and iconv, and macOS's frameworks or Linux's
//! OpenSSL and hardware decoding libraries. ffmpeg-sys-next links FFmpeg's
//! own libraries; the rest is read from FFmpeg's pkg-config files. Also
//! dates the build, for About.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// FFmpeg's libraries the app uses, each before the ones it uses, the
/// order GNU ld needs.
const LIBRARIES: [&str; 5] = ["avformat", "avcodec", "swscale", "swresample", "avutil"];

fn main() {
    date_the_build();
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    let Some(dir) = env::var_os("FFMPEG_DIR").map(PathBuf::from) else {
        // ffmpeg-sys-next finds FFmpeg with pkg-config, which links it all.
        return;
    };
    let lib = dir.join("lib");
    println!("cargo:rerun-if-changed={}", lib.join("pkgconfig").display());
    if !dir.join("include/libavcodec/avcodec.h").exists() {
        panic!(
            "FFmpeg isn't in {}: build it with script/build-ffmpeg",
            dir.display()
        );
    }
    // Shared libraries bring what they need with them.
    let shared = ["libavcodec.dylib", "libavcodec.so"]
        .iter()
        .any(|name| lib.join(name).exists());
    if shared || !lib.join("libavcodec.a").exists() {
        return;
    }

    let apple = env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple");
    let mut args: Vec<Vec<String>> = Vec::new();
    for name in LIBRARIES {
        args.extend(libs(&lib.join(format!("pkgconfig/lib{name}.pc")), &dir));
    }
    // Each argument where it last appears, after everything that needs it.
    let mut seen = Vec::new();
    for arg in args.into_iter().rev() {
        if !seen.contains(&arg) {
            seen.push(arg);
        }
    }
    seen.reverse();
    // Link arguments go after every library on the command line, so GNU ld
    // links FFmpeg's libraries again, in order, then what they need. Apple's
    // linker doesn't mind the order, and warns about libraries given twice,
    // as FFmpeg's are and libSystem's (threads and math) would be.
    for arg in seen {
        let ffmpeg = LIBRARIES.iter().any(|name| arg == [format!("-l{name}")]);
        let system = arg == ["-lm"] || arg == ["-pthread"];
        if !(apple && (ffmpeg || system)) {
            for part in arg {
                println!("cargo:rustc-link-arg={part}");
            }
        }
    }
    link_compiler_runtime();
}

/// The C compiler's runtime library, which FFmpeg and dav1d call into: on
/// macOS clang's, for `__builtin_available` checking the macOS version, and
/// on Linux libgcc, for the atomics GCC builds as calls on ARM64. The
/// compiler links it itself, but Rust links with `-nodefaultlibs`, so name it
/// here, after the libraries that need it.
fn link_compiler_runtime() {
    let compiler = env::var("CC").unwrap_or_else(|_| "cc".into());
    let output = Command::new(&compiler)
        .arg("-print-libgcc-file-name")
        .output()
        .unwrap_or_else(|err| panic!("running {compiler}: {err}"));
    let runtime = String::from_utf8_lossy(&output.stdout);
    let runtime = Path::new(runtime.trim());
    if !runtime.is_file() {
        panic!(
            "{compiler}'s runtime library isn't at {}",
            runtime.display()
        );
    }
    println!("cargo:rustc-link-arg={}", runtime.display());
}

/// The linker arguments a pkg-config file gives in `Libs` and `Libs.private`,
/// its variables filled in, with `-framework` and its name kept together.
/// The file names the folder FFmpeg was built in, which may have moved since,
/// so paths in that folder are taken to be in `prefix` instead.
fn libs(path: &Path, prefix: &Path) -> Vec<Vec<String>> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|err| panic!("reading {}: {err}", path.display()));
    let prefix = prefix.to_string_lossy();
    let mut built_in: Option<String> = None;
    let mut variables = HashMap::new();
    let mut args: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        let Some(split) = line.find([':', '=']) else {
            continue;
        };
        let (key, value) = (line[..split].trim(), line[split + 1..].trim());
        let mut value = expand(value, &variables);
        if let Some(built_in) = &built_in {
            value = value.replace(built_in.as_str(), &prefix);
        }
        if line[split..].starts_with('=') {
            if key == "prefix" {
                built_in = Some(std::mem::replace(&mut value, prefix.to_string()));
            }
            variables.insert(key.to_owned(), value);
        } else if key == "Libs" || key == "Libs.private" {
            let mut words = value.split_whitespace();
            while let Some(word) = words.next() {
                let mut arg = vec![word.to_owned()];
                if word == "-framework" {
                    arg.extend(words.next().map(str::to_owned));
                }
                args.push(arg);
            }
        }
    }
    args
}

/// `value` with each `${name}` replaced by that variable.
fn expand(value: &str, variables: &HashMap<String, String>) -> String {
    let mut expanded = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        let Some(length) = rest[start..].find('}') else {
            break;
        };
        expanded.push_str(&rest[..start]);
        let name = &rest[start + 2..start + length];
        expanded.push_str(variables.get(name).map_or("", String::as_str));
        rest = &rest[start + length + 1..];
    }
    expanded.push_str(rest);
    expanded
}

/// `PLAYS_BUILD_DATE` (YYYY-MM-DD, UTC): the day of `SOURCE_DATE_EPOCH` for
/// a reproducible build, else today. Dated again whenever the source changes.
fn date_the_build() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let seconds = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|epoch| epoch.parse::<i64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs() as i64)
        });
    let (year, month, day) = civil_date(seconds.div_euclid(86_400));
    println!("cargo:rustc-env=PLAYS_BUILD_DATE={year:04}-{month:02}-{day:02}");
}

/// The date `days` after 1970-01-01, in the proleptic Gregorian calendar
/// (Howard Hinnant's `civil_from_days`).
fn civil_date(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
