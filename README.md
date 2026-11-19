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
- Player windows can float above other apps' windows, always or only while
  something in them plays: Video → Float on Top or Float on Top While
  Playing, or Settings → Appearance (they have no keys until given some in
  Settings → Hotkeys). Settings and About float with them, so they don't
  open underneath. On macOS a window comes down while it is full screen
  and floats again after.
- On macOS, Video → Output Device lists the displays by name and puts the
  player window full screen on the one picked, picture, subtitles and all.
  An AirPlay TV or projector is one of them once it's connected as a
  separate display, and macOS sends the sound there with it. AirPlay
  speakers play the sound alone once macOS has them, and the picture is
  held back as long as AirPlay delays the sound, so the two stay in step.
- A tile can play on a TV or a speaker instead (casting), from where it
  is, with its tracks: the cast button on the tile lists the devices
  found, and on macOS Video → Output Device lists those that show video
  under the displays, and Audio → Output Device the speakers under this
  computer's outputs. Speakers are offered for anything with sound; a
  video's sound goes to one while its picture plays on in the tile, in
  step with what the speaker plays, and the tile's buttons and shortcuts
  drive both. While it casts, the tile says where it plays and how the file gets there
  (as it is, repackaged, or re-encoded), and its bar, buttons and shortcuts
  drive the device, whose position moves the bar. The subtitles picked go
  along and change there as they change on the tile; picture ones (PGS,
  DVD) in the file go drawn into the picture where it is re-encoded, and
  stay here where it isn't. Another audio or video track, or subtitles
  drawn in changing, loads the file there again from where it is. Playing it here
  again (This Computer, Play Here, or the display in the menu), closing the
  tile, or the device going away ends the cast, and the tile goes on from
  where the device was. Quitting stops the devices cast to. Chromecasts,
  Google TVs and Nest speakers, and DLNA TVs and speakers, are found;
  AirPlay isn't wired in yet. Settings → Casting turns each protocol on
  or off and lists the devices found, with what each shows: video, or
  sound only.
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
- Shortcuts go to the tile the mouse is over, as its controls show there
  (or the one it was last over, or clicked): play/pause, seek, frame step,
  volume, tracks, subtitle size and height, audio and subtitle delay,
  rotation, flips, aspect ratio, crop, screenshots, full screen.
  With Shift, play/pause, seek, frame step, restart, volume and mute go to
  every file in the window (each tile's volume from its own), and Shift-S syncs them all to that tile; the menu
  bar (a toolbar on Linux) has them too.
- On macOS, windows show just the video, edge to edge, with the traffic
  lights over it like other video players. While something plays, they fade
  out with the controls when the mouse leaves the window or rests in it.
  Colors are macOS's
  own, in light and dark, with your accent color.
- Audio from every file is mixed into one output: the system's default, or
  a device picked in Settings or, on macOS, Audio → Output Device (which
  lists outputs as they are plugged in and checks the one picked, then
  the speakers to cast the tile in front to, checking the one it casts
  to instead), remembered from run to run. A/V sync follows the audio clock.
- Audio-only files show their cover art (or a note) and a level meter.
- About Plays, in the app menu on macOS: a small window with the app's
  icon, version and the day it was built (Settings → About says them too).
- Settings window: theme, master volume, the audio output, default volume, autoplay, loop,
  seek step, fit/fill scaling, gamma for SDR and for HDR video (applied to
  every open video as you drag), decode resolution limit, hardware decoding,
  subtitle style (font, weight, size, color, outline or shadow and its color,
  a background box, on the picture or anywhere in the tile, height), with a
  live preview, the protocols that look for devices to cast to and what
  they found, where screenshots go and what they are like, the keys for
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
static libraries under the LGPL. They decode, and have what the stream
server needs besides: the MP4, MPEG-TS, ADTS and FLAC muxers, the AAC,
AC-3, E-AC-3 and FLAC encoders, and the hardware video encoders (no
software one). It installs
them in `build/ffmpeg`, where `.cargo/config.toml` points the build, and
builds them again only when the script changes. It needs `pkg-config`,
`ninja` and `python3` (meson, which builds dav1d, is downloaded with the
sources), and `nasm` on x86_64.

The repository is a Cargo workspace: Plays, and beside it in `crates/` the
crates it is built from, each built and tested on its own. Cargo commands
run at the top act on Plays alone; `--workspace` takes in the crates too.

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
leaves out). NVDEC needs only NVIDIA's driver, at run time. The stream
server's video encoders are VA-API's, with libva, and NVENC, with NVIDIA's
codec headers (`libffmpeg-nvenc-dev`) at build time and its driver at run
time.

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
| Volume down/up on all | `⇧↓` `⇧↑` |
| Previous/next frame on all | `⌥⇧←` `⌥⇧→` / `Alt+Shift+←` `Alt+Shift+→` |
| Sync all to the active tile | `⇧S` |
| Close the file in front | `⌘W` / `Ctrl+W` (in an empty window, it closes the window) |
| Close all | `⌘⇧W` / `Ctrl+Shift+W` |
| Close the window | `⌥⌘W` / `Ctrl+Alt+W` |
| Settings | `⌘,` / `Ctrl+,` |

Shortcuts act on the active tile: the one the mouse is over (or was last
over, or clicked), or the maximized one; those with `⇧` act on every tile in the window. Sync all puts every
tile where the active one is, playing or paused as it is, so files side
by side go on together (the active one seeks there too); the frame keys
with `⇧` then step them all, each by its own frame rate. A short
note on the tile says what changed; switching tracks rolls a reel, like a
slot machine's, to the track picked, with the ones before and after it
faded above and below. To change shortcuts, open Settings → Hotkeys,
click + by a command and press the keys; a key another command had moves to
it, and Restore brings the defaults back. The app's own shortcuts above (⌘O, ⌘W
and so on) stay as they are.

