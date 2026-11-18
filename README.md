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
- HDR video (HDR10/PQ and HLG, BT.2020), and Dolby Vision, including
  profile 5, which has no HDR10 picture to fall back on. On macOS,
  highlights go brighter than SDR white as far as the screen allows (EDR,
  such as on XDR displays), and are tone mapped to fit on other screens.
  Elsewhere, and for video decoded in software, HDR is tone mapped to SDR on
  the CPU.
- Every file has its own play/pause, seek bar, loop, mute and volume.
- Files with several tracks get a track menu on their tile: pick the audio
  track, the subtitles (or none) and, if there are several, the video track,
  while it plays. Subtitles show over the picture: text ones (SRT, ASS,
  WebVTT, MP4 timed text) sized to the picture and styled as Settings say,
  and bitmap ones (PGS, VobSub, DVB) as the disc has them. A file's default
  subtitles, or failing that its forced ones, show from the start.
- Subtitle files beside a video are tracks too, and show from the start:
  `Movie.srt`, `Movie.en.srt`, `Movie.English.forced.ass` and so on for
  `Movie.mkv` (SRT, ASS, WebVTT, PGS `.sup`, VobSub `.idx`, MicroDVD, SAMI),
  the same in a `Subs` or `Subtitles` folder, and everything in
  `Subs/Movie/`. Their names give their language and whether they are
  forced or for the hearing impaired. Text in legacy encodings (Windows-1252,
  Windows-1251, Shift JIS and the like) is detected and converted.
- Any tile can pop out into a new window, where it keeps playing. Every
  window works the same: drop more files on it and they play side by side.
- Shortcuts, IINA's to start with, go to the tile last clicked: play/pause,
  seek, frame step, volume, tracks, subtitle size and height, full screen.
  With Shift, play/pause, seek, restart and mute go to every file in the
  window, as does the menu bar (a toolbar on Linux).
- On macOS, windows show just the video, edge to edge, with the traffic
  lights over it like other video players. While something plays, they fade
  out with the controls when the mouse leaves the window. Colors are macOS's
  own, in light and dark, with your accent color.
- Audio from every file is mixed into one output. A/V sync follows the audio clock.
- Audio-only files show their cover art (or a note) and a level meter.
- Settings window: theme, master volume, default volume, autoplay, loop,
  seek step, fit/fill scaling, gamma for SDR and for HDR video (applied to
  every open video as you drag), decode resolution limit, hardware decoding,
  subtitle style (font, weight, size, color, outline or shadow and its color,
  a background box, on the picture or anywhere in the tile, height), with a
  live preview, the keys for every shortcut, grid columns, controls
  visibility, file names. Saved as JSON in the user config dir.

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
| Play/pause | `Space` or `P` |
| Seek back/forward | `←` `→` (5 seconds at first; the step is in Settings) |
| Previous/next frame | `⌥←` `⌥→` / `Alt+←` `Alt+→`, or `,` `.` |
| Restart | `Home` |
| Loop | `⌘⇧L` / `Ctrl+Shift+L` |
| Volume down/up | `↓` `↑` |
| Mute/unmute | `M`, or `⌘/` / `Ctrl+/` |
| Next audio, subtitle, video track | `⌃⌘A` `⌃⌘S` `⌃⌘V` / `Ctrl+Alt+A` `S` `V` |
| Show/hide subtitles | `V` |
| Bigger/smaller subtitles | `⇧G` `⇧F` |
| Move subtitles up/down | `R` / `⇧R` or `T` |
| Full screen | `F`, or `⌃⌘F` / `F11`; `Esc` leaves it |
| Play/pause, seek, restart, mute all | `⇧Space`, `⇧←` `⇧→`, `⇧Home`, `⇧M` |
| Close all | `⌘⇧W` / `Ctrl+Shift+W` |
| Settings | `⌘,` / `Ctrl+,` |

