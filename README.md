# Plays

A small video and audio player for macOS and Linux that plays many files at
once, side by side. All UI, including the settings window, is built with
[gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's
GPU-accelerated UI framework.

- Open any number of files (or whole folders): each gets a tile in an auto-sized grid.
- Every file has its own play/pause, seek bar, loop, mute and volume.
- Toolbar and shortcuts control all files at once: play/pause, seek, mute, restart, close.
- Audio from every file is mixed into one output. A/V sync follows the audio clock.
- Audio-only files show their cover art (or a note) and a level meter.
- Settings window: theme, master volume, default volume, autoplay, loop,
  seek step, fit/fill scaling, decode resolution limit, grid columns,
  controls visibility, file names. Saved as JSON in the user config dir.

## Building

You need a Rust toolchain (1.85+, edition 2024), FFmpeg's development
libraries, `pkg-config` and `clang` (FFmpeg bindings are generated with bindgen).

### macOS

Xcode (for the Metal shader compiler) and Homebrew packages:

```bash
brew install ffmpeg pkgconf
```

```bash
cargo run --release -- path/to/video.mp4 path/to/song.mp3
```

If `pkg-config` isn't installed, point the build at FFmpeg directly:

```bash
FFMPEG_DIR=$(brew --prefix ffmpeg) cargo build --release
```

### Linux (Debian/Ubuntu)

```bash
sudo apt install build-essential pkg-config clang libclang-dev \
  libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libswresample-dev \
  libasound2-dev libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libvulkan1 mesa-vulkan-drivers
```

gpui renders with Vulkan on Linux, so a working Vulkan driver is required.
Audio goes through ALSA (PipeWire and PulseAudio provide an ALSA device).
FFmpeg 5.1 or newer is recommended.

## Using it

| Action | Shortcut |
| --- | --- |
| Open files | `⌘O` / `Ctrl+O`, or drop files/folders on the window |
| Play/pause all | `Space` or `K` |
| Seek all back/forward | `←` `→` or `J` `L` (step set in Settings) |
| Restart all | `Home` |
| Mute/unmute all | `M` |
| Close all | `⌘⇧W` / `Ctrl+Shift+W` |
| Settings | `⌘,` / `Ctrl+,` |

On a tile: click the picture to play/pause, double-click (or the corner
button) to maximize it, drag the bar to scrub.

## How it works

```
                   ┌──────────── decoder thread (one per file) ────────────┐
 file ─ FFmpeg ─▶  │ demux ─▶ video packets ─▶ decode ─▶ swscale → BGRA ──▶ │ ─▶ video frame queue ─┐
                   │       └▶ audio packets ─▶ decode ─▶ swresample f32 ──▶ │ ─▶ audio queue ─┐     │
                   └──────────────────────────────────────────────────────┘                 │     │
   cpal output callback: mix every playing audio queue ◀────────────────────────────────────┘     │
        └─ reports the time being heard to each player's clock                                     │
   gpui render (every display frame while playing): show the newest frame with pts ≤ clock ◀───────┘
```

- `src/media/decoder.rs`: per-file thread. Reads packets only when a decoder
  needs input, parks packets for a stream whose output queue is full, converts
  video to BGRA `RenderImage`s (with the right YUV matrix and pixel aspect),
  resamples audio to the mixer's rate, and handles accurate seeking.
- `src/media/audio.rs`: one cpal output stream that mixes all players and
  applies per-player and master volume.
- `src/media/clock.rs`: per-player clock that runs on wall time and is eased
  toward the audio position on every audio callback.
- `src/media/player.rs`: the UI-side handle: play/pause/seek, frame selection,
  buffering after seeks, end-of-file and looping.
- `src/ui/`: gpui views (`workspace`, `player_view`, `settings_view`) and
  small hand-made widgets (buttons, switch, segmented control, slider).

Frames are uploaded to gpui's texture atlas through `img()`. Replaced frames
are released with `Window::drop_image` a few display frames later, so GPU
memory stays flat during playback.

## Tests

```bash
cargo test
```

The engine tests generate short clips with the `ffmpeg` command line tool
(skipped if it isn't installed) and check opening, accurate seeking, audio
resampling through the mixer, end of file, looping and keeping up when frames
are consumed rarely.

## Limitations

- Software decoding only (no VideoToolbox/VAAPI), and every frame is converted
  to BGRA on the CPU. Lower the decode resolution limit in Settings when
  playing many high-resolution files.
- No subtitles, playlists, playback speed or audio device selection.
- On GNOME Wayland (client-side decorations) the window can be moved from the
  toolbar but has no resize handles.