On macOS, the Video menu also lists every aspect ratio and crop to pick
from, checking the active tile's; `A` and `C` go through them in turn,
then back to none. Rotating and
flipping go by the picture as it shows, and cropping keeps its middle;
opening another file in the tile starts it as its file has it. The same
goes for delays, which a note shows as they change ("Audio delay +200 ms").
The sound can move up to 5 seconds either way, subtitles up to a minute.

Settings → Audio picks the output everything plays through: the system's
default (which the sound follows as it changes, on macOS) or one device.
While the device picked isn't connected, players keep playing, silently,
until it is (or, with Wait for it off, through the system's default), and
the sound goes back to it as soon as it's there.

On macOS, Video → Output Device takes the window in front full screen onto
the display picked, and checks the one it's on; picking that one just puts
it full screen. To play on an AirPlay TV or projector, connect it first
from Control Center → Screen Mirroring and use it as a separate display:
it then shows in the menu (by the name macOS gives it), and its sound goes
with it. For sound alone, pick AirPlay speakers in Control Center → Sound:
they become the system's default, which Plays follows, and show in Audio →
Output Device and Settings → Audio like any other output. AirPlay sound
comes out late, by up to a couple of seconds; the picture waits for it.

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
scrub: the picture follows as you drag, and lands on the exact frame where
you let go. In the middle under the bar,
the previous and next buttons open the media file before or after this one
in its folder, by name, in the same tile; between them, skip back and
forward go by the seek step, around play/pause. The track button (next to
loop, on files that have subtitles or more than one audio or video track)
opens the track menu: a column each for audio, subtitles and (when there
are several) video. While there are devices to cast to, the cast button
before it opens the menu of them: This Computer, then each device and its
kind, the one playing checked. For the only tile in a window,
both toggle full screen instead. When a window has more than
one tile, the button next to maximize moves a tile to a new window sized for
it, where it keeps playing. Every window takes drops and works the same
way; menus and shortcuts act on the window in front, and files opened from
Finder go to it too. Drag the top edge of a window to move it. Closing the
last window quits.

Devices to cast to on the same network are found by themselves, in a
second or two. A Chromecast, Google TV or TV with Cast built in is
offered for anything, and a Nest speaker or speaker group for anything
with sound, a video's sound alone going to it as HLS while the picture
plays on here, following it. A file goes to it as it is where its model plays that kind
of file (MP4, WebM, MP3, AAC, FLAC, Ogg, WAV; just MP4 on a TV of another
make), else as HLS, re-encoded where it needs to be: H.264 up to 1080p on
the first Chromecasts and other makes' TVs, HEVC, VP9 and HDR up to 4K on
an Ultra or a Google TV, AV1 too on a Google TV Streamer. Subtitles show
there as Settings → Subtitles has them look (size, color, outline or
shadow and its color, the box behind them); turning them off and on, or
changing how they look, changes them there as it plays, and picking other
subtitles gives it the file again from where it is. The tile's volume is
that of what plays there, so the device's own stays where it is.

A DLNA TV or speaker is offered by what it lists: one that lists only
sound, for anything with sound. A file goes to it as it is where it
lists that kind of file, else as a stream of MPEG-TS, re-encoded where it
needs to be; a video's sound goes to a speaker as a stream of AAC or
FLAC, as internet radio does, while the picture plays on here, following
it. A speaker can't seek in such a stream, so it's given the stream from
where the tile is, and again from elsewhere to seek. Picking other subtitles gives it the file again from where it
is, as DLNA takes subtitles only with the file. The tile's volume scales
the TV's own: where it was when cast, the TV stays where it was.

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
  keyframe its way, which shows at once. Dragging the seek bar scrubs:
  each move shows the first frame decoded on its way at once, then one
  every 40 ms or so, then the frame asked for, decoding nothing past it and
  no sound, so nothing waits on buffering. A move up to two seconds past
  the frame decoded last decodes on from there rather than going back to a
  keyframe. Letting go seeks exactly. It plays one track of each kind;
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
- `vendor/gpui`: gpui 0.2.2 with HDR video on macOS and check marks on menu
  items (see its `PLAYS.md`). Its
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
  outputs and finds one by the id the system keeps for it. On macOS it
  adds the latency CoreAudio gives the device's stream, which cpal leaves
  out and AirPlay makes long, to how late what is mixed will be heard.
- `src/media/clock.rs`: per-player clock that runs on wall time and is eased
  toward the audio position on every audio callback.
- `src/media/player.rs`: the UI-side handle: play/pause/seek, frame selection,
  buffering after seeks (and, for streams, until a second is read ahead or
  when the network runs dry), end-of-file and looping, and the console's
  figures: frames shown and dropped, lateness, and what is queued.
- `src/app/`: starting the app and its own actions (`mod.rs`), the shared
  audio output and which device it plays through (`audio`: the one picked,
  waited for or stood in for while it's away, and watched for to come
  back; on macOS, CoreAudio says when outputs come and go), what each
  action does (`commands`), how actions reach the window and tile in front
  (`dispatch`), the keymap and the menu bar (set again as outputs,
  displays or devices to cast to come and go, another is picked, or the
  tile in front casts, turns out to have no sound, or is shaped or
  cropped, or another tile is in front).
- `src/hotkeys.rs`: the commands shortcuts can run, their default
  keys, how they are saved in Settings and shown (`⌃⌘F`, `Ctrl+Alt+F`).
  The keymap is rebuilt from them whenever they change, and the menus show
  them.
- `src/ui/`: gpui views (`open_url`, the player and Settings below) and a
  text field based on gpui's input example. `displays` lists the displays
  by the names macOS gives them and moves a window onto one, which gpui
  can't.
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
  `screenshot`; `pip` runs Picture in Picture for it, `cast` casts it),
  and the graveyard frames wait in before they are released. While a tile
  casts, its play, pause, seek, volume, loop and track changes go to the
  device, and what it shows (position, playing, the end) comes from it.
