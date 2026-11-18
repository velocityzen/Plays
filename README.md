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
  Elsewhere, and for video decoded in software, HDR is tone mapped to SDR:
  on Linux on the GPU (Dolby Vision on the CPU), on macOS on the CPU.
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
- Turn the picture a quarter turn either way, flip it, show it at another
  aspect ratio (1:1 to 2.35:1) or crop it to another shape (1:1 to 2.39:1),
  with shortcuts or the Video menu, each tile on its own. Videos whose
  files say to turn them (as phones do) show upright. It all happens as the
  picture is drawn, so even a paused video changes at once.
- Move a file's sound or subtitles earlier or later against the picture,
  for files that are out of step: the sound in 50 ms steps, subtitles
  (embedded or in a file beside the video) in 100 ms steps. The sound
  plays on as it is and the picture moves against it.
- Take a screenshot of a tile with `S` or Video → Take Screenshot: the
  frame it shows, saved as PNG or JPEG at the video's full resolution
  (whatever the decode limit), turned, reshaped and cropped as it shows,
  with the subtitles showing drawn in. HDR video is saved as an SDR screen
  shows it, or kept as HDR in a 16-bit PNG. Files are named after the video
  and the time, `Movie 01-23-45.678.png`, in Pictures or a folder you pick,
  and never replace another.
- Any tile can pop out into a new window, where it keeps playing. Every
  window works the same: drop more files on it and they play side by side.
- As files open, their window takes the size of the video, or of a grid
  with room for each video at its own size, scaled down to fit the screen.
  After that the window keeps the size you give it.
- On macOS 12 and later, a tile can play in macOS's own Picture in Picture
  window, over every app and Space, with its play/pause and skip buttons:
  `⌃⌘P`, Video → Picture in Picture, or the button on the tile. The sound
  keeps coming through Plays, and the subtitles showing are drawn into the
  picture. Closing the window, or its button to go back, brings the video
  back to the tile.
- While a tile plays, its header and controls fade when the mouse leaves
  it, or rests on it for a couple of seconds; the pointer hides then too,
  until the mouse moves. Resting on the header or controls keeps them.
- While a video plays the screen doesn't sleep, and while only sound plays
  the computer doesn't (a power assertion on macOS, the desktop portal's
  Inhibit on Linux). Paused or ended, they sleep as they would.
- A console on the tile in front, `~` or Video → Console: the file's
  format, each stream playing as the file has it (codec and profile, size,
  pixel format, colors, sample rate, channels, bit rates), the decoder and
  what its frames become, and, as it plays, frames shown a second, frames
  dropped, how far the picture is from the clock, what is queued and how
  far ahead the file or stream is read.
- Shortcuts go to the tile last clicked: play/pause, seek, frame step,
  volume, tracks, subtitle size and height, audio and subtitle delay,
  rotation, flips, aspect ratio, crop, screenshots, full screen.
  With Shift, play/pause, seek, restart and mute go to every file in the
  window, as does the menu bar (a toolbar on Linux).
- On macOS, windows show just the video, edge to edge, with the traffic
  lights over it like other video players. While something plays, they fade
  out with the controls when the mouse leaves the window or rests in it.
  Colors are macOS's
  own, in light and dark, with your accent color.
- Audio from every file is mixed into one output: the system's default, or
  a device picked in Settings, remembered from run to run. A/V sync follows
  the audio clock.
- Audio-only files show their cover art (or a note) and a level meter.
- About Plays, in the app menu on macOS: a small window with the app's
  icon, version and the day it was built (Settings → About says them too).
- Settings window: theme, master volume, the audio output, default volume, autoplay, loop,
  seek step, fit/fill scaling, gamma for SDR and for HDR video (applied to
  every open video as you drag), decode resolution limit, hardware decoding,
  subtitle style (font, weight, size, color, outline or shadow and its color,
  a background box, on the picture or anywhere in the tile, height), with a
  live preview, where screenshots go and what they are like, the keys for
  every shortcut, grid columns, controls
  visibility, file names. Saved as JSON in the user config dir; Settings
  File, at the foot of the sidebar, opens its folder, exports a copy,
  imports settings from one, or resets every setting to its default
  (after asking).

## Building

