//! Links the static FFmpeg that `script/build-ffmpeg` builds with what it
//! needs in turn: dav1d, zlib and iconv, and macOS's frameworks or Linux's
//! OpenSSL and hardware decoding libraries. ffmpeg-sys-next links FFmpeg's
//! own libraries; the rest is read from FFmpeg's pkg-config files.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// FFmpeg's libraries the app uses, each before the ones it uses, the
/// order GNU ld needs.
const LIBRARIES: [&str; 5] = ["avformat", "avcodec", "swscale", "swresample", "avutil"];

fn main() {
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
