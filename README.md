# Plays

A video and audio player for macOS and Linux that plays many files at once,
side by side, each in a block of its own. Built with
[gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's
GPU-accelerated UI framework.

- [Features](#features)
- [Getting Plays](#getting-plays)
- [Shortcuts](#shortcuts)
- [Good to know](#good-to-know)
- [Command line](#command-line)
- [Limitations](#limitations)
- [Working on Plays](#working-on-plays)

## Features

### Playback

- Any number of files and folders, side by side in a grid, or one after
  another in one block as a playlist
- Streams: HTTP(S), HLS, DASH, RTMP, RTSP
- Play/pause, seek, loop, volume and mute in each block
- Frame-accurate scrubbing and frame stepping
- Hardware decoding: VideoToolbox on macOS; VA-API, NVDEC or VDPAU on Linux
- Chapters
- Previous and next file in the folder or playlist
- Open Recent
- Files resume where they were left, with their tracks
- Music Mode for audio files: cover art, title, artist, album and quality,
  over a speaker moving with the sound; the speaker and quality can be off
- The screen stays awake while video plays
- Play, pause, seek, step, sync, volume and mute every block in every window
  at once

### Picture

- HDR10, HLG and Dolby Vision (profile 5 too): brighter than SDR white on
  macOS (EDR), tone mapped elsewhere
- Rotate, flip, aspect ratio and crop
- Gamma for SDR and for HDR
- Fit or fill

### Tracks and subtitles

- Switch audio, subtitle and video tracks
- Text subtitles (SRT, ASS, WebVTT, MP4 timed text), styled in Settings;
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
- Controls that fade while playing; each panel over the video can be hidden
- Frosted, glass, glow, tint or plain overlays
- Picture in Picture (macOS 12 and later)
- A screensaver in an empty window

### Displays and sound

- Full screen on any display, the others blacked out if you like
- macOS full screen in a Space of its own, or just over the display
- An output device for the app, and for each window
- A window sent to a TV plays through the TV's speakers
- AirPlay speakers as outputs, on macOS and Linux
- Bluetooth headphones connected when picked (macOS)

### Casting

- Chromecast, Google TV and Nest; DLNA; Apple TV and AirPlay TVs
- Files go as they are, repackaged or re-encoded, as the device needs
- Subtitles go along
- A video's sound to a speaker while its picture plays here

### And

- Screenshots at full resolution, PNG or JPEG, HDR as 16-bit PNG
- A console: format, streams, decoder, dropped frames, A/V offset, buffers
- Command palette (`⇧⌘P`) and Go to (`⌘P`)
- Every shortcut can be changed
- The `plays` command line
- Settings in a JSON file you can export, import and reset
- A menu bar on macOS, a toolbar on Linux

## Getting Plays

Plays is built from source: see [CONTRIBUTING.md](CONTRIBUTING.md#building).

### macOS

`script/bundle-macos` makes `target/release/Plays.app`, with FFmpeg built
in. Finder then offers it under **Open With**, and its Dock icon takes
dropped files.

### Linux

- A working Vulkan driver, OpenSSL, libva and libvdpau
- For hardware decoding, your GPU's VA-API driver (`intel-media-va-driver`
  or `mesa-va-drivers`), or NVIDIA's for NVDEC
- Sound goes through ALSA (PipeWire and PulseAudio provide it)
- For **Open With**, put `plays` on your `PATH` and copy
  `resources/linux/plays.desktop` to `~/.local/share/applications/`

```bash
plays video.mp4 song.mp3 https://example.com/live.m3u8
```

One Plays runs at a time: `plays FILE…` hands files to the one running.

## Shortcuts

| Action | Shortcut |
| --- | --- |
| Open files | `⌘O` / `Ctrl+O`, or drop them on the window |
| Open a stream URL | `⌘⇧O` / `Ctrl+Shift+O` |
| Play/pause | `Space` or `P` |
| Seek back/forward | `←` `→` |
| Previous/next frame | `⌥←` `⌥→` / `Alt+←` `Alt+→`, or `,` `.` |
| Restart | `Home` |
| Previous/next chapter | `Page Up` `Page Down` |
| Previous/next file | `⌘←` `⌘→` / `Ctrl+←` `Ctrl+→` |
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
| Screenshot | `S` |
| Full screen | `F`, or `⌃⌘F` / `F11` |
| Window to its videos' size | `⌥⌘0` / `Ctrl+Alt+0` |
| Picture in Picture (macOS) | `⌃⌘P` |
| Console | `~` or `` ` `` |
| Play/pause, seek, restart, mute all | `⇧Space`, `⇧←` `⇧→`, `⇧Home`, `⇧M` |
| Volume down/up on all | `⇧↓` `⇧↑` |
| Previous/next frame on all | `⌥⇧←` `⌥⇧→` / `Alt+Shift+←` `Alt+Shift+→` |
| Sync all to this block | `⇧S` |
| Focus the block left, right, above, below | `⌥⌘` and an arrow / `Ctrl+Alt+` arrows |
| Focus the previous/next block | `⌘[` `⌘]` / `Ctrl+[` `Ctrl+]` |
| Move the block left, right, up, down | `⌥⇧⌘` and an arrow / `Ctrl+Alt+Shift+` arrows |
| Maximize the block, or show them all | `⇧⌘↩` / `Ctrl+Shift+Enter` |
| Copy the file's path | `⌥⌘C` / `Ctrl+Alt+C` |
| Close the file | `⌘W` / `Ctrl+W` |
| Close all files | `⌘⇧W` / `Ctrl+Shift+W` |
| Close the window | `⌥⌘W` / `Ctrl+Alt+W` |
| New window | `⌘N` / `Ctrl+N` |
| Player window 1 to 9 | `⌘1` … `⌘9` / `Ctrl+1` … `Ctrl+9` |
| Next/previous window | `⌃⇥` `⌃⇧⇥` / `Ctrl+Tab` `Ctrl+Shift+Tab` |
| The window in front before | `⌘'` / `Ctrl+'` |
| Command palette | `⇧⌘P` / `Ctrl+Shift+P` |
| Go to a window, block or file | `⌘P` / `Ctrl+P` |
| Settings | `⌘,` / `Ctrl+,` |

Shortcuts go to the block under the mouse, or the one the keys went to.
With `⇧`, they go to every block in every window. Change them in Settings
→ Hotkeys, or with Change Shortcut (`⌘↩`) in the command palette.

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
  To have an Apple TV play the file itself, cast to it.
- **Auracast**: macOS can't broadcast it. Use a USB Auracast transmitter,
  which is a sound output like any other. On Linux, an LE Audio adapter
  can broadcast with a recent BlueZ, PipeWire and WirePlumber set up for it
  (the `bap_bcast_source` role).

## Command line

Off until Settings → Command Line turns it on. On macOS, Plays → Install
Command Line Tool puts `plays` in `/usr/local/bin`.

```bash
plays ls                                  # windows, each with its blocks
plays open ~/Movies/film.mkv              # in the window in front
plays -w Kitchen block ls                 # the window labeled Kitchen
plays do playback.seek time=90            # any command, by its id
plays -b 2 do audio.set_volume volume=40  # on the second block
plays wait ended -b 1 --timeout 600       # until the first block ends
plays events --json                       # every change, a JSON line each
plays settings set autoplay false
plays actions                             # every command
plays actions flip                        # those with flip in id or title
```

- `-w` names a window and `-b` a block, by id, label or number. Without
  them, it's the window in front and the block keys go to.
- `--json` prints JSON.
- [docs/commands.md](docs/commands.md) has every command and event.
- Exit codes: 0 done, 1 failed, 2 bad command line, 3 nothing by that
  name, 4 no Plays to ask, 5 command line control off.

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

- On Linux, frames decoded on the GPU are copied back to memory to be
  drawn. Lower the decode limit in Settings for many large videos.
- HDR goes brighter than SDR white only on macOS, decoded by
  VideoToolbox. HDR metadata isn't used, and Dolby Vision profile 7's
  enhancement layer is ignored.
- No playback speed. Playlist files (`.m3u`, `.pls`) aren't read.
- ASS styling is used only for italics, bold and placement.
- Picture adjustments and delays aren't kept when a file opens again.
- A paused live stream goes on from where it paused, not the live edge,
  and can't be screenshotted.
- Picture in Picture ignores crop, aspect ratio and gamma.
- On Wayland, windows can't float on top or be placed, and on GNOME they
  have no resize handles.
- On Linux, ALSA names a TV's HDMI output after the sound card, so a window
  sent to a TV keeps its sound.
- Casting re-encodes video only with a hardware encoder. While a block
  casts, delays, picture adjustments and frame steps don't apply.
- AirPlay speakers with a password, AirPlay 1 speakers with FairPlay or
  MFi, and HomePods that let only their home play to them can't be played
  through. Several speakers aren't played through as a group.
- AirPlay, Chromecast and Bluetooth headphones have been tested with
  pretend devices only, not real ones.
- A Chromecast's certificate isn't checked against Google's.
- DLNA devices are found over IPv4 only, and their subtitles depend on the
  TV.
- The cast stream server listens on every interface, so the firewall may
  ask about it, and on macOS 15 and later looking for devices asks to use
  the local network.

## Working on Plays

- [ARCHITECTURE.md](ARCHITECTURE.md): how Plays works, with diagrams of
  its parts, the media pipeline and casting.
- [CONTRIBUTING.md](CONTRIBUTING.md): building it on macOS and Linux, the
  macOS app bundle, licenses, tests and smoke runs.
