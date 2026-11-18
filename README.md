# Plays

A small video and audio player for macOS and Linux that plays many files at
once, side by side. All UI, including the settings window, is built with
[gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's
GPU-accelerated UI framework.

- Open any number of files (or whole folders): each gets a tile in an auto-sized grid.
- Open network streams with **Open URL…**: HTTP(S) files, HLS and DASH
  playlists, RTMP and RTSP. Live streams show a LIVE badge; streams that stall
  pause and resume instead of skipping ahead.
- Hardware video decoding: VideoToolbox on macOS; VA-API, NVDEC or VDPAU on
  Linux. Videos the GPU can't decode fall back to software automatically. On
  macOS, hardware-decoded frames never leave the GPU (zero-copy).
- Every file has its own play/pause, seek bar, loop, mute and volume.
- Any tile can pop out into a new window, where it keeps playing. Every
  window works the same: drop more files on it and they play side by side.
- The menu bar (a toolbar on Linux) and shortcuts control all files in a
  window at once: play/pause, seek, mute, restart, close.
- On macOS, windows show just the video, edge to edge, with the traffic
  lights over it like other video players. While something plays, they fade
  out with the controls when the mouse leaves the window. Colors are macOS's
  own, in light and dark, with your accent color.
- Audio from every file is mixed into one output. A/V sync follows the audio clock.
- Audio-only files show their cover art (or a note) and a level meter.
- Settings window: theme, master volume, default volume, autoplay, loop,
  seek step, fit/fill scaling, decode resolution limit, hardware decoding,
  grid columns, controls visibility, file names. Saved as JSON in the user
  config dir.

## Building

You need a Rust toolchain (1.85+, edition 2024), FFmpeg's development
libraries, `pkg-config` and `clang` (FFmpeg bindings are generated with bindgen).

### macOS

Xcode (for the Metal shader compiler) and Homebrew packages:

```bash
brew install ffmpeg pkgconf
```

```bash
cargo run --release -- path/to/video.mp4 path/to/song.mp3 https://example.com/live.m3u8
```

If `pkg-config` isn't installed, point the build at FFmpeg directly:

```bash
FFMPEG_DIR=$(brew --prefix ffmpeg) cargo build --release
```

To open files from Finder (**Open With**) or by dropping them on the Dock
icon, build the app bundle:

```bash
script/bundle-macos
```

This makes `target/release/Plays.app`. Once it's in `/Applications` (or has
been opened once), Finder offers Plays under Open With for video and audio
files without making it the default player. The app uses Homebrew's FFmpeg,
so it only runs where that is installed. The script compiles the app icon,
the Icon Composer file `assets/AppIcon.icon`, into the app, which takes
Xcode 26 or later.

### Linux (Debian/Ubuntu)

```bash
sudo apt install build-essential pkg-config clang libclang-dev \
  libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libswresample-dev \
  libasound2-dev libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libvulkan1 mesa-vulkan-drivers
```

gpui renders with Vulkan on Linux, so a working Vulkan driver is required.
For hardware decoding, install your GPU's VA-API driver (`intel-media-va-driver`
or `mesa-va-drivers`), or use NVIDIA's driver for NVDEC. The tile header shows
the API in use, for example `h264 (VA-API)`.
Audio goes through ALSA (PipeWire and PulseAudio provide an ALSA device).
FFmpeg 5.1 or newer is recommended.

For your file manager's **Open With** menu, put the `plays` binary on your
`PATH` and copy `resources/linux/plays.desktop` to `~/.local/share/applications/`.

## Using it

| Action | Shortcut |
| --- | --- |
| Open files | `⌘O` / `Ctrl+O`, or drop files/folders on the window (or on the Dock icon of `Plays.app`) |
| Open a stream URL | `⌘⇧O` / `Ctrl+Shift+O` (pre-filled from the clipboard) |
| Play/pause all | `Space` or `K` |
| Seek all back/forward | `←` `→` or `J` `L` (step set in Settings) |
| Restart all | `Home` |
| Mute/unmute all | `M` |
| Close all | `⌘⇧W` / `Ctrl+Shift+W` |
| Settings | `⌘,` / `Ctrl+,` |

On a tile: click the picture to play/pause, double-click (or the corner
button) to maximize it, drag the bar to scrub. For the only tile in a window,
both toggle full screen instead. When a window has more than
one tile, the button next to maximize moves a tile to a new window sized for
it, where it keeps playing. Every window takes drops and works the same
way; menus and shortcuts act on the window in front, and files opened from
Finder go to it too. Drag the top edge of a window to move it. Closing the
last window quits.