You need a Rust toolchain (1.85+, edition 2024) and `clang` (FFmpeg's
bindings are generated with bindgen).

FFmpeg is built into the app. `script/build-ffmpeg` builds it once from
source, which takes a few minutes: FFmpeg 9.0.2 with dav1d for AV1, as
static libraries that only decode, under the LGPL. It installs them in
`build/ffmpeg`, where `.cargo/config.toml` points the build, and builds them
again only when the script changes. It needs `pkg-config`, `ninja` and
`python3` (meson, which builds dav1d, is downloaded with the sources), and
`nasm` on x86_64.

### macOS

Xcode (for the Metal shader compiler) and Homebrew packages:

```bash
brew install pkgconf ninja
```

```bash
script/build-ffmpeg
```

```bash
cargo run --release -- path/to/video.mp4 path/to/song.mp3 https://example.com/live.m3u8
```

FFmpeg uses VideoToolbox, AudioToolbox and macOS's TLS, so the app needs
nothing beyond macOS. To build with another FFmpeg instead, such as
Homebrew's (which is GPL, and which the app then needs installed to run):

```bash
FFMPEG_DIR=$(brew --prefix ffmpeg) cargo build --release
```

To open files from Finder (**Open With**) or by dropping them on the Dock
icon, build the app bundle:

```bash
script/bundle-macos
```

This builds FFmpeg first if it isn't built, and makes
`target/release/Plays.app`, which runs on any Mac. Once it's in
`/Applications` (or has been opened once), Finder offers Plays under Open
With for video and audio files without making it the default player. The
script compiles the app icon, the Icon Composer file `assets/AppIcon.icon`,
into the app, which takes Xcode 26 or later.

### Linux (Debian/Ubuntu)

```bash
sudo apt install build-essential pkg-config clang libclang-dev nasm ninja-build python3 \
  libssl-dev zlib1g-dev libva-dev libvdpau-dev libx11-dev libffmpeg-nvenc-dev \
  libasound2-dev libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-xcb-dev libxcb1-dev libvulkan1 mesa-vulkan-drivers
```

```bash
script/build-ffmpeg
```

```bash
cargo run --release -- path/to/video.mp4 path/to/song.mp3 https://example.com/live.m3u8
```

FFmpeg is linked in, but uses the system's OpenSSL for https, and for
hardware decoding libva (VA-API) and libvdpau, which the script builds it
with when their development packages are installed (it says which it
leaves out). NVDEC needs only NVIDIA's driver, at run time.

gpui renders with Vulkan on Linux, so a working Vulkan driver is required.
For hardware decoding, install your GPU's VA-API driver (`intel-media-va-driver`
or `mesa-va-drivers`), or use NVIDIA's driver for NVDEC. The tile header shows
the API in use, for example `h264 (VA-API)`.
Audio goes through ALSA (PipeWire and PulseAudio provide an ALSA device),
so Settings → Audio lists ALSA's outputs: `default` and `pipewire` or
`pulse` go through the sound server, which picks the speakers.

For your file manager's **Open With** menu, put the `plays` binary on your
`PATH` and copy `resources/linux/plays.desktop` to `~/.local/share/applications/`.

### Licenses

Plays is under the MIT license (`LICENSE`). It is built with FFmpeg (LGPL 2.1
or later) and dav1d (BSD 2-Clause), from their unchanged source, and with
the Rust crates in `Cargo.lock`. Settings → About lists them all with their
licenses, from `resources/attributions.json`; `script/attributions` writes
it again when the dependencies change (`script/bundle-macos` runs it).

## Using it