- `src/cast/`: casting. A protocol is a `Target`, which finds devices (a
  name, what they show, video or sound only, and an address) and connects
  to one for a `Session`: what it plays, for the stream server, then load,
  play, pause, seek, volume, subtitles, stop and status. Each protocol's
  crate is adapted to these. `Casting`, a global, keeps the protocols,
  starting each one Settings have on looking (its target) and dropping
  it when turned off, which stops it looking; a cast to one of its
  devices goes on, its link holding the target. It asks the targets every
  second what they have found, and starts the stream server for the first
  cast, on every interface. A tile's `Link` runs its
  session on a thread of its own, as every call may wait on the network:
  it connects, has the stream server serve the file with the tracks the
  tile plays, and loads it at the tile's position; then it does what the
  tile sends it (seeks in a row only the last), asks the device what it
  is doing twice a second (ten times while the picture follows the
  sound), and shows the tile what it asked for until the device has done
  it. A video's sound sent to a speaker is the one cast with the picture
  here: the tile keeps playing with its sound unheard, playing and pausing
  as the speaker is seen to and following where it says it is, the time
  it said it at moved on to now; its clock eases there, and seeks where it
  is far off. Dropping the link stops the device and the stream
  server lets the file go; as the app quits, it waits a moment for the
  devices to stop. `dlna` adapts the DLNA crate: it asks each renderer
  found what it plays (its protocol info), which decides whether it shows
  video and how the stream server serves it (the files it lists, else a
  stream of MPEG-TS rather than HLS, which fewer TVs take; H.264 and AAC
  in MPEG-TS for one that doesn't say; for a speaker given a video's
  sound, a stream of AAC or FLAC, by what it lists), and names the MIME
  type as the renderer lists it. While the picture follows the sound it
  asks the renderer where it is ten times a second. A stream of the sound
  starts where its URL says: a speaker refuses to seek in one and says
  how long it is as how much it has played, so it's given the stream from
  where it's to start, again to seek, and where it says it is counts from
  there, against the file's duration. Subtitles go as SRT, both ways the crate writes them;
  other subtitles load the file again from where it is. The renderer
  stopping after it played is the end; a seek it refuses while stopped is
  made once it plays; something else playing there ends the cast.
  `chromecast` adapts the Chromecast crate: devices that show video are
  offered for anything, speakers and groups for sound; the model each says
  it is decides how the stream server serves it, and HLS made for it says
  its kind of segments, which the receiver otherwise takes for MPEG-TS.
  Subtitles go as a WebVTT track styled from Settings, beside the HLS
  rather than in it; off and on is the track's, other subtitles load the
  media again with a track at a URL it hasn't fetched. The tile's volume
  is the stream's. Played to the end, it's loaded again to play again; it
  going idle stopped, or unable to play, ends the cast, as the crate's
  ways of ending do. `fake` is a protocol for tests.
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
  Video, Subtitles, Casting, Screenshots, Hotkeys, Appearance, About) is a file of its own with its
  name and icon for the sidebar; `form` has the cards, rows and sliders they
  are made of, and `file` the settings file's menu at the sidebar's foot. About shows the version and the credits, each of which opens
  to its license.
- `src/ui/workspace/`: a window, one per window, a file per part: opening
  files, folders and streams (`opening`), the tiles and moving one to its
  own window (`tiles`), sizing the window for the videos opened in it
  (`fit`), the grid, the empty window's ring (`empty`), the
  toolbar, the mouse coming, going and resting, and the traffic lights
  (`mouse`), floating on top (`on_top`), going full screen on another
  display (`display`: out of full screen, across, and in again), and what
  the Playback commands do to every tile (`commands`).
- `src/ui/components/`: the reusable components, one `RenderOnce`
  component per file, each in three sizes (small, medium, large) with its
  numbers for each in `theme`. An icon; a button in seven variants
  (normal, primary, secondary, danger, ghost, link, icon) with its icon
  on either side, and selected or disabled; a switch, a segmented control,
  a slider and a meter; menu rows, and a popover that opens a menu from a
  button; a badge; a flat card; and a floating panel to put anything in,
  a menu's or a note's over the picture.
- `build.rs`, with `crates/ffmpeg-link`: links the static FFmpeg with what
  it needs in turn, read from its pkg-config files (dav1d, zlib, iconv, and
  macOS's frameworks or Linux's OpenSSL and hardware decoding libraries),
  and the C compiler's runtime library they call into. Linking is a crate
  of its own because a build script's link arguments reach only its own
  package, and the stream server links FFmpeg too.
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

### The stream server

`crates/stream-server` serves media to TVs and speakers over HTTP: a
protocol's crate tells a device to play a URL, and this crate makes that
URL. It knows nothing of Plays or gpui, so it is tested on its own.

```
 file/URL + what the device plays
        │
      plan ──▶ plays as is ───────────────▶ the file by byte ranges, or the stream's own URL
        └────▶ else: each track copied ───▶ HLS segments (MPEG-TS or fragmented MP4),
               or re-encoded                an MPEG-TS stream, or a stream of the sound
                                            alone (AAC or FLAC), made as they are asked for
```

- `profile.rs`: what a device plays, as its protocol's crate knows it: the
  containers it takes as files, HLS (with MPEG-TS or fragmented MP4
  segments), a progressive MPEG-TS stream or a progressive stream of the
  sound alone (AAC as ADTS, or FLAC), and the codecs, size, bit
  depth, HDR kinds and channels it decodes; whether the HLS playlist lists
  the subtitles, which it doesn't for a device given them beside it.