## How it works

```
                ┌ demux thread ┐   ┌──────────── decode thread (one per file/stream) ──────────────┐
 file/URL ──▶   │ read ahead   │ ─▶│ video packets ─▶ decode (CPU or GPU) ─▶ swscale → BGRA ──────▶ │ ─▶ video frame queue ─┐
   FFmpeg       │ & seek       │ ─▶│ audio packets ─▶ decode ─▶ swresample f32 ───────────────────▶ │ ─▶ audio queue ─┐     │
                └──────────────┘   └───────────────────────────────────────────────────────────────┘                 │     │
   cpal output callback: mix every playing audio queue ◀────────────────────────────────────────────────────────────┘     │
        └─ reports the time being heard to each player's clock                                                             │
   gpui render (every display frame while playing): show the newest frame with pts ≤ clock ◀───────────────────────────────┘
```

- `src/media/decoder.rs`: two threads per file or stream. The demux thread
  reads packets ahead (about a second for files, up to 15 s for network
  streams so a slow connection doesn't stall decoding) and carries out seeks.
  The decode thread decodes only while its output queues have room, converts
  video to BGRA `RenderImage`s (with the right YUV matrix and pixel aspect),
  resamples audio to the mixer's rate, and handles accurate seeking.
- `src/media/hwaccel.rs`: shares one hardware device per API between all
  decoders and copies GPU frames back for conversion where they can't be drawn
  directly. If the GPU can't take a stream, or keeps failing on it, decoding
  switches to software.
- `src/media/videotoolbox.rs` (macOS): the zero-copy path. A
  `VTPixelTransferSession` converts each VideoToolbox frame on the GPU into
  the full-range NV12, BT.601 surface gpui's `surface()` element draws,
  scaling it to the display size on the way. Untagged video gets the same
  matrix guess as in software (BT.709 from 720 lines up).
- `src/media/source.rs`: files and stream URLs. URLs are logged without
  credentials or query strings.
- `src/media/audio.rs`: one cpal output stream that mixes all players and
  applies per-player and master volume.
- `src/media/clock.rs`: per-player clock that runs on wall time and is eased
  toward the audio position on every audio callback.
- `src/media/player.rs`: the UI-side handle: play/pause/seek, frame selection,
  buffering after seeks (and, for streams, until a second is read ahead or
  when the network runs dry), end-of-file and looping.
- `src/ui/`: gpui views (`workspace`, one per window, `player_view`,
  `settings_view`, `open_url`) and a text field based on gpui's input example.
- `src/ui/components/`: the reusable controls (icon, buttons, switch,
  segmented control, slider), one `RenderOnce` component per file.

Software-decoded frames (and, on Linux, hardware-decoded ones) are uploaded to
gpui's texture atlas through `img()`; on macOS, hardware-decoded frames are
drawn in place with `surface()`. Replaced frames are kept a few display frames
in case the GPU is still reading them, then released (images with
`Window::drop_image`), so GPU memory stays flat during playback.

Playing four 4K H.264 files at once on an Apple silicon Mac takes about 20% of
one core with zero-copy, against about 100% when hardware frames are copied
back and converted on the CPU.

## Tests

```bash
cargo test
```

The engine tests generate short clips with the `ffmpeg` command line tool
(skipped if it isn't installed) and check opening, accurate seeking, audio
resampling through the mixer, end of file, looping, keeping up when frames
are consumed rarely, and hardware decoding (VideoToolbox is required on
macOS). On macOS, zero-copy frames are read back and run through the surface
shader's math, and must match the software path's colors for SD and HD video,
tagged or not, limited or full range. Stream tests serve the clips from a small HTTP server on localhost:
HTTP files, HLS (normal and live), a connection that stalls mid-way,
unreachable hosts, and closing while a connection hangs.

## Limitations

- On Linux, every frame is converted to BGRA on the CPU, including frames
  decoded on the GPU, which are copied back first (gpui's Linux renderer
  can't draw GPU surfaces). Lower the decode resolution limit in Settings when
  playing many high-resolution files. The same applies on macOS to videos
  VideoToolbox can't decode, such as VP9.
- Resuming a paused live stream continues from where it paused rather than
  jumping to the live edge, and after a long pause the server may have closed
  the connection.
- No subtitles, playlists, playback speed or audio device selection.
- On GNOME Wayland (client-side decorations) the window can be moved from the
  toolbar but has no resize handles.
