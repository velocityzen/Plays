# Contributing to Plays

How to build Plays, run it from source, and test it. [README.md](README.md)
says what the app does, and [ARCHITECTURE.md](ARCHITECTURE.md) how it
works.

- [Building](#building)
  - [macOS](#macos)
  - [Linux (Debian/Ubuntu)](#linux-debianubuntu)
  - [The macOS app](#the-macos-app)
  - [Licenses](#licenses)
- [Tests](#tests)
  - [Running them](#running-them)
  - [The media engine](#the-media-engine)
  - [Screenshots and drawing](#screenshots-and-drawing)
  - [Windows](#windows)
  - [Casting](#casting)
  - [The crates](#the-crates)
  - [Fixtures](#fixtures)
  - [Smoke runs](#smoke-runs)
- [Conventions](#conventions)

## Building

You need a Rust toolchain (1.85+, edition 2024) and `clang` (FFmpeg's
bindings are generated with bindgen).

FFmpeg is built into the app. `script/build-ffmpeg` builds it once from
source, which takes a few minutes: FFmpeg 9.0.2 with dav1d for AV1, as
static libraries under the LGPL. They decode, and have what the stream
server needs besides: the MP4, MPEG-TS, ADTS and FLAC muxers, the AAC,
AC-3, E-AC-3 and FLAC encoders, and the hardware video encoders (no
software one). It installs them in a folder named for the version,
`build/ffmpeg-9.0.2`, where `.cargo/config.toml` points the build, and
builds them again only when the script changes (or with `--force`).
FFmpeg's Rust bindings are made again only when that folder changes, not
when the headers in it do, so a new version is named in both the script
and `.cargo/config.toml`; the script stops if they don't match. Sources
are checked against the checksums in the script. It needs `pkg-config`,
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
Homebrew's (which is GPL, and which the app then needs installed to run),
name the folder Homebrew keeps its version in, so the bindings are made
again when Homebrew upgrades it:

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

gpui renders with Vulkan on Linux, so a working Vulkan driver is required
to run it; see [README.md](README.md#linux) for the drivers for hardware
decoding.

### The macOS app

To open files from Finder (**Open With**) or by dropping them on the Dock
icon, build the app bundle:

```bash
script/bundle-macos
```

This builds FFmpeg first if it isn't built, writes the credits again
(below), and makes `target/release/Plays.app`, which runs on any Mac. It
always uses the app's own FFmpeg, even where `FFMPEG_DIR` names another.
The script compiles the app icon, the Icon Composer file
`assets/AppIcon.icon`, into the app, which takes Xcode 26 or later
(without that file, the app gets the generic icon), and signs the bundle
ad hoc. Once the app is in `/Applications`, or has been opened once,
Finder offers it under Open With.

### Licenses

Plays is under the MIT license (`LICENSE`). It is built with FFmpeg (LGPL
2.1 or later) and dav1d (BSD 2-Clause), from their unchanged source, and
with the Rust crates in `Cargo.lock`. Settings → About lists them all with
their licenses, from `resources/attributions.json`; `script/attributions`
writes it again when the dependencies change (`script/bundle-macos` runs
it). It lists the crates built into the app on macOS and on Linux (not
those that only run while building it), and a crate that comes with no
license text gets the standard text of its license, from SPDX's list.

## Tests

### Running them

```bash
cargo test               # Plays
cargo test --workspace   # Plays and the crates in crates/
```

The engine tests generate short clips with the `ffmpeg` command line tool,
and are skipped if it isn't installed. Dolby Vision profile 5 can't be made
with the `ffmpeg` tool, so that test runs only on a file you name,
comparing both paths on two of its frames (and saving them as PNGs):

```bash
PLAYS_DOLBY_VISION_SAMPLE=path/to/profile5.mkv cargo test dolby -- --nocapture
```

One ignored test plays through the machine's own outputs, each by its id:

```bash
cargo test plays_through_the_devices_here -- --ignored --nocapture
```

### The media engine

The engine tests check opening, accurate seeking, where skips land,
scrubbing (the frames it queues and nothing past them, no sound, decoding
on or going back to a keyframe), audio resampling through the mixer, end
of file, looping, a file whose colors FFmpeg has no names for (played to
the end, with them unknown), keeping up when frames are consumed rarely, a
decoder thread panicking (saying so and waiting no more), a paused
player's decoder sleeping, and hardware decoding (VideoToolbox is required
on macOS); unit tests read timestamps at the ends of what they can be, and
take colors FFmpeg has no names for as unspecified. The mixer's tests
count, with an allocator of their own, that mixing neither allocates nor
frees, and hold its locks and the clock's on another thread to see it
doesn't wait for them. On macOS, zero-copy frames are read back and run
through the surface shader's math, and must match the software path's
colors for SD and HD video, tagged or not, limited or full range. HDR10
and HLG clips (needs `libx265`) must come out as tagged 10-bit surfaces
whose HDR shader math, for an SDR screen, matches the CPU tone mapping.

A Matroska file with two audio and two SRT tracks checks that tracks are
listed and named, that switching audio while playing brings in the other
track, and that subtitles show, change track and turn off, at the right
times. A clip with an English SRT and a Windows-1251 Russian one beside it
checks that they are listed and named, that the first shows from the
start, that seeking back lands in a cue already showing, and that the
Russian one reads right. With its subtitle files slow to open and read, as
on a share far away, a clip must show its first frame without waiting to
read them or to open them one by one, and play on while a picked one is
read, showing it once it is. DVD subtitles drawn as a clip plays are
dropped from the GPU once they are off screen. Unit tests cover finding
files for a video, reading their names and telling encodings, and the
shortcuts: no default key used twice, a key moving to the command it is
given, reading and writing them in the settings file, staying out of text
fields, and how they are shown. The text field's tests compose as an IME
does: the cursor stays in what is composed, counted within it even after
other text and wider characters, deleting it leaves an empty field to type
into, and ranges outside the text are kept inside it. The settings file is
read a value at a time, so one that's wrong leaves only its setting as
Plays starts it out; a file not read whole, half written or from a later
Plays, is kept beside it as `settings.json.bad`, and a good one, or one
from before there was a version, is read as it is; and saving replaces a
file whole rather than writing over it.

A clip remuxed with a display matrix must report its turn and flip; unit
tests check that orientations compose like their matrices, read FFmpeg's
own matrices right, and that framing samples the right part of the
picture, turned, reshaped and cropped, with bitmap subtitles landing where
the picture shows what they cover. For the Linux GPU conversion, unit tests
check the sample matrices (limited and full range, 8 and 10 bits, in the
low or high bits of 16) against known colors, and which frames are handed
over, scaled or not, and where their chroma sits.

With the sound delayed, the playhead and the picture must run that far
ahead of what the mixer has played; with subtitles delayed either way, a
cue must show only at its moved time, including right after a seek past
where it originally ended. With no audio device, a player must still play
in real time; unit tests check that what is queued follows the mixer to
another sample rate, from where it was. A unit test opens devices that
take their time to open, and the app must go on meanwhile, at first and
when Settings pick another, then play through each once it's open.

Stream tests serve the clips from a small HTTP server on localhost:
HTTP files, HLS (normal and live), a connection that stalls mid-way,
unreachable hosts, and closing while a connection hangs.

### Screenshots and drawing

Screenshots: a still of a player capped at 180 lines must still be the
full 640x360, and exactly the frame shown (each frame a different grey)
though it isn't a keyframe; an HDR10 still kept as PQ must tone map to the
same picture as one saved for SDR. Unit tests check screenshot file names,
never replacing a file, turning and cropping on the CPU, bitmap subtitles
and boxes landing where they should, subtitles over HDR at reference
white, PNGs (8-bit sRGB, or 16-bit with cICP) and JPEGs, and the math
that turns light back into PQ and HLG signals. Bitmap subtitles on a
turned picture are sampled turned with it.

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
are at reference white. Its progress bar runs and stops with playback
whether the tile's buttons or the window's own play and pause it.

The looks of the panels over the video are checked in pixels on macOS,
where `gpui::render_offscreen` (its test support) draws a panel over a
picture with the renderer windows use, off screen, and reads it back.
Frosted blurs and dims what's under it, and leaves what's beside it sharp;
tint shows the colors around and nothing of their shapes; glass lights its
rim, is clear at its edge where its top is frosted, and bends what's just
inside the edge out to it; glow lights up what's around it in its colors,
which frosted doesn't. Linux draws them with the same shaders in WGSL,
which the smoke runs load.

### Windows

Windows are tested headless with `#[gpui::test]`, on gpui's test platform
(its `test-support` feature, in test builds only): simulated input, a clock
that moves only when told, and nothing drawn, so these check what the app
does and where things are, not pixels. `app::set_up_for_tests` starts the
app as it starts for real, but with default settings that are never saved
(no test reads or writes the real settings file), silent audio, and without
keeping the computer awake. Files open on real decoding threads, so tests
wait for them in real time. A file opened into a window must open in a
tile, and the window must then fit it.

`ui::testing` clicks, double-clicks and types as someone at the screen
would, finding what to click by its debug selector (`tile-0`, a menu row's
or a button's id), set only in test builds. It draws the window before it
looks, as what a file says once it opens doesn't draw it again by itself,
and moves the mouse to what it clicks first.

What the window tests check:

- A click picks a tile without pausing it, and shortcuts then go to it; so
  does the mouse moving over one, but not while it drags (a slider of the
  tile it came from).
- A double-click fills the window with a tile, the whole area under the
  toolbar, and another puts the grid back, as opening more files does;
  ⌘W closes the tile in front, then the empty window; four tiles sit in
  two even rows.
- A tile dragged by its picture onto another takes that one's place, the
  tiles between moving over, and shortcuts then go to it; it keeps playing,
  isn't maximized, and let go over itself, stays. The only tile in a
  window, and a maximized one, move the window instead; a press that
  barely moves doesn't, nor does any in full screen.
- A file closed partway in opens there again; one barely begun starts
  over, and so does every file with Settings' resuming off.
- `⇧S` puts every tile where the one clicked is, paused as it is, and
  `⌥⇧→` and `⌥⇧←` step them all a frame at a time; with the one clicked
  playing, the others go back to where it is and play. `⇧↑` and `⇧↓` turn
  every tile up or down from its own volume.
- In a file with three chapters, `Page Down` goes to the second and then
  the third, and no further; `Page Up` goes back to the second, then the
  first; the bar has a notch at the second and third, none with Settings'
  notches off. Playback → Chapters lists them, checking the one the tile in
  front is in, and follows the tile the mouse is over.
- Settings' Reset to Defaults asks first and changes nothing on Cancel,
  and picking an output in the Audio menu sets it.
- A window playing through an output of its own: with one output the
  tile's menu offers none; with two, picking one plays the tile there, a
  file opened in the window plays there too, a tile popped out takes the
  device along and shares its output, going back to the app's output
  leaves the other window on the device, and the output closes with its
  last window.
- A window sent full screen to a display plays through the output named
  after it, follows it to another, plays as it did on one without
  speakers, and goes back to the app's output when it comes back; full
  screen where it is, a device picked for it before or while away, and
  Settings saying not to, keep the sound where it is.
- Floating on top while playing lifts the window, and Settings with it,
  only while its file plays, and the menu items turn their mode on and off.
- Video → Output Device checks the display the window in front is on, and
  picking a display puts the window full screen, leaving full screen first
  when it goes to another.
- Black out covers the other displays while a window is full screen, and
  only as Settings say, and it and the Output Device menus go by the
  displays as last looked at rather than asking the system each time.
- Video → Aspect Ratio and Crop check what the tile in front has (as the
  file is, and none, at first), as soon as it changes and as soon as the
  mouse is over another tile.
- Picking a look for the panels over the video in Settings → Appearance
  sets it, and the controls of a paused video then show it through them
  with that look's backdrop, and none when plain.
- Settings → About, scrolled hundreds of credits down, builds only the
  few rows in view to draw, and a license there opens and closes.

### Casting

Casting is tested with a fake protocol whose devices keep what they are
told and say what the test has them say, and the real stream server,
serving this computer only. Picking a device from the cast button plays
the file there from where the tile was, as it played (the device fetches
the MP4 as it is), and pauses the tile. The next file goes there too,
with the subtitles beside it, and the previous one to a speaker sends
only its sound, the picture staying on the tile. Space, the arrows and `M` play,
pause, seek and mute it, not the tile, and the bar follows where it says
it is; so do the buttons of a Picture in Picture window, which isn't
offered while the picture plays there, and they play and pause a speaker
a video's sound is cast to. Play Here goes on in the tile from where the
device was, paused as it was, the device stops, and with no cast left the
stream server stops listening. A device going away ends the cast the same
way, and the cast button goes with the last device; quitting stops the
device and keeps where it was to resume from, and closing the last window,
which quits, waits for it to stop too. Subtitles picked on a Matroska file
cast as HLS reach the device as WebVTT with the file's cues, and turning
them off hides them, without loading it again; another sound track loads
it again with them still going along. DVD subtitles picked stay on this
computer, the tile says, where the file goes as it is; where it is
re-encoded they are drawn in, the file loaded again from where it was at a
URL of its own, the first no longer served, and turning them off loads it
again without them (macOS). Speakers are offered for anything with sound,
and get a file of sound alone as it is. A video's sound goes to a speaker
as a stream of AAC, its picture staying on the tile, which waits while the
speaker plays but hasn't moved, plays once it's seen moving, pauses with
it where it stopped (from the speaker or the tile), and keeps its place
and its sound when the cast ends; dragging the slider shows the picture
where it's dragged, however often the speaker says where it is, and the
speaker seeks there once it's let go; a video without sound isn't sent to
one. Video → Output Device lists the devices that show video after the
displays, checking the one the tile in front casts to instead of the
display; a speaker it casts to leaves the display checked. Audio → Output
Device lists the speakers after this computer's outputs for a tile with
sound and none for one without, checking the one cast to (or a TV) instead
of the output. Settings → Casting lists the devices found, a new one as
it's found, and turning a protocol off there saves it off, stops it
looking (its target dropped), takes its devices away and leaves a cast to
one going; turned on, it looks again and finds them. A protocol that takes
a while to stop doesn't hold up the windows: it stops on another thread.
One off from the start never looks, and other settings or protocols
changing leave it as it is; one that can't start says why and tries again
when turned off and on. Unit tests check that of what the tile sends at
once, only the last of seeks in a row is done, and only the last volume,
looping and subtitles asked for. Each test has been checked to fail when
the behavior it covers is broken.

Casting to DLNA is tested with the DLNA crate's fake renderer, found as it
would be on the network. Picking the TV gives it the tile's Matroska file
as it is (the TV lists Matroska), seeks it to where the tile was, then
plays it, with the subtitles picked as SRT both ways the crate writes them;
both URLs serve what they should. A TV that lists MP4 and MPEG-TS (as
`video/mpeg`) only gets that file as a stream of MPEG-TS, by its own name
for it; the speaker, from its protocol info, is offered for anything with
sound and gets the M4A as it is, and a paused tile leaves it paused. A
video's sound goes to it as a stream of FLAC (it lists FLAC, not AAC),
re-encoded, the picture staying here; it's asked where it is several times
a second from the start, and seen moving, it plays on from when it was
first heard where it is, not from when it was last asked. From a tile
partway through, the speaker gets the stream at a URL that starts there,
with what's left as its duration, and is never told to seek; where it says
it is counts from there, the bar's length is the file's however long the
speaker says it is, and seeking gives it the stream again from there,
playing as it was, what it said of the last one not taken for this one.
Space, the arrows, `M` and the volume keys play, seek, mute, turn down and
pause the TV (its volume scaled from where it was); the bar follows where
it says it is, and its playing to the end shows the end. Other subtitles
give it the file again, from where it was, with them or without. A TV that
refuses to seek before it plays, as Rygel's renderers do, goes there once
it plays, the bar staying where the tile was meanwhile. Something else
playing there, or the TV going away, ends the cast. What the renderer
plays decides what it shows and its profile, from Samsung's, LG's, Sonos's
and Devialet's protocol info, and the tile's volume scales the renderer's.
Each test has been checked to fail when the behavior it covers is broken.

Casting to a Chromecast is tested with the Chromecast crate's fake device,
over TLS as a real one. Picking the TV loads the tile's Matroska file
there as HLS of MPEG-TS, saying so, from where the tile was and playing,
with the subtitles picked as a WebVTT track (its language as BCP 47) shown
in the style Settings give them; the playlist doesn't list them too, and
the track's URL serves the file's cues. The speaker is offered for
anything with sound and gets the M4A as it is, and a paused tile leaves it
paused. An MP4 goes as it is; space, the arrows, `M` and the volume keys
play, seek, mute, turn down and pause it, the volume being the stream's
and the device's own left alone; the bar follows where it says it is, its
playing to the end shows the end, and playing again loads it again from
the start, turned down as before. Picking subtitles loads it again from
where it is with a track at a new URL; turning them off and on again is
the track's, with no load. Another sender casting there, the app closing
or another launching, the device going away, and it stopping or failing
to play end the cast, leaving what plays there alone. Unit tests check the
profile for each model (the first Chromecasts, an Ultra, a Google TV, the
HD one, the Streamer, another make's TV, a speaker), the subtitle style
from Settings, and language codes as BCP 47. Each test has been checked to
fail when the behavior it covers is broken.

### The crates

`cargo test --workspace` tests the crates in `crates/` too.

ffmpeg-link's tests check that `.cargo/config.toml` names the folder for
the FFmpeg version `script/build-ffmpeg` builds, and that the compiler's
runtime library is found when `CC` puts the compiler behind a wrapper.

The stream server's tests serve the fixtures over HTTP, as a device
fetches them, and read what comes back with FFmpeg: the plan for each
fixture; a file as it is, with byte ranges, HEAD, CORS, DLNA's headers and
nothing served but what was cast, and only to the device it was cast to;
HLS in both kinds of segments, each starting with a keyframe (and MPEG-TS
ones with their tables) at its time, with every frame and all the sound
once; a segment asked for first, made by a job starting there, the same as
one made from the start; sound re-encoded to AC-3; video re-encoded
smaller on VideoToolbox, in both kinds of segments, each holding only the
sound that plays during it though the encoder hands frames back late
(macOS; with no hardware encoder, as in the Linux test container, planning
it must fail); video whose only keyframe is its first re-encoded, not
copied as one segment (macOS); colors FFmpeg has no names for read as
unknown, and video with them re-encoded to 10-bit HEVC (macOS); a read
failing partway through a file, the segment being made an error and the
next made whole by a job of its own; sound re-encoded over a gap in
`clip-sound-gap.mkv`, going on with the picture after it, the same from a
job starting there; a progressive stream from a DLNA time seek; a video's
sound alone for a speaker taking only files, as ADTS copied, encoded from
Opus to AAC, and as FLAC, seeking by time too, and nothing for a video's
picture or a speaker without such streams; a live stream; subtitles from a
file and from a track in `clip-subs.mkv`, as WebVTT, SRT and an HLS
rendition, which a device given them beside it doesn't find in the
playlist; and the DVD subtitles in `clip-dvdsub.mkv`, left out of video
copied and not served as text, and drawn into video re-encoded, each frame
at a cue's time differing from the same frame without them where its cue's
bar is and nowhere else (macOS). Unit tests check how far ahead a job
runs, where it restarts and what it forgets, packets given their lengths,
byte ranges, a request's head too long refused before it's all read, one
sent a byte at a time cut off when a whole one would be, connections past
the most served at once closed and their places given back, codec names
from parameter sets, a hardware encoder kept once it opens and, when it
doesn't, tried again at another size or a minute on, playlists (made once
for a file), no more segments than the most for a file claiming to last
for ever, a segment handed over by the muxer without being copied, the
muxer's output going back to put things right but not into what it handed
over, subtitle text, and picture subtitles' timing, colors (PQ and HLG at
graphics white), scaling and blending into NV12 and P010. Each has been
checked to fail when what it covers is broken.

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
and one that takes only FairPlay refused; a receiver that goes away or
refuses feedback ending the session; and a session dropped without being
stopped, or failing once connected (a speaker busy with another sender
refusing RECORD), letting go of the receiver: its connections closed and
its clock no longer answering. Unit tests check SRP against RFC 5054's
vectors, both sides of pairing, the encrypted frames, AES-128 CBC against
NIST's vectors, RSA-OAEP both ways, base64, TLV8, records and their
features, RTP packets and playback info. The key wrapped with the
AirPort's public key has been checked to unwrap with its private one. Each
has been checked to fail when what it covers is broken.

The Chromecast crate's tests cast to the fake device over TLS: connecting
launches the Default Media Receiver and connects to it, the heartbeat goes
both ways and keeps the cast past the silence that would end it; LOAD
carries the URL, where to start, the subtitle track and its style; play,
pause, seek, subtitles, the device's volume and the stream's reach it,
the one leaving the other; its status and volume come as events, the end
of what plays too; a refused LOAD says so; loading again isn't taken for
another sender; stopping closes the app without telling of it; a speaker
takes a cast; and the cast ends when the device goes away or stops
answering, when the app closes or another launches, and when another
sender plays something else. Unit tests check the protobuf framing, the
JSON LOAD sends and the status read back, records from a Google TV, a
Nest speaker and a group, and that what a device sends after closing TLS
is dropped rather than read for ever, and that a device which takes
nothing more of what's sent is given up on. Each has been checked to fail
when what it covers is broken.

The DLNA crate's tests use the fake renderer over HTTP and SSDP on this
computer: it's found by searching and by its advert, and dropped on byebye
and when its max-age runs out; its capabilities come from its protocol
info; SetAVTransportURI carries the URL and the DIDL-Lite with subtitles
both ways; play, pause, seek (by absolute time for a speaker that takes
only that) and volume reach it; refusals come back as typed faults;
loading while it plays stops it first when it must; events tell what a TV
does and asking tells what a speaker does, and a TV whose events never
come, its position taken as heard when it first came, halfway between the
asking and a slow answer, and asked again at once and then as often as
told; subscriptions are renewed, made again when the renderer forgot them,
and ended, one being renewed as the session ends too; and the session ends
when the renderer goes away or plays something else, but not for what
played before or a track that lags. Unit tests read descriptions and
protocol info in the forms Samsung, LG and Sonos give them and as a
Devialet speaker gave them (ids changed), SSDP's messages, DIDL-Lite,
times, URLs and HTTP, check the events listener stops with nothing
connecting to wake it and a renderer that moves read again when its first
read there fails, and refuse what a hostile device could send: XML nested
too deep, an advert or a subscription lasting for ever, a chunk too long
to count, a URL with a character across where `http://` would end, URLs
with spaces or line breaks, a description whose services or icons are on
another host or whose service type isn't one, and an advert for a
description somewhere other than where it came from; an answer sent a byte
at a time is cut off when a whole one would be; and no more than 256
renderers are kept, with a description that can't wait to be read asked
for again when its renderer is next heard. Each has been checked to fail
when what it covers is broken.

Each protocol crate has an example that talks to real devices on the
network; see [ARCHITECTURE.md](ARCHITECTURE.md#the-chromecast-crate) for
what they do.

```bash
cargo run -p chromecast --example chromecast -- list
cargo run -p dlna --example dlna -- list
cargo run -p airplay --example airplay -- list
```

### Fixtures

The window, media engine, casting and stream server tests open the small
files in `tests/fixtures`, committed so they run without the `ffmpeg`
tool. `script/make-fixtures` makes them again (writing the DVD subtitles'
pictures itself, as FFmpeg makes picture subtitles only from pictures, and
the colors FFmpeg has no names for into `clip-odd-colors.mkv`, as it won't
write them). It takes `ffmpeg` with `libx264`; run it only to change them.
The script lists what each file is.

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
names), and the script exits non-zero if any failed:

- `first-open` opens a video into the empty window, which must fit it;
- `several` opens two videos and a sound into one window;
- `close` closes the tiles and then the window with ⌘W, and the app must
  quit;
- `subtitles` turns the subtitles beside a video off and on again;
- `step-back` steps a frame forward and two back through B-frames;
- `float-on-top` floats the window while it plays, as the system has it
  (the window's level on macOS, `_NET_WM_STATE_ABOVE` on X11), brings it
  down when paused, and floats Settings with it.

Throughout, a window that has fitted its files must be laid out at the
size it really is, every playing video must show at least 15 of its 25
frames a second, the sound must go to a device, and nothing may log an
error, whatever `RUST_LOG` lets through. The clips loop and play at no
volume. A window must stay uncovered while it runs, since one that isn't
drawing fails. Resizing the window on macOS from inside its own update
again, which leaves gpui laying it out at its old size, makes `first-open`
fail.

On Linux with no display, the script starts Xvfb with openbox, as gpui
only draws under a window manager, and PulseAudio, whose null sink plays
in real time; Vulkan can be software (lavapipe). It uses `Xvfb`,
`openbox`, `pulseaudio`, `pactl`, `xdpyinfo` and `xprop`.

## Conventions

- Cargo commands at the top of the repository act on Plays alone;
  `--workspace` takes in the crates in `crates/`.
- The crates in `crates/` know nothing of Plays or gpui (the protocol
  crates nothing of FFmpeg either), and are built and tested on their own.
- Everything Plays changes in `vendor/gpui` is marked "Plays" in the code
  and described in `vendor/gpui/PLAYS.md`, which also says how to update
  gpui; `vendor/xattr/PLAYS.md` does the same for xattr.
- When the dependencies change, run `script/attributions` so Settings →
  About lists them.
- The test fixtures are committed; change them with
  `script/make-fixtures`.
- Tests never read or write the real settings file, and debug selectors
  exist only in test builds.
- Each test has been checked to fail when what it covers is broken, and
  expected values for color math are worked out apart from Plays' code.