- `media.rs`: opening a file or stream and reading what is in it.
- `plan.rs`: the least work that plays. The file as it is, or a stream's
  own URL, when the device plays it with the default tracks; else HLS, or
  for a DLNA TV without it an MPEG-TS stream, and for a DLNA speaker given
  a video's sound a stream of it alone, each track copied when the
  device decodes it and re-encoded when it doesn't. Video is re-encoded to
  H.264 or HEVC (10-bit and HDR kept in HEVC where the device shows them),
  no bigger than the device shows; sound to AAC, AC-3, E-AC-3 or (for a
  stream of FLAC) FLAC. Picture
  subtitles picked from the media are drawn into video re-encoded; video
  copied goes without them.
- `encode.rs`: the hardware video encoders only, VideoToolbox on macOS and
  NVENC or VA-API on Linux, each tried once and remembered; FFmpeg's own
  sound encoders, through a resampler and a FIFO of whole frames.
- `segments.rs`: where a file's segments start. Copied video can be cut
  only at its own keyframes, read from the file's index (or, with none, by
  reading the file); re-encoded video and sound alone are cut every six
  seconds, with a keyframe made there.
- `mux.rs`: fragmented MP4, MPEG-TS, ADTS or FLAC written in memory, cut into segments
  where told. Every timestamp is moved ten seconds later, so none is
  negative and segments made by different jobs line up.
- `job.rs`: making them. A job reads from where a segment starts, copies or
  re-encodes, and cuts where the leading stream (video, else sound) reaches
  the next segment's start, holding the other stream's packets past it
  until then. Jobs run three segments ahead of the device and wait; asking
  for one far from the job (a seek) starts another there, and a segment is
  cut the same way whichever job makes it. Packets without a length get
  one from the next. A live stream has one job, keeping the last ten
  segments and listing six; a progressive stream is made for each request,
  from the time it asks for.
- `overlay.rs`: picture subtitles drawn into video as it is re-encoded.
  The track's packets are decoded as the media is read; a display set
  shows until the next or its own end, and one with nothing in it clears
  the screen. Its pictures, their canvas stretched over the frame
  (interpolated larger, averaged smaller), are blended into each NV12 or
  P010 frame in the frame's colors, their white at graphics white in PQ
  and HLG.