Shortcuts act on the active tile: the one last clicked, or the maximized
one. A short
note on the tile says what changed; switching tracks rolls a reel, like a
slot machine's, to the track picked, with the ones before and after it
faded above and below. To change shortcuts, open Settings → Hotkeys,
click + by a command and press the keys; a key another command had moves to
it, and Restore brings IINA's back. The app's own shortcuts above (⌘O, ⌘W
and so on) stay as they are.

On a tile: click the picture to play/pause, double-click (or the corner
button) to maximize it, drag the bar to scrub. The track button (next to
loop, on files that have subtitles or more than one audio or video track)
opens the track menu: a column each for audio, subtitles and (when there
are several) video. For the only tile in a window,
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
  resamples audio to the mixer's rate, and handles accurate seeking. It
  plays one track of each kind; picking another opens a decoder for it and
  seeks to the playhead, so the new track starts where playback is.
- `src/media/tracks.rs`: the tracks a file has, and their names for the
  menu (language, title, forced and SDH, codec, channels).
- `src/media/subtitles.rs`: subtitle cues. Subtitle packets are decoded as
  soon as they are read. FFmpeg turns every text format into ASS lines, of
  which the words, line breaks, italics, bold and top/middle/bottom placement
  are kept; bitmaps become images placed as fractions of the picture. A
  cue without an end (as in PGS) lasts until the next one.
- `src/media/external.rs`: subtitle files beside a video: finding them,
  reading their names, and telling a text file's encoding (with `chardetng`)
  for FFmpeg's iconv to convert. A picked file is read whole and its packets
  decoded two seconds ahead of the playhead; after a seek, from 30 seconds
  before it, so cues already showing are there.
- `src/media/hdr.rs`: PQ and HLG, and the tone curve. HDR reference white
  (203 nits) is shown as SDR white, and brightness above three quarters of
  the screen's headroom eases toward it instead of clipping. On the CPU it
  turns HDR frames, scaled to 16-bit RGB by swscale, into 8-bit sRGB.
- `src/media/dovi.rs`: Dolby Vision profile 5, whose pictures are IPT
  rather than YCbCr. Each frame's reshaping curves and matrices (from its
  RPU, which FFmpeg parses) turn it into BT.2020 light like HDR10's. On the
  CPU from 16-bit 4:4:4 planes; for zero-copy surfaces, the curves are
  packed for gpui's Dolby Vision shader instead.
- `src/media/hwaccel.rs`: shares one hardware device per API between all
  decoders and copies GPU frames back for conversion where they can't be drawn
  directly. If the GPU can't take a stream, or keeps failing on it, decoding
  switches to software.
- `src/media/videotoolbox.rs` (macOS): the zero-copy path. A
  `VTPixelTransferSession` converts each VideoToolbox frame on the GPU into
  the full-range NV12, BT.601 surface gpui's `surface()` element draws,
  scaling it to the display size on the way. Untagged video gets the same
  matrix guess as in software (BT.709 from 720 lines up). HDR frames go to
  10-bit BT.2020 surfaces tagged with their transfer function instead, and
  Dolby Vision frames are only scaled, carrying their reshaping along.
- `vendor/gpui`: gpui 0.2.2 with HDR video on macOS (see its `PLAYS.md`). Its
  window draws in half floats and extended sRGB, everything but video as
  before, and a second surface shader decodes PQ and HLG and tone maps them
  under the screen's current EDR headroom, with the same math as `hdr.rs`.
  A third one reshapes Dolby Vision as `dovi.rs` does.
- `src/media/picture.rs`: the forms a decoded picture takes, and the gamma
  for SDR and HDR video that every player shares. Converted frames get it on
  the CPU; surfaces carry it to gpui's shaders each time they are drawn, so a
  change shows at once, even on paused video.
- `src/media/source.rs`: files and stream URLs. URLs are logged without
  credentials or query strings.
- `src/media/audio.rs`: one cpal output stream that mixes all players and
  applies per-player and master volume.
- `src/media/clock.rs`: per-player clock that runs on wall time and is eased
  toward the audio position on every audio callback.
