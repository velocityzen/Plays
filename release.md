# Plays — what's new since 0.1.0

## ✨ New features

### Playback
- **Playback speed**: play anything from 0.25× to 4× without changing its pitch. Use `]` and `[` to speed up and slow down, and Backspace to go back to normal. Also in Playback → Speed and the command palette.
- **Skip Intro, Skip Credits and Next Episode**: when a file has chapters named as an intro or credits, a button appears low on the right while they play. Past the end credits, it plays the next file in the playlist or folder.
- **Playlists**: `.m3u`, `.m3u8` and `.pls` files, on disk or on the web, play what they list one after another. HLS playlists play as the stream they are.
- **Files remember how you left them**: a file opened again comes back turned, mirrored, cropped, at the aspect ratio and with the audio and subtitle delays you gave it. This is kept for 30 days along with its place and tracks.

### Live streams
- **Pausing a live stream** and playing it again picks up from what's live now.
- **Seeking back in live HLS**, as far as the server keeps. Seek past the live edge, press End or click the LIVE badge to go live again. The controls say how far behind live you are.
- **Screenshots of live streams** save the picture on screen, at the size it plays.

### Subtitles
- **Styled ASS and SSA subtitles**: fonts, sizes, colors, outlines, shadows, boxes, positions and karaoke show as the file styles them. They also show in screenshots and in Picture in Picture. To turn this off, use Settings → Subtitles → File's own styles.

### HDR and Dolby Vision
- **HDR is brighter than SDR white on macOS**, even when decoded in software.
- **HDR is tone mapped to the video's own peak** (from MaxCLL, the mastering display, or HLG's 1,000 nits), so highlights keep their detail without dimming the rest.
- **Dolby Vision profile 7**: the enhancement layer is combined with the base layer as the file plays, seeks and is screenshot.
- **Dolby Vision profile 20** plays, decoded on the GPU.
- **Picture in Picture** shows a video as its block does: cropped, at its aspect ratio, turned and mirrored (on macOS 12 too), in HDR and in Dolby Vision.

### Performance
- **8K AV1 plays smoothly**: it's decoded on the GPU where the Mac can (M3 and later), and in software it uses more threads. On test hardware, 8K60 has nearly twice the headroom it had.

### Casting
- **AirPlay TVs of other makes** (LG, Samsung, some Google TVs, projectors) get the picture, not just the sound. They're sent H.264 up to 1080p with stereo AAC, and HDR is fitted to SDR for them.
- **DLNA TVs that don't list their codecs** but have AirPlay built in are asked what their screen shows. Those that show HDR get 10-bit HEVC in HDR, up to the screen's largest picture. Dolby Vision goes as it is when the screen plays that profile.
- **DLNA renderers are found over IPv6** as well as IPv4.
- **HDR to devices without HDR** is fitted to SDR as Plays shows it, instead of looking washed out.
- **Dolby Vision profiles 5, 10.0 and 20** cast to any device. They're turned into HDR10 for devices that show it and fitted to SDR for the rest.
- **Subtitles in casts**:
  - Text subtitles keep their position (top, middle or bottom; left, centre or right).
  - Picture subtitles (such as PGS and DVB) now reach the device too, drawn into the video.
- **Your adjustments reach the cast**: audio and subtitle delays, gamma, crop, rotation and mirroring.

### Command line and Lua scripts
- **`plays info`** prints what's in a file or stream without starting Plays:
  - The format, duration, chapters and every stream's codec.
  - Each video's size, colors, HDR, Dolby Vision profile and mastered peak.
  - The subtitle files that play beside it.
  - Add `--json` for JSON.
- **`plays actions`** and **`plays api list`** list every command by id, as a table or as JSON.
- **Explicit play and pause**: `block.playback.play` and `block.playback.pause` play or pause rather than toggle. The window and app versions do the same for every block.
- **Window and app versions of every command**: `window.*` and `app.*` now have every playback, audio, subtitle and video command that `block.*` has.
- **`block.set_ui` and `window.set_ui`** choose which panels show over the video.
- **Inspecting a block**: `block.inspect` and `block.ls` say whether a file opened, why it couldn't, and whether it's buffering.
- **Opening files from a script**: `app.open` takes `autoplay`, `several_files` and `kind`, whatever Settings say.
- **Listing**: `plays.ls` lists a folder and `app.formats` lists the formats Plays opens.
- **Waiting and stopping**:
  - `plays.wait` waits for whichever of several events comes first.
  - `plays.on_stop` runs cleanup however a script ends. Ctrl-C stops such a script with exit status 130.
- **Example scripts** are in `lua/`, starting with `slideshow.lua`.

### Other
- About links to Plays' repository and shows its copyright.
- Bluetooth outputs say Bluetooth in Play Audio On, as AirPlay and DLNA ones do.

## ⚠️ Changes for scripts
- **Commands and events now start with what they act on**: for example `block.playback.play`, `window.playback.pause`, `app.playback.pause`, `block.audio.mute.on` and `block.playback.ended`.
- **Every on/off feature** has `<feature>.toggle`, `.on` and `.off`, and sends a `.on` or `.off` event whenever it changes, whatever changed it.
- **`docs/commands.md`** lists every command's arguments, what it returns, the events it sends, and what causes each event.

## 🔧 Improvements
- **A video's pixel shape** set by its container (Matroska's display size, MP4's `pasp`) now comes before the pictures' own, as it plays, in screenshots and when cast.
- **WebVTT cues** follow their `line` setting: top, middle or bottom.
- **Dolby Vision casts** use the HDR gamma setting, as Plays shows them.

## 🐛 Fixes
- Dolby Vision profile 20 showed green and magenta.
- Dolby Vision profiles 5 and 20 cast to an Apple TV 4K showed green and purple.
- HDR cast over HLS to an Apple TV showed a black screen.
- Some AirPlay TVs were listed as speakers.
- AirPlay TVs of other makes showed only sound over a black screen, or stuck at Loading.
- A phone's video went sideways when cast, and anamorphic video was squashed when re-encoded.
- Picture subtitles in an HDR cast were over-saturated.
- The stream server no longer:
  - plays a Dolby Vision enhancement layer on its own;
  - mistakes some text subtitles for pictures;
  - takes "zxx" as a language;
  - leaves HDR tagged with an unusual matrix unfitted.

## 🔒 Security
- **Untrusted files**:
  - Video frames stored upside down are read safely, not past their memory.
  - A file claiming an extreme pixel shape can no longer make Plays allocate huge frames.
  - Subtitle text that isn't valid UTF-8 is cleaned up instead of trusted.
  - A local file can only reach other local files, never the network.
  - FFmpeg is built with only the protocols Plays uses.
- **The local network**:
  - Forged network announcements can't make Plays fetch from an address over and over.
  - Another device can't remove a renderer from the cast menu, and look-alike renderers show their address.
- **This computer**:
  - The updater can't be tricked into overwriting a file through a link placed beside the app.
  - A Lua script's `require` no longer loads modules from the folder you run it in.
  - Command-line control only answers your own user's programs, and its socket is private from the moment it's created.
  - The stream server checks that a stream exists before reading it.