| Action | Shortcut |
| --- | --- |
| Open files | `⌘O` / `Ctrl+O`, or drop files/folders on the window (or on the Dock icon of `Plays.app`) |
| Open a stream URL | `⌘⇧O` / `Ctrl+Shift+O` (pre-filled from the clipboard) |
| Play/pause | `Space` or `P` |
| Seek back/forward | `←` `→` (5 seconds at first; the step is in Settings). Where keyframes are far apart, a skip lands on the nearest one its way, so it shows at once |
| Previous/next frame | `⌥←` `⌥→` / `Alt+←` `Alt+→`, or `,` `.`, or `<` `>` |
| Restart | `Home` |
| Previous/next file in its folder | `⌘←` `⌘→` / `Ctrl+←` `Ctrl+→` |
| Loop | `⌘⇧L` / `Ctrl+Shift+L` |
| Volume down/up | `↓` `↑` |
| Mute/unmute | `M`, or `⌘/` / `Ctrl+/` |
| Next audio, subtitle, video track | `⌃⌘A` `⌃⌘S` `⌃⌘V` / `Ctrl+Alt+A` `S` `V` |
| Show/hide subtitles | `V` |
| Bigger/smaller subtitles | `⇧G` `⇧F` |
| Move subtitles up/down | `R` / `⇧R` or `T` |
| Audio earlier/later, back in step | `⌃-` `⌃=`, `⌃⌫` / `Ctrl+-` `Ctrl+=`, `Ctrl+Backspace` |
| Subtitles earlier/later, back in step | `Z` `X`, `⇧⌫` / `Shift+Backspace` |
| Rotate right/left | `⌘R` `⌘L` / `Ctrl+R` `Ctrl+L` |
| Flip horizontally/vertically | `⌘⇧H` `⌘⇧V` / `Ctrl+Shift+H` `Ctrl+Shift+V` |
| Next aspect ratio, next crop | `A`, `C` |
| Picture as the file has it | `⌘0` / `Ctrl+0` |
| Take a screenshot | `S` |
| Full screen | `F`, or `⌃⌘F` / `F11`; `Esc` leaves it |
| Picture in Picture (macOS) | `⌃⌘P` |
| Console | `~` or `` ` `` |
| Play/pause, seek, restart, mute all | `⇧Space`, `⇧←` `⇧→`, `⇧Home`, `⇧M` |
| Close the file in front | `⌘W` / `Ctrl+W` (in an empty window, it closes the window) |
| Close all | `⌘⇧W` / `Ctrl+Shift+W` |
| Close the window | `⌥⌘W` / `Ctrl+Alt+W` |
| Settings | `⌘,` / `Ctrl+,` |

Shortcuts act on the active tile: the one last clicked, or the maximized
one. A short
note on the tile says what changed; switching tracks rolls a reel, like a
slot machine's, to the track picked, with the ones before and after it
faded above and below. To change shortcuts, open Settings → Hotkeys,
click + by a command and press the keys; a key another command had moves to
it, and Restore brings the defaults back. The app's own shortcuts above (⌘O, ⌘W
and so on) stay as they are.

On macOS, the Video menu also lists every aspect ratio and crop to pick
from; `A` and `C` go through them in turn, then back to none. Rotating and
flipping go by the picture as it shows, and cropping keeps its middle;
opening another file in the tile starts it as its file has it. The same
goes for delays, which a note shows as they change ("Audio delay +200 ms").
The sound can move up to 5 seconds either way, subtitles up to a minute.

Settings → Audio picks the output everything plays through: the system's
default (which the sound follows as it changes, on macOS) or one device.
While the device picked isn't connected, players keep playing, silently,
until it is (or, with Wait for it off, through the system's default), and
the sound goes back to it as soon as it's there.

A screenshot's note says which folder it went to. Settings → Screenshots
picks the folder (Pictures at first; Show opens it), PNG or JPEG and the
JPEG's quality, whether subtitles go in, and how HDR video is saved: as
an SDR screen shows it (tone mapped, with the HDR gamma), or as it is, in
a 16-bit PNG tagged BT.2020 PQ or HLG (its cICP chunk), which only
HDR-aware apps show right. Text subtitles are laid out as on a tile
showing just the picture, in the style Settings give them; bitmap ones go
where the stream puts them.

On a tile: click the picture to make it the tile shortcuts go to,
double-click it (or the corner button) to maximize it, drag the bar to
scrub. In the middle under the bar,
the previous and next buttons open the media file before or after this one
in its folder, by name, in the same tile; between them, skip back and
forward go by the seek step, around play/pause. The track button (next to
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
  resamples audio to the mixer's rate, and handles accurate seeking: frames
  no other frame refers to aren't decoded on the way to a seek's target. A
  skip (the arrow keys, the skip buttons) is accurate when a keyframe is
  within a second before its target, and otherwise lands on the nearest
  keyframe its way, which shows at once. It plays one track of each kind;
  picking an audio or video track opens a decoder for it and seeks to the
  playhead, so the new track starts where playback is. The other subtitle
  tracks' packets are kept for a few seconds, so a subtitle track picked
  starts with them, without a seek.
- `src/media/details.rs`: what the console shows of the file's format and
  the streams playing, read from their decoders before they decode.
- `src/media/tracks.rs`: the tracks a file has, and their names for the
  menu (language, title, forced and SDH, codec, channels).
- `src/media/subtitles.rs`: subtitle cues. Subtitle packets are decoded as
  soon as they are read. FFmpeg turns every text format into ASS lines, of
  which the words, line breaks, italics, bold and top/middle/bottom placement
  are kept; bitmaps become images placed as fractions of the picture. A
  cue without an end (as in PGS) lasts until the next one. Cues show by the
  playhead less the subtitle delay. Seeks keep them, from a minute before
  the playhead to a minute after, so stepping or skipping back shows the
  ones just over at once (even one that started before the keyframe the
  seek reads from), and moving subtitles later brings them back. A cue read
  again is kept once.
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
  A third one reshapes Dolby Vision as `dovi.rs` does. On Linux, its Vulkan
  renderer uploads video frames' planes to textures of their own and a
  shader converts and tone maps them the same way. Images and surfaces
  can be drawn turned, mirrored and cropped, by where they are sampled.
- `src/media/yuv.rs` (Linux): frames handed to gpui as their Y'CbCr planes
  (`yuv420p`, `nv12`, 10-bit `yuv420p10` and `p010`, which hardware decoders
  give), with the matrix from samples to R'G'B' for their YCbCr matrix,
  range and bit depth, and where their chroma samples sit. The CPU only
  copies the planes, or has swscale scale them (still as Y'CbCr) when the
  picture is over the resolution limit or its pixels aren't square; gpui
  converts them to RGB, tone maps HDR and applies the gamma on the GPU.
  Dolby Vision and other formats are converted on the CPU as before, and
  `PLAYS_GPU_CONVERSION=0` sends every frame that way.
- `src/media/orientation.rs`: the eight ways to turn and mirror a picture,
  and the one a video's display matrix asks for.
- `vendor/xattr`: xattr 0.2.3, which gpui reaches, building on Linux with
  today's libc (see its `PLAYS.md`).
- `src/media/still.rs`: a frame decoded again on its own for a screenshot:
  the file opened again, sought to the keyframe before the frame shown and
  decoded in software up to it (found by its timestamp), then converted at
  full size, tone mapped as playback does, or as BT.2020 PQ or HLG.
- `src/media/picture.rs`: the forms a decoded picture takes, and the gamma
  for SDR and HDR video that every player shares. Converted frames get it on
  the CPU; surfaces carry it to gpui's shaders each time they are drawn, so a
  change shows at once, even on paused video.
- `src/media/source.rs`: files and stream URLs. URLs are logged without
  credentials or query strings. Folders are listed (for the media files in
  one dropped, and a file's previous and next) in the background, as they
  may be on a network drive, and by name without looking at each file.
- `src/media/audio.rs`: the mixer, which sums all players and applies
  per-player and master volume. It tells each player's clock what is being
  heard, moved by the player's audio delay, so the picture runs that far
  ahead of the sound (or behind it). It plays at the output's rate; when a
  new device wants another, what players have queued is converted to it and
  decoders resample to it from then on.
- `src/media/output.rs`: where the mixer's sound goes: a cpal stream to a
  device, in the device's own format, or, with no device, a thread that
  runs the mixer in time so players keep playing silently. It lists the
  outputs and finds one by the id the system keeps for it.
- `src/media/clock.rs`: per-player clock that runs on wall time and is eased
  toward the audio position on every audio callback.
- `src/media/player.rs`: the UI-side handle: play/pause/seek, frame selection,
  buffering after seeks (and, for streams, until a second is read ahead or
  when the network runs dry), end-of-file and looping, and the console's
  figures: frames shown and dropped, lateness, and what is queued.
- `src/app/`: starting the app and its own actions (`mod.rs`), the shared
  audio output and which device it plays through (`audio`: the one picked,
  waited for or stood in for while it's away, and watched for to come
  back), what each action does (`commands`), how actions reach the
  window and tile in front (`dispatch`), the keymap and the menu bar.
- `src/hotkeys.rs`: the commands shortcuts can run, their default
  keys, how they are saved in Settings and shown (`⌃⌘F`, `Ctrl+Alt+F`).
  The keymap is rebuilt from them whenever they change, and the menus show
  them.
- `src/ui/`: gpui views (`open_url`, the player and Settings below) and a
  text field based on gpui's input example.
  `framing` works out how a tile's picture shows: the file's orientation,
  then the tile's turns, flips, aspect ratio and crop, as a size to fit and
  where to sample the frame. `subtitles` paints a tile's cues on the
  picture it frames, in the style from Settings, turning and cropping
  bitmap ones with it; the settings' Subtitles pane draws its preview with
  the same code. `overlay` draws subtitles into pixels rather than with
  gpui: text laid out by `subtitles` as on a tile showing just the picture,
  drawn with the glyphs gpui rasterizes, and bitmaps stretched where the
  stream places them. `screenshot` saves a tile's frame: bitmap subtitles
  laid on it, then turned and cropped on the CPU as `framing` says, then
  text subtitles, and written as PNG (with `png`, which can tag HDR) or
  JPEG.
- `src/ui/player/`: a player's tile, a file per part: what fills it
  (`content`), the header, the controls, the track menu, notes on what
  shortcuts did (`osd`, with the track reel), the `console`, what
  shortcuts do to it
  (`commands`, `adjust` for the picture, `sync` for delays and
  `screenshot`; `pip` runs Picture in Picture for it), and the graveyard
  frames wait in before they are released.
- `src/ui/pip/` (macOS): Picture in Picture. The tile's frames go, as they
  are due, to an `AVSampleBufferDisplayLayer` hidden over the tile, which
  AVKit's `AVPictureInPictureController` shows in the system's window; a
  delegate of Plays' own answers its buttons. The tile pumps its player on
  a timer of its own, since gpui stops drawing a covered window. VideoToolbox
  surfaces go as they are, tagged with their colors (HDR included); other
  frames are copied into BGRA buffers, and turned or mirrored videos are
  turned on the GPU. AVKit moves the layer's video into its window at a size
  and scale of its own rather than fitted to it, so Plays keeps it filling
  the window. Subtitles can't go over the video there, so while any show,
  frames are scaled to the window's pixels on the GPU and the subtitles
  drawn into them (`burn_in`), in the frame's own signal and samples.
- `src/ui/settings/`: the Settings window. Each pane (Playback, Audio,
  Video, Subtitles, Screenshots, Hotkeys, Appearance, About) is a file of its own with its
  name and icon for the sidebar; `form` has the cards, rows and sliders they
  are made of, and `file` the settings file's menu at the sidebar's foot. About shows the version and the credits, each of which opens
  to its license.
- `src/ui/workspace/`: a window, one per window, a file per part: opening
  files, folders and streams (`opening`), the tiles and moving one to its
  own window (`tiles`), sizing the window for the videos opened in it
  (`fit`), the grid, the empty window's ring (`empty`), the
  toolbar, the mouse coming, going and resting, and the traffic lights
  (`mouse`), and what the Playback
  commands do to every tile (`commands`).
- `src/ui/components/`: the reusable components, one `RenderOnce`
  component per file, each in three sizes (small, medium, large) with its
  numbers for each in `theme`. An icon; a button in seven variants
  (normal, primary, secondary, danger, ghost, link, icon) with its icon
  on either side, and selected or disabled; a switch, a segmented control,
  a slider and a meter; menu rows, and a popover that opens a menu from a
  button; a badge; a flat card; and a floating panel to put anything in,
  a menu's or a note's over the picture.
- `build.rs`: links the static FFmpeg with what it needs in turn, read
  from its pkg-config files (dav1d, zlib, iconv, and macOS's frameworks or
  Linux's OpenSSL and hardware decoding libraries), and the C compiler's
  runtime library they call into.
- `src/ui/theme.rs`: colors, every size, radius and duration the UI uses,
  and named text styles (`text_display`, `text_title`, `text_heading`,
  `text_label`).

On macOS, software-decoded frames are uploaded to gpui's texture atlas as
images, and hardware-decoded ones are drawn in place as surfaces. On Linux,
frames (hardware-decoded ones copied back first) are uploaded as their
Y'CbCr planes and converted as they are drawn. Either way the tile paints them itself, framed. Replaced frames are kept a few display frames
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
(skipped if it isn't installed) and check opening, accurate seeking, where
skips land, audio
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
settings file, staying out of text fields, and how they are shown. A clip
remuxed with a display matrix must report its turn and flip; unit tests
check that orientations compose like their matrices, read FFmpeg's own
matrices right, and that framing samples the right part of the picture,
turned, reshaped and cropped, with bitmap subtitles landing where the
picture shows what they cover. For the Linux GPU conversion, unit tests
check the sample matrices (limited and full range, 8 and 10 bits, in the low
or high bits of 16) against known colors, and which frames are handed over,
scaled or not, and where their chroma sits. With the sound delayed, the playhead and the
picture must run that far ahead of what the mixer has played; with
subtitles delayed either way, a cue must show only at its moved time,
including right after a seek past where it originally ended. With no audio
device, a player must still play in real time; unit tests check that what
is queued follows the mixer to another sample rate, from where it was. One
ignored test plays through the machine's own outputs, each by its id:

```bash
cargo test plays_through_the_devices_here -- --ignored --nocapture
```

Screenshots: a still of a player capped at 180 lines must still be the full
640x360, and exactly the frame shown (each frame a different grey) though
it isn't a keyframe; an HDR10 still kept as PQ must tone map to the same
picture as one saved for SDR. Unit tests check screenshot file names,
never replacing a file, turning and cropping on the CPU, bitmap subtitles
and boxes landing where they should, subtitles over HDR at reference
white, PNGs (8-bit sRGB, or 16-bit with cICP) and JPEGs, and the math
that turns light back into PQ and HLG signals. Bitmap subtitles on a turned
picture are sampled turned with it.

Picture in Picture's subtitles: drawn one over another they add up as
drawn straight onto the picture; over Y'CbCr, luma takes each pixel and
chroma the average of the four it covers, in 8-bit full range and 10-bit
video range samples (high in their 16 bits), and over BGRA; over HDR they
are at reference white.

Stream tests serve the clips from a small HTTP server on localhost:
HTTP files, HLS (normal and live), a connection that stalls mid-way,
unreachable hosts, and closing while a connection hangs.

## Limitations

- On Linux, frames decoded on the GPU are copied back to memory and
  uploaded again to be drawn (gpui's Linux renderer can't draw GPU surfaces
  such as DMA-BUFs), though converting them to RGB happens on the GPU. Dolby
  Vision, and formats other than 8- and 10-bit 4:2:0, are converted to BGRA
  on the CPU. Lower the decode resolution limit in Settings when playing
  many high-resolution files. On macOS, videos VideoToolbox can't decode,
  such as VP9, are converted on the CPU too.
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
  Subtitle files can't be dropped on a tile. Subtitles in the video file
  itself are read about a second ahead with it, so moved earlier than that
  they show late. In a live stream, a new track starts with its next packets
  rather than at the playhead.
- Picture in Picture is macOS only, from macOS 12. Its window shows the
  video turned (from macOS 13; before, as decoded) but not reshaped,
  cropped or with the gamma from Settings, and can't show Dolby Vision
  profile 5. AVKit doesn't fit the video to its window by itself, so Plays
  lays out AVKit's own layers there, which a later macOS may change.
- Screenshots of live streams can't be taken (the frame can't be found
  again). Text subtitles in a screenshot are laid out for the picture
  rather than the tile, so in a small tile, whose text is held to a least
  size, they come out relatively smaller. HDR JPEGs (with a gain map)
  aren't made, and HDR PNGs carry no mastering display or light level
  metadata.
- Picture adjustments and delays aren't kept: a file opens as its file
  has it. A display matrix that isn't a quarter turn goes to the nearest
  one, and text subtitles stay upright when the picture turns.
- No playlists or playback speed.
- Outputs aren't watched for coming and going while the one wanted is
  playing: the device list in Settings → Audio is read again when its menu
  opens. ALSA's own error messages about plugins with nothing behind them
  show in the terminal when it does.
- On GNOME Wayland (client-side decorations) the window can be moved from the
  toolbar but has no resize handles.