- `src/media/player.rs`: the UI-side handle: play/pause/seek, frame selection,
  buffering after seeks (and, for streams, until a second is read ahead or
  when the network runs dry), end-of-file and looping.
- `src/app/`: starting the app and its own actions (`mod.rs`), the shared
  audio output, what each action does (`commands`), how actions reach the
  window and tile in front (`dispatch`), the keymap and the menu bar.
- `src/hotkeys.rs`: the commands shortcuts can run, their keys (IINA's to
  start), how they are saved in Settings and shown (`⌃⌘F`, `Ctrl+Alt+F`).
  The keymap is rebuilt from them whenever they change, and the menus show
  them.
- `src/ui/`: gpui views (`workspace`, one per window, `player_view`,
  `open_url`) and a text field based on gpui's input example.
  `subtitles` paints a tile's cues where `object_fit` puts the picture, in
  the style from Settings; the settings' Subtitles pane draws its preview
  with the same code.
- `src/ui/settings/`: the Settings window. Each pane (Playback, Audio,
  Video, Subtitles, Hotkeys, Appearance) is a file of its own with its name
  and icon for the sidebar; `form` has the cards, rows and sliders they are
  made of.
- `src/ui/components/`: the reusable controls (icon, buttons, switch,
  segmented control, slider) and a floating panel to put anything in,
  either a menu's or a note's over the picture, one `RenderOnce` component
  per file.

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
tagged or not, limited or full range. HDR10 and HLG clips (needs `libx265`)
must come out as tagged 10-bit surfaces whose HDR shader math, for an SDR
screen, matches the CPU tone mapping. Dolby Vision profile 5 can't be made
with the `ffmpeg` tool, so that test runs only on a file you name, comparing
both paths on two of its frames (and saving them as PNGs):

```bash
PLAYS_DOLBY_VISION_SAMPLE=path/to/profile5.mkv cargo test dolby -- --nocapture
```

A Matroska file with two audio and two SRT tracks checks that tracks are
listed and named, that switching audio while playing brings in the other
track, and that subtitles show, change track and turn off, at the right
times. A clip with an English SRT and a Windows-1251 Russian one beside it
checks that they are listed and named, that the first shows from the start,
that seeking back lands in a cue already showing, and that the Russian one
reads right. Unit tests cover finding files for a video, reading their
names and telling encodings, and the shortcuts: no default key used twice, a
key moving to the command it is given, reading and writing them in the
settings file, staying out of text fields, and how they are shown.

Stream tests serve the clips from a small HTTP server on localhost:
HTTP files, HLS (normal and live), a connection that stalls mid-way,
unreachable hosts, and closing while a connection hangs.

## Limitations

- On Linux, every frame is converted to BGRA on the CPU, including frames
  decoded on the GPU, which are copied back first (gpui's Linux renderer
  can't draw GPU surfaces). Lower the decode resolution limit in Settings when
  playing many high-resolution files. The same applies on macOS to videos
  VideoToolbox can't decode, such as VP9.
- HDR goes brighter than SDR white only on macOS, and only for video decoded
  by VideoToolbox; software-decoded HDR is tone mapped to SDR. HDR metadata
  (mastering display, content light level, HDR10+ and Dolby Vision's
  per-scene brightness and trims) isn't used. Dolby Vision profiles with an
  HDR10 or HLG picture (7, 8) show that picture; profile 7's enhancement
  layer is ignored.
- Resuming a paused live stream continues from where it paused rather than
  jumping to the live edge, and after a long pause the server may have closed
  the connection.
- Subtitles: ASS styling (fonts, colors, sizes, positions, effects) isn't
  used, only italics, bold and top/middle/bottom placement. A cue in the
  file that started before the keyframe a seek lands on is missed (one in a
  subtitle file beside it isn't, unless it started over 30 seconds before).
  Subtitle files can't be dropped on a tile, and there is no way to shift
  their timing. In a live stream, a new track starts with its next packets
  rather than at the playhead.
- No playlists, playback speed or audio device selection.
- On GNOME Wayland (client-side decorations) the window can be moved from the
  toolbar but has no resize handles.
