# Contributing to Plays

Building, running from source, and testing. [README.md](README.md) says
what Plays does, [ARCHITECTURE.md](ARCHITECTURE.md) how it works.

- [Building](#building)
  - [macOS](#macos)
  - [Linux (Debian/Ubuntu)](#linux-debianubuntu)
  - [One Plays at a time](#one-plays-at-a-time)
  - [The macOS app](#the-macos-app)
  - [Licenses](#licenses)
- [Tests](#tests)
  - [Running them](#running-them)
  - [Where they live](#where-they-live)
  - [What's tested where](#whats-tested-where)
  - [Window and command tests](#window-and-command-tests)
  - [Fixtures](#fixtures)
  - [Smoke runs](#smoke-runs)
- [Conventions](#conventions)

## Building

- Rust 1.85+ (edition 2024) and `clang` (bindgen makes FFmpeg's bindings)
- `script/build-ffmpeg` builds FFmpeg 9.0.2 with dav1d once, from source,
  as static LGPL libraries in `build/ffmpeg-9.0.2`, the folder
  `.cargo/config.toml` names. A new version is named in both; the script
  stops if they don't match. It builds again only when the script changes,
  or with `--force`, and needs `pkg-config`, `ninja`, `python3`, and
  `nasm` on x86_64.
- A Cargo workspace: Plays, and its crates in `crates/`. Cargo commands at
  the top act on Plays alone; `--workspace` takes in the crates too.

### macOS

Xcode (for the Metal shader compiler), then:

```bash
brew install pkgconf ninja
script/build-ffmpeg
cargo run --release -- path/to/video.mp4 path/to/song.mp3 https://example.com/live.m3u8
```

FFmpeg uses VideoToolbox, AudioToolbox and macOS's TLS: the app needs
nothing beyond macOS. For another FFmpeg, such as Homebrew's (GPL, and
then needed to run), name the folder Homebrew keeps its version in:

```bash
FFMPEG_DIR=$(realpath "$(brew --prefix ffmpeg)") cargo build --release
```

### Linux (Debian/Ubuntu)

```bash
sudo apt install build-essential pkg-config clang libclang-dev nasm ninja-build python3 \
  libssl-dev zlib1g-dev libva-dev libvdpau-dev libx11-dev libffmpeg-nvenc-dev \
  libasound2-dev libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libvulkan1 mesa-vulkan-drivers
```

```bash
script/build-ffmpeg
cargo run --release -- path/to/video.mp4 path/to/song.mp3 https://example.com/live.m3u8
```

- FFmpeg uses the system's OpenSSL for https, and libva (VA-API) and
  libvdpau for hardware decoding: the script builds them in when their
  development packages are installed, and says which it leaves out.
- NVDEC needs only NVIDIA's driver, at run time. The stream server encodes
  with VA-API, or NVENC, which needs `libffmpeg-nvenc-dev` to build and
  NVIDIA's driver to run.
- gpui renders with Vulkan: a working Vulkan driver is required. Drivers
  for hardware decoding: [README.md](README.md#linux).

### One Plays at a time

- With a Plays running, installed or another build, `cargo run -- FILE`
  hands `FILE` to it and exits: quit it first to run your build.
- Tests and `smoke` builds take no socket and run beside it, unless
  `PLAYS_SOCKET` names one.
- The command line (`cargo run -- ls`) asks whichever Plays runs, and
  starts your build if none does.

### The macOS app

```bash
script/bundle-macos
```

- Makes `target/release/Plays.app`, which runs on any Mac, for **Open
  With** and files dropped on the Dock icon
- Builds FFmpeg if it isn't built, writes the credits again, and always
  uses the app's own FFmpeg, whatever `FFMPEG_DIR` says
- Compiles the icon, `assets/AppIcon.icon`, which takes Xcode 26 or later,
  and signs the bundle ad hoc
- Finder offers it under Open With once it's in `/Applications` or has
  been opened once

### Licenses

- Plays is MIT (`LICENSE`), built with FFmpeg (LGPL 2.1 or later) and
  dav1d (BSD 2-Clause), from unchanged source, GPUI (Apache 2.0), Lua
  (MIT), and the crates in `Cargo.lock`.
- Settings → About credits FFmpeg, dav1d, GPUI and Lua, with their license
  texts, from `resources/attributions.json`, which `script/attributions`
  writes (`script/bundle-macos` runs it).

## Tests

### Running them

```bash
cargo test                          # Plays
cargo test --workspace              # Plays and the crates in crates/
cargo test -- --skip media::tests   # without the engine tests
script/check                        # all a change passes before a commit
```

- `script/check` runs the formatting, clippy, the workspace's tests, gpui's
  Linux build and cargo-deny's advisories; it skips the last two, saying
  what they need, when that isn't installed.

- The engine tests make clips with the `ffmpeg` command line tool, with
  libx264 and libx265 (`brew install ffmpeg`, `sudo apt install ffmpeg`).
  Without it they fail, saying so.
- Hardware decoding tests need VideoToolbox on macOS.
- What fish and Nushell complete is checked only where they're installed.
- Property tests try 256 cases each; `PROPTEST_CASES` sets more.

Ignored tests, and the protocol crates' examples for real devices:

```bash
# Dolby Vision profile 5 on a file of your own; saves both paths' frames as PNGs
PLAYS_DOLBY_VISION_SAMPLE=path/to/profile5.mkv cargo test dolby -- --ignored --nocapture
# plays through this machine's own outputs, each by its id
cargo test plays_through_the_devices_here -- --ignored --nocapture

cargo run -p chromecast --example chromecast -- list
cargo run -p dlna --example dlna -- list
cargo run -p airplay --example airplay -- list
```

`hand_over_as_another_process` and `command_line_as_another_process` are
ignored too: other tests run them in the test binary started again.

### Where they live

- In a `tests` folder beside the code, a file named after each code file:
  `src/media/tracks.rs`'s in `src/media/tests/tracks.rs`. The code file
  declares them at its end, so they reach its private items via `super`:

```rust
#[cfg(test)]
#[path = "tests/tracks.rs"]
mod tests;
```

- A folder module's own tests (its `mod.rs`'s) are in `tests/mod.rs`.
- A crate root's are named after the crate
  (`crates/dlna/src/tests/dlna.rs`), as proptest takes a folder holding a
  `lib.rs` or `main.rs` for the crate's own.
- Property tests are in `tests/<file>_properties.rs`, declared as
  `mod properties`; a helper both use is `pub(super)` in the unit tests.
- proptest saves a failure in the crate's `proptest-regressions/` at the
  property file's path (`proptest-regressions/tests/http_properties.txt`):
  commit it with the fix.
- Window tests are `ui::workspace`'s, in `src/ui/workspace/tests/`, a file
  per feature; one named for a workspace code file, such as `screensaver`,
  is that file's tests (`ui::workspace::screensaver::tests`).

### What's tested where

- `src/media/tests/`: the engine, tracks, subtitles, delays, colors, HDR, streams
- `src/media/decoder/tests/`: cover files, timestamps, seeks, frame conversion
- `src/ui/tests/`: screenshots, framing, PQ and HLG math, the text field and IME
- `src/ui/components/tests/`: panels' looks in pixels, `gpui::render_offscreen` (macOS)
- `src/ui/pip/tests/`: Picture in Picture's subtitles
- `src/ui/player/tests/`: a block's controls, console, notes, playlist, delays
- `src/ui/blackout/tests/`: blacking out the other displays
- `src/ui/workspace/tests/`: whole windows, a file per feature
- `src/ui/settings/tests/`: Settings' panes, settings as commands, About
- `src/ui/palette/tests/`: the command palette, Go to…, Change Shortcut
- `src/app/tests/`: menus, the keymap, windows, audio devices, speakers, Bluetooth
- `src/settings/tests/`: each setting's key and the values it takes
- `src/tests/`: the settings, progress, recents and hotkeys files; the command line
- `src/cli/tests/`: shell completions, `events` and `wait`, tables, Lua scripts
- `src/cli/lua/tests/`: a script's sleeps, waits, handlers, timers,
  programs and JSON; the guides' examples
- `src/api/tests/`, `src/api/*/tests/`: commands, events, the socket
- `src/cast/tests/`: each protocol's profiles, AirPlay pairings, the cast link
- `crates/*/src/tests/`: each crate on its own, protocols against fake devices
- `*_properties.rs`: parsers fed what a hostile device or client might send

### Window and command tests

- Headless, with `#[gpui::test]` on gpui's test platform (its
  `test-support` feature, in test builds only): simulated input, a clock
  that moves only when told, nothing drawn.
- `app::set_up_for_tests` starts the app with default settings never
  saved, silent audio, and without keeping the computer awake.
- Files open on real decoding threads, so tests wait for them in real
  time. A file plays on only as its block ticks: advance the clock a step
  at a time (`play_until`). The test platform's timers don't play a file
  to its end.
- `ui::testing` clicks and types, finding things by debug selector
  (`block-0`, a row's or button's id; a selected button also has
  `<id>-selected`); `testing::painted` lists the text a window drew, every
  character the same width; `testing::errors_logged`, the errors logged
  on the test's thread since `keep_errors`.
- `api::intercept` notes what's sent without running it, `api::record` as
  it runs.
- Run a command outside its window's update, with
  `TestAppContext::update(cx, |cx| api::run(..))`, not
  `VisualTestContext::update`, which holds its window.
- Ids are hex: a window number meant not to be found mustn't be one an id
  could end with (`past_the_last`); a name with letters past `f`, such as
  `Attic`, never is.

### Fixtures

Small files in `tests/fixtures`, committed so tests run without the
`ffmpeg` tool. `script/make-fixtures` makes them again and lists what each
is; it needs `ffmpeg` with `libx264`. Run it only to change them.

### Smoke runs

Headless windows have no native window behind them; smoke runs drive the
real app:

```bash
script/smoke                    # every scenario
script/smoke close subtitles    # just these
```

- Builds Plays with the `smoke` feature into `target/smoke`, with its own
  cargo profile, leaving the release build alone. Its settings are the
  defaults, never read or saved.
- Each scenario runs in its own process (`target/smoke/plays --smoke
  <name>`, `--smoke list` for the names); the script exits non-zero if any
  failed.

Scenarios:

- `first-open`: a video into the empty window, which must fit it
- `several`: two videos and a sound in one window
- `close`: ⌘W closes the blocks, then the window, and the app quits
- `subtitles`: the subtitles beside a video off and on again
- `step-back`: a frame forward and two back through B-frames
- `float-on-top`: floats while playing, Settings with it, as the system
  has it (the level on macOS, `_NET_WM_STATE_ABOVE` on X11)
- `windows`: a second window comes to the front, then ⌘1, Ctrl+Tab and
  Ctrl+Shift+Tab each bring theirs there
- `cli`: `plays`, a process of its own, over a socket `PLAYS_SOCKET`
  names: refused with 5 until control is on, then opening a clip, listing
  it, turning looping off, and waiting for it to end

Every run checks that:

- A window that fitted its files is laid out at its real size
- Each playing video shows at least 15 of its 25 frames a second
- The sound goes to a device
- Nothing logs an error, whatever `RUST_LOG` lets through

The clips loop at no volume. Keep the window uncovered while it runs: one
not drawing fails.

On Linux with no display, the script starts Xvfb with openbox (gpui draws
only under a window manager) and PulseAudio (its null sink plays in real
time); Vulkan can be software (lavapipe). It uses `Xvfb`, `openbox`,
`pulseaudio`, `pactl`, `xdpyinfo` and `xprop`.

## Conventions

- `cargo clippy --workspace --all-targets --all-features` prints nothing.
  The lints are in the root `Cargo.toml`, taken with
  `[lints] workspace = true`.
- `too_many_lines`: a function over 100 lines is split into helpers named
  for what each part does.
- The crates in `crates/` know nothing of Plays or gpui (the protocol
  crates nothing of FFmpeg either), and build and test on their own.
- Everything Plays changes in `vendor/gpui` is marked "Plays" in the code
  and described in `vendor/gpui/PLAYS.md`, which says how to update gpui
  too; `vendor/xattr/PLAYS.md` does the same for xattr.
- gpui isn't in the workspace, so its own tests don't run; check its code
  builds for Linux with
  `cargo check -p gpui --target aarch64-unknown-linux-gnu --tests` (needs
  a C compiler for that target). Its shaders for Linux are validated as
  Blade takes them by `src/ui/tests/linux_shaders.rs`.
- When FFmpeg, dav1d, gpui or Lua (crate `lua-src`) changes, run
  `script/attributions`.
- The test fixtures are committed; change them with `script/make-fixtures`.
- After changing a command or an event, write the reference again with
  `cargo run -- actions --markdown > docs/commands.md`. Developer commands,
  in debug builds only, aren't in it.
- Each command namespace is a file of its own in `src/api/commands/`.
- Every command has its own lower-case id in a namespace, its own title, a
  description that is a sentence, and its arguments named once. Every
  event is registered once, named in its namespace as commands are
  (`playback.ended`), with a sentence for it and each field.
- Commands, events, arguments, namespaces and help name only windows and
  blocks, a block being what shows a player.
- Debug builds have a Developer menu: look over its Component Gallery
  after changing a component or `theme`.
- Tests never read or write the real settings or progress files: the
  app's folder (`files::Folder`) is none in tests and smoke runs. Debug
  selectors exist only in test builds.
- A test that needs a folder takes its own: `Folder::for_test` for the
  app's, `files::test_folder` for any other. It's in `plays-tests` in the
  temporary folder and goes as the test's thread ends, pass or fail
  (`files::gone_as_test_ends`). Only media made for the tests stays there,
  made once.
- Each test is checked to fail when what it covers breaks. Expected values
  for color math are worked out apart from Plays' code.
