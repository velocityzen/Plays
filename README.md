# Plays

A video and audio player for macOS and Linux that plays many files at once,
side by side, each in a block of its own. Built with
[gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's
GPU-accelerated UI framework.

- [Features](#features)
- [Getting Plays](#getting-plays)
- [Good to know](#good-to-know)
- [Command line](#command-line)
- [Limitations](#limitations)
- [Working on Plays](#working-on-plays)

## Features

### Playback

- Any number of files and folders, side by side in a grid, or one after
  another in one block as a playlist
- Playlist files (`.m3u`, `.m3u8`, `.pls`), on disk or over HTTP, each in
  one block
- Streams: HTTP(S), HLS, DASH, RTMP, RTSP. A live one paused at its live
  edge plays on from where it is now; a live HLS one seeks back as far as
  its server keeps, and End goes live again
- Play/pause, seek, loop, volume and mute in each block
- Speed from 0.25× to 4×, the sound at its own pitch
- Frame-accurate scrubbing and frame stepping
- Hardware decoding: VideoToolbox on macOS, Dolby Vision profile 20's
  MV-HEVC too; VA-API, NVDEC or VDPAU on Linux
- Chapters, and Skip Intro, Skip Credits and Next Episode where they
  mark them
- Previous and next file in the folder or playlist
- Open Recent
- Files resume where they were left, with their tracks, picture
  adjustments and delays
- Music Mode for audio files: cover art, title, artist, album and quality,
  over a speaker moving with the sound; the speaker and quality can be off
- The screen stays awake while video plays
- Play, pause, seek, step, mute, and change the volume, speed, tracks,
  subtitles and picture of every block in a window, or in every window, at once

### Picture

- HDR10, HLG and Dolby Vision (profiles 5 and 20, and profile 7's
  enhancement layer, too): brighter than SDR white on macOS (EDR), tone
  mapped elsewhere
- Rotate, flip, aspect ratio and crop
- Gamma for SDR and for HDR
- Fit or fill

### Tracks and subtitles

- Switch audio, subtitle and video tracks
- Text subtitles (SRT, ASS, WebVTT, MP4 timed text), styled in Settings;
  ASS and SSA in their own fonts, colors, outlines, places and karaoke;
  bitmap subtitles (PGS, VobSub, DVB)
- Subtitle files beside the video, in legacy encodings too
- Preferred audio and subtitle languages
- Audio and subtitle delay

### Windows and blocks

- Pop a block out to a window of its own
- Drag blocks to reorder them
- Go between blocks and windows from the keyboard
- Name blocks and windows
- Windows sized to their videos, reopening where they closed
- Float on top, always or while playing
- Controls that fade while playing; each panel over the video can be hidden,
  or all at once, in a block, a window or every window
- Frosted, glass, glow, tint or plain overlays
- Picture in Picture (macOS 12 and later)
- A screensaver in an empty window

### Displays and sound

- Full screen on any display, the others blacked out if you like
- macOS full screen in a Space of its own, or just over the display
- An output device for the app, and for each block
- A window sent to a TV plays through the TV's speakers
- AirPlay speakers as outputs, on macOS and Linux
- Bluetooth headphones connected when picked (macOS)

### Casting

- Chromecast, Google TV and Nest; DLNA; Apple TV, and TVs with AirPlay
  built in, paired with the PIN they show
- Files go as they are, repackaged or re-encoded, as the device needs
- HDR10, HLG and Dolby Vision to devices that show them; Dolby Vision as
  it is in a file sent as it is, of a profile the device shows, else as
  HDR10 or HLG
- HDR fitted to SDR, with Plays' own tone curve, for devices without it
- DLNA TVs that name no codecs, as a Google TV's player, get the HDR their
  screen shows, up to its largest picture, as AirPlay at their address
  lists
- Subtitles go along
- Delays, gamma and picture adjustments go along
- Phone video upright and anamorphic video at its shape
- A video's sound to a speaker while its picture plays here

### And

- Screenshots at full resolution, PNG or JPEG, HDR as 16-bit PNG
- A console: format, streams, decoder, dropped frames, A/V offset, buffers
- Command palette (`⇧⌘P`) and Go to (`⌘P`)
- Every shortcut is shown in the command palette, and can be changed there
  or in Settings → Hotkeys
- The `plays` command line
- [Lua scripts](docs/lua.md) that run commands and react to events, with
  `plays do`
- Settings in a JSON file you can export, import and reset
- Plays as the default player, for the formats you pick in Settings
- Updates itself from a signed feed, downloading only what changed where it can
- A menu bar on macOS, a toolbar on Linux

## Getting Plays

Plays is built from source: see [CONTRIBUTING.md](CONTRIBUTING.md#building).

### macOS

`script/bundle-macos` makes `target/release/Plays.app`, with FFmpeg built
in. Finder then offers it under **Open With**, and its Dock icon takes
dropped files. Settings → Default Player makes it the default for the
formats picked; macOS asks you to confirm each.

### Linux

`script/install-linux` builds Plays, with FFmpeg built in, and installs it
for you: `plays` in `~/.local/bin`, with its desktop file and icons, for
launchers and **Open With**, and for Settings → Default Player to make
it the default (`xdg-mime`). `--uninstall` takes it out again.

- A working Vulkan driver, OpenSSL, libva and libvdpau
- For hardware decoding, your GPU's VA-API driver (`intel-media-va-driver`
  or `mesa-va-drivers`), or NVIDIA's for NVDEC
- Sound goes through ALSA (PipeWire and PulseAudio provide it)

```bash
plays video.mp4 song.mp3 https://example.com/live.m3u8
```

One Plays runs at a time: `plays FILE…` hands files to the one running.

## Good to know

- **Subtitle files** beside `Movie.mkv` are its tracks: `Movie.srt`,
  `Movie.en.srt`, `Movie.English.forced.ass` and so on, also in a `Subs`
  or `Subtitles` folder, and everything in `Subs/Movie/`.
- **Cover art**: a sound file's own, else `cover.jpg` (or `.jpeg`, `.png`,
  `.tif`, `.tiff`) beside it.
- **Settings** are in `~/Library/Application Support/plays/settings.json`
  on macOS and `~/.config/plays/settings.json` on Linux.
- **An AirPlay TV or projector**: connect it from Control Center → Screen
  Mirroring as a separate display, then pick it in Video → Output Device.
  To have it play the file itself, cast to it.
- **Auracast**: macOS can't broadcast it. Use a USB Auracast transmitter,
  which is a sound output like any other. On Linux, an LE Audio adapter
  can broadcast with a recent BlueZ, PipeWire and WirePlumber set up for it
  (the `bap_bcast_source` role).

## Command line

Off until Settings → Command Line turns it on; off, Plays' socket takes
only files another Plays hands over. On macOS, Plays → Install Command Line Tool puts `plays` in
`/usr/local/bin`.

```bash
plays ls                                        # windows, each with its blocks
plays open ~/Movies/film.mkv                    # in the window in front
plays info ~/Movies/film.mkv                    # its streams, colours, HDR, chapters
plays -w Kitchen block ls                       # the window labeled Kitchen
plays do block.playback.seek time=90            # any command, by its id
plays do seek.lua time=90                       # a Lua script; -e CODE, - for stdin
plays -b 2 do block.audio.set_volume volume=40  # on the second block
plays do window.full_screen.on                  # each feature: .on, .off, .toggle
plays wait block.playback.ended -b 1            # until the first block ends
plays wait block.1.position@90s                 # until it plays to 1:30
plays events --json                             # every change, a JSON line each
plays settings set autoplay false
plays actions                                   # every command
plays actions flip                              # those with flip in id or title
```

- `-w` names a window and `-b` a block, by id, label or number. Without
  them, it's the window in front and the block keys go to.
- `--json` prints JSON.
- `plays info` reads files and streams itself, without Plays or its socket.
- [docs/commands.md](docs/commands.md) has every command and event;
  [docs/lua.md](docs/lua.md) and [docs/lua-events.md](docs/lua-events.md),
  Lua scripts and their handlers; [lua/](lua), example scripts, as
  [slideshow.lua](lua/slideshow.lua).
- Exit codes: 0 done, 1 failed, 2 bad command line, 3 nothing by that
  name, 4 no Plays to ask, 5 command line control off, 6 no answer in
  time.

Completions for bash, zsh, fish and Nushell:

```bash
plays completions bash > ~/.local/share/bash-completion/completions/plays
plays completions zsh > ~/.zfunc/_plays  # with fpath=(~/.zfunc $fpath) before compinit
plays completions fish > ~/.config/fish/completions/plays.fish
```

```nu
mkdir ($nu.user-autoload-dirs | first); plays completions nushell | save -f ($nu.user-autoload-dirs | first | path join plays.nu)
```

## Limitations

- Live streams but HLS can't be seeked. A live stream's screenshot is the
  picture as it plays, at the size it plays at.
- HDR goes brighter than SDR white only on macOS. Of HDR metadata, only
  peaks are used (MaxCLL, the mastering display's).
- Dolby Vision profile 7's enhancement layer is composed on the CPU.
- ASS and SSA lines don't move, fade, turn, scale, blur or clip
  (`\move` stays where it starts), keep one size and font per line, and
  lines placed alike stack rather than dodge each other. Drawings aren't
  drawn.
- On Linux, frames decoded on the GPU are copied back to memory to be
  drawn. Lower Settings → Video → Resolution limit for many large videos.
- On Linux, ALSA names a TV's HDMI output after the sound card, so a window
  sent to a TV keeps its sound.
- On Wayland, windows can't float on top or be placed, full screen on
  another display is up to the compositor, and other displays aren't
  blacked out. On GNOME, windows have no resize handles.
- Plays doesn't update itself run from where it was built (`target/`),
  on a Mac from where it was downloaded, or installed by a package
  manager, Flatpak or Snap.
- Casting re-encodes video only with a hardware encoder. While a block
  casts, speed and frame steps don't apply; a device that takes only
  files gets them without delays or picture adjustments, and one that
  plays nothing this computer encodes, without picture adjustments and,
  in MPEG-TS, phone video sideways.
- Cast subtitles go where they're placed only to a third of the picture,
  and on DLNA TVs at the bottom; picture subtitles (PGS, VobSub, DVB)
  don't reach a device that takes only files or plays neither H.264 nor
  HEVC.
- AirPlay speakers with a password, AirPlay 1 speakers with FairPlay or
  MFi, and speakers and TVs that let only their home play to them can't be
  played through. Several speakers aren't played through as a group.
- TVs with AirPlay built in, other than Apple TVs, get H.264 up to 1080p
  and stereo AAC, their HDR fitted to SDR: their AirPlay player may show
  no HEVC.
- Over AirPlay, and wherever it's repackaged, Dolby Vision goes without
  its own signalling, as HDR10 or HLG.
- HDR reaches a DLNA TV only where it names no codecs and AirPlay at its
  address lists its screen's HDR.
- A Chromecast's certificate isn't checked against Google's.
- DLNA devices aren't authenticated: one on the network can take another's
  name, so devices of one protocol sharing a name show their addresses.
- DLNA devices' subtitles depend on the TV.
- The cast stream server listens on every interface, so the firewall may
  ask about it, and on macOS 15 and later looking for devices asks to use
  the local network.

### Casting hardware checklist

Tested with pretend devices; checked with real ones (✅), not yet (⬜), or
not on that system (—):

| Device | macOS | Linux |
| --- | --- | --- |
| Bluetooth headphones | ✅ | ⬜ |
| AirPlay 2 speaker | ✅ | ⬜ |
| AirPlay 1 speaker, such as an AirPort Express | ⬜ | ⬜ |
| Apple TV | ✅ | ⬜ |
| TV of another make with AirPlay | ✅ | ⬜ |
| Chromecast | ⬜ | ⬜ |
| Google TV | ⬜ | ⬜ |
| Nest speaker or display | ⬜ | ⬜ |
| DLNA speaker | ✅ | ⬜ |
| DLNA TV | ✅ | ⬜ |
| A TV's speakers, for a window sent to it | ⬜ | — |

Checked with: an Apple TV 4K, HDR10 playing, not yet seen to be in HDR;
a VisionMaster projector, a Google TV, over AirPlay, and over DLNA in
HDR10 and Dolby Vision profile 5.

## Working on Plays

- [ARCHITECTURE.md](ARCHITECTURE.md): how Plays works, with diagrams of
  its parts, the media pipeline and casting.
- [CONTRIBUTING.md](CONTRIBUTING.md): building it on macOS and Linux, the
  macOS app bundle, licenses, tests and smoke runs.