- `subtitles.rs`: a text track of the media, or a subtitle file (from the
  encoding it is said to be in), read once and written as WebVTT (whole, or for each HLS
  segment, mapped to the media's timestamps) or SRT, keeping italics and
  bold.
- `playlist.rs`: the master playlist (bandwidth, size, frame rate,
  subtitles, and codec names read from each stream's parameter sets) and
  the media and subtitle playlists.
- `http.rs`, `server.rs`: a small HTTP/1.1 server with a thread for each
  connection, kept open between requests: GET, HEAD and OPTIONS, byte
  ranges, CORS for Chromecast, and DLNA's headers (content features, time
  seeks, Samsung's subtitles header). Each cast is served under a random
  token, at the address the device reaches this computer at.

### The AirPlay crate

`crates/airplay` finds AirPlay receivers, pairs with them and sends to
them: a TV is told to play a URL (one the stream server makes) and driven
while it does, and a speaker is sent live sound. It knows nothing of
Plays, gpui or FFmpeg. It blocks, on the caller's thread or a session's
own, and what a session says as it goes comes over a channel.

- `browser.rs`, `device.rs`: mDNS for `_airplay._tcp` and `_raop._tcp`, a
  receiver's two records put together by its id, and read for whether it
  shows video, plays sound and speaks AirPlay 2, and how it pairs: once
  with a PIN it shows, for each session only, or not at all. An AirPlay 1
  speaker that doesn't say its features, as older ones don't, plays sound.
- `tlv8.rs`, `srp.rs`, `pairing.rs`: HomeKit pairing. With the PIN (SRP-6a
  with SHA-512 and the 3072-bit group, then long-term Ed25519 keys swapped
  under ChaCha20-Poly1305), which leaves credentials to keep; transient,
  for receivers open to everyone; and pair-verify with the credentials,
  for each session after.
- `channel.rs`, `message.rs`, `connection.rs`: the control connection,
  RTSP and HTTP requests on one TCP connection, encrypted once paired in
  frames of up to 1024 bytes. A receiver on more than one network has an
  address on each, and not all reach it from here, so each is tried in
  turn.
- `session.rs`: AirPlay 2's session. SETUP says where our clock is, which
  the receiver asks the time; the receiver talks back on an event
  connection; feedback goes every two seconds. Feedback refused or the
  event connection closed is the receiver gone.
- `video.rs`: `/play` with the URL and where to start, `/rate`, `/scrub`
  and `/stop`, and `/playback-info` read as loading, playing, paused or
  ended.
- `audio.rs`, `rtp.rs`: 16-bit stereo PCM at 44.1 kHz in RTP packets of
  352 frames, each sent as it is due: to AirPlay 2 speakers as realtime
  audio, sealed with a key sent in SETUP, and to AirPlay 1 ones plain. An
  AirPlay 1 speaker that takes sound only encrypted gets each packet in
  AES-128 CBC, from the same IV each time, with the key and IV in the
  ANNOUNCE, the key wrapped (`rsa.rs`: RSA-OAEP with SHA-1) with the
  public key every AirPort Express holds the private half of. A sync
  packet every second ties RTP time to the clock, and the last thousand
  packets are kept to send again. A frame is heard 1.5 seconds after it's
  sent, plus what an AirPlay 1 speaker says it adds.
- `fake.rs`, with the `fake` feature: a pretend receiver on this computer,
  for tests here and in Plays. An encrypted AirPlay 1 fake holds a key
  of its own, made for it, rather than the AirPort's, and senders here
  wrap the sound's key for it with that.

`cargo run -p airplay --example airplay -- list` lists the receivers on
the network; `pair`, `play` and `tone` pair with a TV, have it play a URL,
and play a quiet tone on a speaker.

### The Chromecast crate

`crates/chromecast` finds Google Cast devices (Chromecasts, Google TVs,
Nest speakers and speaker groups) and has one play a URL, then drives it.
Like the AirPlay crate it knows nothing of Plays, gpui or FFmpeg, blocks on
the caller's thread or a session's own, and says what happens over a
channel.

- `browser.rs`, `device.rs`: mDNS for `_googlecast._tcp`; a record's TXT
  gives the id, name (`fn`), model (`md`), what it does (`ca`: video out,
  sound out, a group) and what it's showing (`rs`).
- `channel.rs`, `proto.rs`: TLS to the device (rustls; each device signs
  its own certificate, so any is taken, though the handshake is checked
  against it), and on it Chromium's `CastMessage` protobufs, each after its
  length, carrying JSON.
- `session.rs`: a cast. It connects to the device, launches the Default
  Media Receiver (or joins it) and connects to that; requests wait for the
  answer with their request id. A thread reads everything the device says:
  it answers pings and pings every five seconds, and passes on media
  status and volume as events. The cast ends when the device goes away or
  says nothing for 15 seconds, when the app closes or another one
  launches, or when another sender plays something else.
- `media.rs`: what LOAD takes (the URL, its type, buffered or live, a
  title and duration, WebVTT subtitle tracks, which to show and how they
  look, and for HLS its kind of segments) and the status read back: idle and why, buffering, playing or
  paused, with the position and duration.
- `fake.rs`, with the `fake` feature: a pretend device on this computer,
  TLS with a certificate made for it.

`cargo run -p chromecast --example chromecast -- list` lists the devices
on the network; `play` casts a URL to one and says what it does with it.

### The DLNA crate

`crates/dlna` finds UPnP media renderers (smart TVs from Samsung, LG, Sony
and others, and speakers such as Sonos or Devialet), has one play a URL and
drives it. Like the other two it knows nothing of Plays, gpui or FFmpeg,
blocks on the caller's thread or a session's own, and says what happens
over a channel.

- `ssdp.rs`, `browser.rs`: SSDP. A search for media renderers goes to
  `239.255.255.250:1900` on every interface, again a second later as
  datagrams get lost, and every two minutes after; renderers' NOTIFYs are
  heard on SSDP's port, shared with whatever else listens there. A
  renderer is dropped when it says byebye or isn't heard from within the
  max-age it gave.
- `device.rs`, `xml.rs`: the device description, read for the name,
  maker, model, icons and where AVTransport, RenderingControl and
  ConnectionManager are, of any version. The renderer may be a device
  inside another, as a Sonos speaker's is. ConnectionManager's protocol
  info is read into capabilities: the protocol, MIME type and DLNA profile
  of each.
- `http.rs`, `soap.rs`: just enough HTTP/1.1, and SOAP actions on it. A
  refusal's `UPnPError` becomes a typed fault, as each service numbers
  them.
- `media.rs`: the DIDL-Lite that goes with a URL (title, class, `res`
  with its protocol info, duration and size), with subtitles both as a
  second `res` and as Samsung's `sec:CaptionInfoEx`. What serves the media
  matters as much: TVs look for `transferMode.dlna.org`,
  `contentFeatures.dlna.org`, time seeks and `CaptionInfo.sec`, which the
  stream server sends, and a speaker seeks in a file by byte ranges.
- `session.rs`: SetAVTransportURI (after Stop, for a renderer that won't
  take another while it plays), Play, Pause, Seek (by absolute time when
  it won't by relative), Stop, the volume and mute. GENA subscriptions to
  AVTransport and RenderingControl send their events to a listener of the
  session's own, renewed halfway through; GetPositionInfo is asked every
  second, or as often as the session's told, with the transport state,
  and the volume every five seconds, when there are no events for them.
  A position is taken as heard halfway between the asking and the
  answer, when it first comes: a renderer saying where it is to the
  millisecond may move it on only once a second. The session ends when the renderer stops answering, or when it
  plays something it wasn't given here: what it's set to play, asked
  afresh, isn't ours.
- `fake.rs`, with the `fake` feature: a pretend renderer on this computer,
  a TV with events or a speaker without, saying where it is and how
  long what it plays is to the millisecond and, when told to, slow to
  answer.

`cargo run -p dlna --example dlna -- list` lists the renderers on the
network and what they play; `play` has one play a URL, with subtitles
when given, and says what it does.

## Tests

```bash
cargo test
```

The engine tests generate short clips with the `ffmpeg` command line tool
(skipped if it isn't installed) and check opening, accurate seeking, where
skips land, scrubbing (the frames it queues and nothing past them, no
sound, decoding on or going back to a keyframe), audio
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

Text subtitles in screenshots are drawn into memory by the machine's own
text system (Core Text, cosmic-text), on a test context the vendored gpui
adds for it: a cue must sit centered inside the picture, at the size and
height Settings give, twice as big on a picture twice as tall, wrap inside
the picture, and have the outline or box Settings pick. Known PQ and HLG
signals must land on known sRGB values, with and without gamma, reference
white just under SDR white; a Dolby Vision profile 5 frame of known colors
(black, greys, reference white, the primaries, a skin tone), made backwards
through its reshaping as an encoder would, must come back as those colors,
and moving its chroma must tint it the way IPT says. The expected values
were worked out apart from Plays' code, and each test has been checked to
fail when what it covers is broken.

Picture in Picture's subtitles: drawn one over another they add up as
drawn straight onto the picture; over Y'CbCr, luma takes each pixel and
chroma the average of the four it covers, in 8-bit full range and 10-bit
video range samples (high in their 16 bits), and over BGRA; over HDR they
are at reference white.

Stream tests serve the clips from a small HTTP server on localhost:
HTTP files, HLS (normal and live), a connection that stalls mid-way,
unreachable hosts, and closing while a connection hangs.

Windows are tested headless with `#[gpui::test]`, on gpui's test platform
(its `test-support` feature, in test builds only): simulated input, a clock
that moves only when told, and nothing drawn, so these check what the app
does and where things are, not pixels. `app::set_up_for_tests` starts the
app as it starts for real, but with default settings that are never saved
(no test reads or writes the real settings file), silent audio, and without
keeping the computer awake. Files open on real decoding threads, so tests
wait for them in real time. A file opened into a window must open in a
tile, and the window must then fit it. These tests open the small files in
`tests/fixtures`, committed so they run without the `ffmpeg` tool;
`script/make-fixtures` makes them again (writing the DVD subtitles'
pictures itself, as FFmpeg makes picture subtitles only from pictures).

`ui::testing` clicks, double-clicks and types as someone at the screen
would, finding what to click by its debug selector (`tile-0`, a menu row's
or a button's id), set only in test builds. It draws the window before it
looks, as what a file says once it opens doesn't draw it again by itself,
and moves the mouse to what it clicks first. A click picks a tile without pausing it, and
shortcuts then go to it; so does the mouse moving over one, but not while
it drags (a slider of the tile it came from); a double-click fills the window with a tile, the
whole area under the toolbar, and another puts the grid back; ⌘W closes the
tile in front, then the empty window; four tiles sit in two even rows.
`⇧S` puts every tile where the one clicked is, paused as it is, and
`⌥⇧→` and `⌥⇧←` step them all a frame at a time; with the one clicked
playing, the others go back to where it is and play. `⇧↑` and `⇧↓` turn
every tile up or down from its own volume. Settings' Reset to
Defaults asks first and changes nothing on Cancel, and
picking an output in the Audio menu sets it. Floating on top while playing
lifts the window, and Settings with it, only while its file plays, and
the menu items turn their mode on and off. Video → Output Device checks
the display the window in front is on, and picking a display puts the
window full screen, leaving full screen first when it goes to another.
Video → Aspect Ratio and Crop check what the tile in front has (as the
file is, and none, at first), as soon as it changes and as soon as the
mouse is over another tile.

Casting is tested with a fake protocol whose devices keep what they are
told and say what the test has them say, and the real stream server,
serving this computer only. Picking a device from the cast button plays
the file there from where the tile was, as it played (the device fetches
the MP4 as it is), and pauses the tile. Space, the arrows and `M` play,
pause, seek and mute it, not the tile, and the bar follows where it says
it is. Play Here goes on in the tile from where the device was, paused as
it was, the device stops and the file is no longer served. A device going
away ends the cast the same way, and the cast button goes with the last
device; quitting stops the device. Subtitles picked on a Matroska file
cast as HLS reach the device as WebVTT with the file's cues, and turning
them off hides them, without loading it again; another sound track loads
it again with them still going along. DVD subtitles picked stay
on this computer, the tile says, where the file goes as it is; where it is
re-encoded they are drawn in, the file loaded again from where it was at
a URL of its own, the first no longer served, and turning them off loads
it again without them (macOS). Speakers are offered for anything with
sound, and get a file of sound alone as it is. A video's sound goes to a
speaker as a stream of AAC, its picture staying on the tile, which waits
while the speaker plays but hasn't moved, plays once it's seen moving,
pauses with it where it stopped (from the speaker or the tile), and
keeps its place and its sound when the cast ends; dragging the slider
shows the picture where it's dragged, however often the speaker says
where it is, and the speaker seeks there once it's let go; a video
without sound isn't sent to one. Video → Output Device lists the
devices that show video after the displays, checking the one the tile in
front casts to instead of the display; a speaker it casts to leaves the
display checked. Audio → Output Device lists the speakers after this
computer's outputs for a tile with sound and none for one without,
checking the one cast to (or a TV) instead of the output. Settings →
Casting lists the devices found, a new one as it's found, and turning a
protocol off there saves it off, stops it looking (its target dropped),
takes its devices away and leaves a cast to one going; turned on, it
looks again and finds them. One off from the start never looks, and
other settings or protocols changing leave it as it is; one that can't
start says why and tries again when turned off and on. Each test has been checked to fail
when the behavior it covers is broken.

Casting to DLNA is tested with the DLNA crate's fake renderer, found as it
would be on the network. Picking the TV gives it the tile's Matroska file
as it is (the TV lists Matroska), seeks it to where the tile was, then
plays it, with the subtitles picked as SRT both ways the crate writes them;
both URLs serve what they should. A TV that lists MP4 and MPEG-TS (as
`video/mpeg`) only gets that file as a stream of MPEG-TS, by its own name
for it; the speaker, from its protocol info, is offered for anything
with sound and gets the M4A as it is, and a paused tile leaves it paused.
A video's sound goes to it as a stream of FLAC (it lists FLAC, not AAC),
re-encoded, the picture staying here; it's asked where it is several
times a second from the start, and seen moving, it plays on from when it
was first heard where it is, not from when it was last asked. From a
tile partway through, the speaker gets the stream at a URL that starts
there, with what's left as its duration, and is never told to seek;
where it says it is counts from there, the bar's length is the file's
however long the speaker says it is, and seeking gives it the stream
again from there, playing as it was, what it said of the last one not
taken for this one. Space, the
arrows, `M` and the volume keys play, seek, mute, turn down and pause the
TV (its volume scaled from where it was); the bar follows where it says it
is, and its playing to the end shows the end. Other subtitles give it the
file again, from where it was, with them or without. A TV that refuses to
seek before it plays, as Rygel's renderers do, goes there once it plays,
the bar staying where the tile was meanwhile. Something else playing
there, or the TV going away, ends the cast. What the renderer
plays decides what it shows and its profile, from Samsung's, LG's, Sonos's
and Devialet's protocol info, and the tile's volume scales the
renderer's. Each test has been checked to fail when the behavior it covers
is broken.

Casting to a Chromecast is tested with the Chromecast crate's fake device,
over TLS as a real one. Picking the TV loads the tile's Matroska file
there as HLS of MPEG-TS, saying so, from where the tile was and playing,
with the subtitles picked as a WebVTT track (its language as BCP 47) shown
in the style Settings give them; the playlist doesn't list them too, and
the track's URL serves the file's cues. The speaker is offered for
anything with sound and gets the M4A as it is, and a paused tile leaves it paused. An
MP4 goes as it is; space, the arrows, `M` and the volume keys play, seek,
mute, turn down and pause it, the volume being the stream's and the
device's own left alone; the bar follows where it says it is, its playing
to the end shows the end, and playing again loads it again from the start,
turned down as before. Picking subtitles loads it again from where it is
with a track at a new URL; turning them off and on again is the track's,
with no load. Another sender casting there, the app closing or another
launching, the device going away, and it stopping or failing to play end
the cast, leaving what plays there alone. Unit tests check the profile for
each model (the first Chromecasts, an Ultra, a Google TV, the HD one, the
Streamer, another make's TV, a speaker), the subtitle style from Settings,
and language codes as BCP 47. Each test has been checked to fail when the
behavior it covers is broken.

`cargo test --workspace` tests the crates in `crates/` too. The stream
server's tests serve the fixtures over HTTP, as a device fetches them, and
read what comes back with FFmpeg: the plan for each fixture; a file as it
is, with byte ranges, HEAD, CORS, DLNA's headers and nothing served but
what was cast; HLS in both kinds of segments, each starting with a keyframe
(and MPEG-TS ones with their tables) at its time, with every frame and all
the sound once; a segment asked for first, made by a job starting there,
the same as one made from the start; sound re-encoded to AC-3; video
re-encoded smaller on VideoToolbox, in both kinds of segments, each
holding only the sound that plays during it though the encoder hands
frames back late (macOS; with no hardware encoder, as in the Linux test
container, planning it must fail); a progressive stream from a DLNA time
seek; a video's sound alone for a speaker taking only files, as ADTS
copied, encoded from Opus to AAC, and as FLAC, seeking by time too, and
nothing for a video's picture or a speaker without such streams; a live
stream; subtitles from a file and from a track in
`clip-subs.mkv`, as WebVTT, SRT and an HLS rendition, which a device
given them beside it doesn't find in the playlist; and the DVD subtitles
in `clip-dvdsub.mkv`, left out of video copied and not served as text,
and drawn into video re-encoded, each frame at a cue's time differing
from the same frame without them where its cue's bar is and nowhere else
(macOS). Unit tests check how far ahead a job runs, where it restarts and
what it forgets, packets given their lengths, byte ranges, codec names
from parameter sets, playlists, subtitle text, and picture subtitles'
timing, colors (PQ and HLG at graphics white), scaling and blending into
NV12 and P010. Each has been checked to fail when what it covers is
broken.

The AirPlay crate's tests talk to the fake receiver as to a real one: a TV
paired once with its PIN, its credentials kept as text and verified for
each session after; a wrong PIN, and credentials for another TV; a URL
played, paused, sought and stopped and the volume set, every request
encrypted, and what the TV says read as playing, paused and ended; sound
to an AirPlay 2 speaker, sealed and sent as it is due, with its sync
packets, our clock's answer and the latency; packets sent again when asked
for; volume and a flush; sound to an AirPlay 1 speaker, plain, with the
delay it adds; sound to one that takes it only encrypted, which it
decrypts with the key it unwraps from the ANNOUNCE back to what was sent,
and one that takes only FairPlay refused; and a receiver that goes away or
refuses feedback ending the session. Unit tests check SRP against RFC
5054's vectors, both sides of pairing, the encrypted frames, AES-128 CBC
against NIST's vectors, RSA-OAEP both ways, base64, TLV8, records and
their features, RTP packets and playback info. The key wrapped with the
AirPort's public key has been checked to unwrap with its private one.
Each has been checked to fail when what it covers is broken.

The Chromecast crate's tests cast to the fake device over TLS: connecting
launches the Default Media Receiver and connects to it, the heartbeat goes
both ways and keeps the cast past the silence that would end it; LOAD
carries the URL, where to start, the subtitle track and its style; play,
pause, seek, subtitles, the device's volume and the stream's reach it,
the one leaving the other; its status and volume come as
events, the end of what plays too; a refused LOAD says so; loading again
isn't taken for another sender; stopping closes the app without telling of
it; a speaker takes a cast; and the cast ends when the device goes away or
stops answering, when the app closes or another launches, and when another
sender plays something else. Unit tests check the protobuf framing, the
JSON LOAD sends and the status read back, and records from a Google TV, a
Nest speaker and a group. Each has been checked to fail when what it
covers is broken.

The DLNA crate's tests use the fake renderer over HTTP and SSDP on this
computer: it's found by searching and by its advert, and dropped on byebye
and when its max-age runs out; its capabilities come from its protocol
info; SetAVTransportURI carries the URL and the DIDL-Lite with subtitles
both ways; play, pause, seek (by absolute time for a speaker that takes
only that) and volume reach it; refusals come back as typed faults;
loading while it plays stops it first when it must; events tell what a TV
does and asking tells what a speaker does, its position taken as heard
when it first came, halfway between the asking and a slow answer, and
asked again at once and then as often as told;
subscriptions are renewed, made
again when the renderer forgot them, and ended; and the session ends when
the renderer goes away or plays something else, but not for what played
before or a track that lags. Unit tests read descriptions and protocol
info in the forms Samsung, LG and Sonos give them and as a Devialet
speaker gave them (ids changed), SSDP's messages, DIDL-Lite, times, URLs
and HTTP. Each has been checked to fail when what it covers is broken.

### Smoke runs

Headless windows have no native window behind them, so a bug between
gpui and the system, such as a window laid out at its old size after it
fits a video, can't show up there. Smoke runs drive the real app instead:

```bash
script/smoke                    # every scenario
script/smoke close subtitles    # just these
```

It builds Plays with the `smoke` feature, into `target/smoke` with its own
cargo profile so the release build stays as it is. That build's settings
are the defaults, never read or saved. Each scenario runs in its own
process (`target/smoke/plays --smoke <name>`, `--smoke list` for the
names): `first-open` opens a video into the empty window, which must fit
it; `several` opens two videos and a sound into one window; `close` closes
the tiles and then the window with ⌘W, and the app must quit; `subtitles`
turns the subtitles beside a video off and on again; `step-back` steps a
frame forward and two back through B-frames; `float-on-top` floats the
window while it plays, as the system has it (the window's level on macOS,
`_NET_WM_STATE_ABOVE` on X11), brings it down when paused, and floats
Settings with it. Throughout, a window that has
fitted its files must be laid out at the size it really is, every playing
video must show at least 15 of its 25 frames a second, the sound must go
to a device, and nothing may log an error, whatever `RUST_LOG` lets
through. The clips loop and play at no volume. A window must stay
uncovered while it runs, since one that isn't drawing fails. On Linux with
no display, the script starts Xvfb with openbox, as gpui only draws under
a window manager, and PulseAudio, whose null sink plays in real time;
Vulkan can be software (lavapipe). Resizing the window on macOS from
inside its own update again, which leaves gpui laying it out at its old
size, makes `first-open` fail.

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
- Windows can't float on top under Wayland, which has no way for an app to
  ask for it; on X11 it takes a window manager that keeps windows above
  (`_NET_WM_STATE_ABOVE`), as most do.
- The stream server re-encodes video only on a hardware encoder, so a
  computer without one (Linux without NVENC or VA-API) can't cast video a
  device doesn't play as it is. Video decodes in software for it. HDR
  re-encoded for a device without HDR isn't tone mapped, and re-encoded
  HDR keeps no mastering or light level metadata. Picture subtitles (PGS,
  DVD) go only drawn into video re-encoded: where the video goes as it is
  they stay out, as do those in a file of their own, and one already
  showing where a job starts (a seek) shows only from the next. Text
  subtitles lose their placement, and a track in the media is read
  through the whole file first.
- AirPlay is Apple's own protocol, unpublished; the AirPlay crate speaks
  it as others have worked it out, so an update to a TV or speaker can
  break it. It doesn't take receivers with a password, or AirPlay 1
  speakers that take sound only in FairPlay or MFi encryption. Sound goes
  uncompressed, which a busy Wi-Fi network may not keep up with.
- The Chromecast crate takes whatever certificate a device offers, not
  checking it against Google's device certificates, so it can't tell a
  genuine device from something on the network pretending to be one.
- The DLNA crate finds renderers over IPv4 only, and gives subtitles in the
  forms most TVs take, as DLNA has none of its own; a TV may take neither.
  A renderer that neither sends events nor answers GetPositionInfo can't
  be followed.
- Casting to a Chromecast hasn't been checked with a real one yet, only
  the fake device: none was on hand. What one plays goes by the model it
  says it is, and a Google TV that calls itself just "Chromecast" is
  taken for one of the first, getting H.264 up to 1080p; a TV of another
  make gets H.264 and AAC in MP4 or HLS, which it may play more than.
  Surround sound goes as stereo AAC, and Dolby Vision is re-encoded. A
  receiver that applies the stream's volume to the device's own turns that
  instead. Subtitles take Settings' font, size, colors, edge and box, the
  weight only as bold or not, and not their placement or the area they go
  in. An HLS stream from the internet is handed over as it is, and one
  whose server doesn't let other sites have it (CORS) won't play.
- Casting: Chromecast and DLNA are wired in so far. Subtitles on a DLNA TV
  depend on the TV, as DLNA has none of its own: most take SRT one way or
  the other, some neither, and changing them loads the file again. A DLNA TV
  has only its own volume, which the tile's scales. One that doesn't say
  what it plays gets H.264 and AAC in MPEG-TS, which it may still not
  play, and one that won't seek while stopped starts from the beginning a
  moment first. While a tile casts, delays don't apply, as the device plays the picture and the
  sound; gamma, crop, rotation, flips, aspect ratio, and picture subtitles
  where the video isn't re-encoded, stay on this computer (the tile says
  so), and frames can't be stepped.
  The stream server listens on every interface, so the firewall may ask to
  let Plays take incoming connections, and on macOS 15 and later looking
  for devices asks to use the local network.
- Plays can't find or connect to AirPlay receivers itself: macOS keeps
  that to its own picker and to AVPlayer, which Plays doesn't play
  through. Connect one from Control Center first. Mirrored, an AirPlay
  display shows the whole screen; as a separate display, just the window.
  The Output Device menus are macOS only.
