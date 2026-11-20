# Plays

A small video and audio player for macOS and Linux that plays many files at
once, side by side. Every file gets a tile of its own, with its own
controls; a tile can move to a window of its own, play full screen on
another display, or cast to a TV or a speaker. All of the UI, including the
settings window, is built with
[gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's
GPU-accelerated UI framework.

- [Features](#features)
- [Getting Plays](#getting-plays)
- [Using it](#using-it)
- [Limitations](#limitations)
- [Working on Plays](#working-on-plays)

## Features

### Playback

- Open any number of files, or whole folders: each gets a tile in a grid
  that sizes itself.
- Open network streams with **Open URL…**: HTTP(S) files, HLS and DASH
  playlists, RTMP and RTSP. Live streams show a LIVE badge; a stream that
  stalls pauses and resumes instead of skipping ahead.
- Every file has its own play/pause, seek bar, loop, mute and volume.
- Scrub with the seek bar: the picture follows as you drag and lands on the
  exact frame where you let go. Step a frame at a time, forward or back.
- Hardware video decoding: VideoToolbox on macOS; VA-API, NVDEC or VDPAU on
  Linux. Videos the GPU can't decode fall back to software by themselves.
- Chapters: a notch on the bar where each starts, the time under the bar
  says which one you're in, and keys and a menu go from one to the next.
- The previous and next media file in the folder open in the same tile.
- A file opened again starts where it was left, for 30 days since it last
  played; one barely begun, or played nearly to its end, starts over.
  Settings → Playback turns it off, which forgets every file's place.
- Audio-only files show their cover art (or a note) and a level meter.
- While a video plays the screen doesn't sleep, and while only sound plays
  the computer doesn't. Paused or ended, they sleep as they would.

### Picture

- HDR video (HDR10/PQ and HLG, BT.2020), and Dolby Vision, including
  profile 5, which has no HDR10 picture to fall back on. On macOS,
  highlights go brighter than SDR white as far as the screen allows (EDR,
  such as on XDR displays), and are tone mapped to fit on other screens.
  Elsewhere, and for video decoded in software, HDR is tone mapped to SDR.
- Turn the picture a quarter turn either way, flip it, show it at another
  aspect ratio (1:1 to 2.35:1) or crop it to another shape (1:1 to 2.39:1),
  each tile on its own. Even a paused video changes at once.
- Videos whose files say to turn them, as phones' do, show upright.
- Gamma for SDR and for HDR video, applied to every open video as you
  drag it.

### Tracks and subtitles

- Files with several tracks get a track menu: pick the audio track, the
  subtitles (or none) and, if there are several, the video track, while it
  plays.
- Text subtitles (SRT, ASS, WebVTT, MP4 timed text) sized to the picture
  and styled as you like; bitmap ones (PGS, VobSub, DVB) as the disc has
  them.
- Subtitle files beside a video, or in a `Subs` or `Subtitles` folder, are
  tracks too, with their language read from their names. Text in legacy encodings is
  converted.
- A file's default subtitles, or failing that its forced ones, show from
  the start.
- Move a file's sound (in 50 ms steps) or subtitles (in 100 ms steps)
  earlier or later against the picture, for files that are out of step.

### Windows and tiles

- Any tile can pop out into a new window, beside the one it left, where it
  keeps playing. Every window works the same: drop more files on it and
  they play side by side.
- Drag a tile by its picture onto another to put it in that one's place.
  A window's only tile, or a maximized one, drags the whole window.
- Windows take the size of the video as files open, or of a grid with room
  for each video at its own size, scaled down to fit the screen. After
  that a window keeps the size you give it.
- Windows open where you are working: where the last one closed, on the
  same display, at the same size.
- Player windows can float above other apps' windows, always or only while
  something in them plays.
- While a tile plays, its header and controls fade when the mouse leaves
  or rests, and the pointer hides.
- The controls, notes and menus over a video show it through them:
  frosted, glass, glow, tint, or plain see-through black.
- On macOS, windows show just the video, edge to edge, with the traffic
  lights over it. Colors are macOS's own, in light and dark, with your
  accent color.
- Picture in Picture on macOS 12 and later: a tile in macOS's own floating
  window, over every app and Space, subtitles and all.

### Displays and full screen

- With more than one display, put a window full screen on the one you
  pick, picture, subtitles and all. Out of full screen, it goes back where
  it was.
- On macOS, full screen is macOS's own, in a Space of its own, or, if you
  like, just over the window's display, going in and out at once.
- Black out the other displays while a window is full screen, for
  projectors, TVs and dark rooms.
- An AirPlay TV or projector connected as a separate display is a display
  like any other, with the sound going there too.

### Sound

- The sound of every file is mixed into one output: the system's default,
  or a device you pick, remembered from run to run.
- A window can play through an output of its own instead, such as a TV's
  speakers while the desk keeps its own.
- A window sent full screen to a TV plays through the TV's speakers until
  it comes back.
- While the device picked isn't connected, the sound waits for it, or goes
  through the system's default, and goes back to it as soon as it's there.
- AirPlay speakers play the sound once macOS has them, with the picture
  held back as long as AirPlay delays the sound, so the two stay in step.

### Casting

- Play a tile on a TV or a speaker instead, from where it is, with its
  tracks: Chromecasts, Google TVs and Nest speakers, and DLNA TVs and
  speakers, found on the network by themselves.
- A file goes as it is where the device plays it, else repackaged or
  re-encoded for it.
- A video's sound can go to a speaker while its picture plays on in the
  tile, in step with it.
- The tile's bar, buttons and shortcuts drive the device, and the
  subtitles picked go along.

### Screenshots

- Save the frame a tile shows, at the video's full resolution, turned,
  reshaped and cropped as it shows, with the subtitles showing drawn in.
- PNG or JPEG. HDR video is saved as an SDR screen shows it, or kept as
  HDR in a 16-bit PNG.
- Files are named after the video and the time, go to Pictures or a
  folder you pick, and never replace another.

### Console

- A console on the tile in front: the file's format, each stream as the
  file has it, the decoder, and, as it plays, frames shown and dropped, how
  far the picture is from the clock, what is queued and how far ahead the
  file or stream is read.

### Keyboard shortcuts

- Shortcuts go to the tile the mouse is over. With Shift, the playback
  ones go to every file in the window, and Shift-S syncs them all to that
  tile.
- Change any playback shortcut in Settings → Hotkeys.
- The menu bar (a toolbar on Linux) has them too.

### Settings

- A Settings window for playback, audio, video, subtitles (with a live
  preview), casting, screenshots, hotkeys and appearance.
- Settings are saved in a file of their own, which you can export,
  import, or reset to the defaults.
- About Plays: the app's icon, version and the day it was built.

## Getting Plays

Plays is built from source; [CONTRIBUTING.md](CONTRIBUTING.md#building)
says how, on macOS and on Linux (Debian/Ubuntu).

### macOS

`script/bundle-macos` makes `target/release/Plays.app`. FFmpeg is built
into it, so it runs on any Mac with nothing else installed. Once it's in
`/Applications` (or has been opened once), Finder offers Plays under
**Open With** for video and audio files, without making it the default
player, and files and folders dropped on its Dock icon open in it.

### Linux

The `plays` binary has FFmpeg built in, but uses the system's OpenSSL for
https, and libva and libvdpau for hardware decoding.

- gpui renders with Vulkan on Linux, so a working Vulkan driver is
  required.
- For hardware decoding, install your GPU's VA-API driver
  (`intel-media-va-driver` or `mesa-va-drivers`), or use NVIDIA's driver
  for NVDEC. The tile header shows the API in use, for example
  `h264 (VA-API)`.
- For your file manager's **Open With** menu, put the `plays` binary on
  your `PATH` and copy `resources/linux/plays.desktop` to
  `~/.local/share/applications/`.

Files and stream URLs can be given on the command line:

```bash
plays path/to/video.mp4 path/to/song.mp3 https://example.com/live.m3u8
```

## Using it

### Shortcuts

| Action | Shortcut |
| --- | --- |
| Open files | `⌘O` / `Ctrl+O`, or drop files/folders on the window (or on the Dock icon of `Plays.app`) |
| Open a stream URL | `⌘⇧O` / `Ctrl+Shift+O` (pre-filled from the clipboard) |
| Play/pause | `Space` or `P` |
| Seek back/forward | `←` `→` (5 seconds at first; the step is in Settings) |
| Previous/next frame | `⌥←` `⌥→` / `Alt+←` `Alt+→`, or `,` `.`, or `<` `>` |
| Restart | `Home` |
| Previous/next chapter | `Page Up` `Page Down` |
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
over, or clicked), or the maximized one, as its controls show. Those with
`⇧` act on every tile in the window: play/pause, seek, frame step, restart,
volume (each tile's from its own) and mute. Sync all puts every tile where
the active one is, playing or paused as it is, so files side by side go on
together (the active one seeks there too); the frame keys with `⇧` then
step them all, each by its own frame rate.

Where keyframes are far apart, a skip lands on the nearest one its way, so
it shows at once.

A short note on the tile says what a shortcut changed; switching tracks
rolls a reel, like a slot machine's, to the track picked, with the ones
before and after it faded above and below.

To change shortcuts, open Settings → Hotkeys, click + by a command and
press the keys; a key another command had moves to it, and Restore brings
the defaults back. The app's own shortcuts above (⌘O, ⌘W and so on) stay
as they are. Some commands have no keys until given some there, such as
Float on Top.

### Tiles

Click the picture to make it the tile shortcuts go to; double-click it (or
the corner button) to maximize it. For the only tile in a window, both
toggle full screen instead. Drag the picture onto another tile to put the
tile there, the tiles between moving over; its title follows the pointer,
and the tile it would take the place of lights up. With no other tile
showing (the only one in its window, or maximized), dragging the picture
moves the window, except in full screen. Drag the bar to scrub. In the middle
under the bar, the previous and next buttons open the media file before or
after this one in its folder, by name, in the same tile; between them,
skip back and forward go by the seek step, around play/pause.

The track button (next to loop, on files that have subtitles or more than
one audio or video track) opens the track menu: a column each for audio,
subtitles and (when there are several) video. While there are devices to
cast to, or more than one display or audio output, the cast button before
it opens a menu of where to play.

While a tile plays, its header and controls fade when the mouse leaves it,
or rests on it for a couple of seconds; the pointer hides then too, until
the mouse moves. Resting on the header or controls keeps them.

The console (`~` or Video → Console) shows the file's format, each stream
playing as the file has it (codec and profile, size, pixel format, colors,
sample rate, channels, bit rates), the decoder and what its frames become,
and, as it plays, frames shown a second, frames dropped, how far the
picture is from the clock, what is queued and how far ahead the file or
stream is read.

### Windows

When a window has more than one tile, the button next to maximize moves a
tile to a new window sized for it, beside the one it left, where it keeps
playing. Every window takes drops and works the same way; menus and
shortcuts act on the window in front, and files opened from Finder go to
it too. Drag the top edge of a window to move it, or the picture of its
only tile. Closing the last window quits.

As files open, their window takes the size of the video, or of a grid with
room for each video at its own size, scaled down to fit the screen. After
that the window keeps the size you give it.

Plays opens where its last window closed, on the same display, at the same
size. Without a saved place, or when that display is gone, it opens on the
display with the mouse (on macOS). Settings and About open on the display
of the window in front. On Wayland, the compositor places windows.

Player windows float above other apps' windows with Video → Float on Top
or Float on Top While Playing, or Settings → Appearance. Settings and
About float with them, so they don't open underneath. On macOS a window
comes down while it is full screen and floats again after.

On macOS, the traffic lights fade out with the controls while something
plays, when the mouse leaves the window or rests in it.

Settings → Appearance sets how the controls, notes and menus over a video
show it through them: frosted (blurred and dimmed), glass (clear, with a
polished edge that bends the light and catches it), glow (glass whose edge
glows with the colors around it, inside and out), tint (the colors around
it, none of their shapes), or plain see-through black.

### Picture

On macOS, the Video menu lists every aspect ratio and crop to pick from,
checking the active tile's; `A` and `C` go through them in turn, then back
to none. Rotating and flipping go by the picture as it shows, and cropping
keeps its middle. Opening another file in the tile starts it as its file
has it.

Settings → Video has the gamma for SDR and for HDR video, the decode
resolution limit, hardware decoding, and fit or fill scaling.

### Tracks, chapters and subtitles

Subtitle files beside a video are tracks too, and show from the start:
`Movie.srt`, `Movie.en.srt`, `Movie.English.forced.ass` and so on for
`Movie.mkv` (SRT, ASS, WebVTT, PGS `.sup`, VobSub `.idx`, MicroDVD, SAMI),
the same in a `Subs` or `Subtitles` folder, and everything in
`Subs/Movie/`. Their names give their language and whether they are
forced or for the hearing impaired. Text in legacy encodings
(Windows-1252, Windows-1251, Shift JIS and the like) is detected and
converted.

Files with chapters (Matroska, MP4 and the like) go from one to the next
with `Page Down` and back with `Page Up`, or straight to one from
Playback → Chapters, which checks the one the tile is in. Going back
within the first couple of seconds of a chapter goes to the one before, as
on a disc player; later, to its start. The bar has a notch where each
starts (Settings → Playback turns them off), and the time under it says
which one it's in.

The sound and the subtitles can move earlier or later against the
picture: the sound in 50 ms steps, up to 5 seconds either way; subtitles
(embedded or in a file beside the video) in 100 ms steps, up to a minute.
The sound plays on as it is and the picture moves against it. A note shows
the delay as it changes ("Audio delay +200 ms"). Opening another file in
the tile starts it with none.

Settings → Subtitles sets how text subtitles look: font, weight, size,
color, outline or shadow and its color, a background box, on the picture
or anywhere in the tile, and height, with a live preview.

### Sound

Audio from every file is mixed into one output. Settings → Audio picks it:
the system's default (which the sound follows as it changes, on macOS) or
one device, remembered from run to run. On macOS, Audio → Output Device
picks it too; it lists outputs as they are plugged in and checks the one
picked. While the device picked isn't connected, players keep playing,
silently, until it is (or, with Wait for it off, through the system's
default), and the sound goes back to it as soon as it's there.

A window can play through an output of its own instead, such as a TV's
speakers while the desk keeps its own: pick it under Window Sound in a
tile's Play On menu, which shows with more than one output, or on macOS
from Audio → Window Output Device ("Same as the App" goes back). Its tiles,
and files opened in it, play there, picking up where they are; the other
windows keep the app's output. Windows that pick the same device share it,
and a tile moved to a new window takes the device along. While it isn't
connected, its sound waits for it or goes through the system's default, as
Settings → Audio says for the app's. It isn't remembered from run to run.

A window that Play On (or Video → Output Device) puts full screen on
another display plays through that display's speakers, such as a TV's over
HDMI, until it comes back, and then through what it played through before.
The display's speakers are the output named after it, as macOS names an
HDMI or DisplayPort output (`LG TV` for the display `LG TV`, or one output
with the display's name in it, such as `Studio Display Speakers`). A
display with none, and one the window is on already, leave the sound as
it is. So does an output picked for the window; picking one, or "Same as
the App", while it is there keeps that when it comes back. Settings →
Audio → Play on the display's speakers turns this off.

On Linux, audio goes through ALSA (PipeWire and PulseAudio provide an ALSA
device), so Settings → Audio lists ALSA's outputs: `default` and
`pipewire` or `pulse` go through the sound server, which picks the
speakers.

### Displays, full screen and AirPlay

With more than one display, the cast button on a tile lists them by name
under Play On, as Video → Output Device does on macOS, and puts the player
window full screen on the one picked, picture, subtitles and all. Video →
Output Device checks the display the window is on; picking that one just
puts it full screen. Out of full screen, the window goes back where it
was. On X11 each monitor is a display, named for the model it says it is.
On Wayland the compositor is asked for the display picked but may keep the
window where it is, and places it itself once out of full screen.

On macOS, full screen is macOS's own, in a Space of its own, unless
Settings → Appearance → Full screen says Over the display. Then the window
covers just its own display and goes in and out at once. The other
displays stay as they are, and the menu bar and Dock keep out of the way
while the window is in front.

Settings → Appearance → Black out other displays covers every other
display in black while a window is full screen. Moving the mouse onto a
covered display, or clicking it, shows it again until the mouse rests
there. Leaving full screen, or switching to another app, takes the black
away. On X11 each monitor counts as a display; Wayland doesn't let Plays
place the covers.

To play on an AirPlay TV or projector, connect it first from Control
Center → Screen Mirroring and use it as a separate display: it then shows
in Video → Output Device (by the name macOS gives it), and macOS sends the
sound there with it. For sound alone, pick AirPlay speakers in Control
Center → Sound: they become the system's default, which Plays follows, and
show in Audio → Output Device and Settings → Audio like any other output.
AirPlay sound comes out late, by up to a couple of seconds; the picture
waits for it.

### Picture in Picture

On macOS 12 and later, a tile can play in macOS's own Picture in Picture
window, over every app and Space, with its play/pause and skip buttons:
`⌃⌘P`, Video → Picture in Picture, or the button on the tile. The sound
keeps coming through Plays, and the subtitles showing are drawn into the
picture. Closing the window, or its button to go back, brings the video
back to the tile.

### Casting

The cast button on a tile lists the devices found, after This Computer,
each with its kind and the one playing checked. On macOS, Video → Output
Device also lists those that show video under the displays, and Audio →
Output Device the speakers under this computer's outputs, checking the one
the tile in front casts to. Devices on the same network are found by
themselves, in a second or two. Settings → Casting turns each protocol on
or off and lists the devices found, with what each shows: video, or sound
only. AirPlay casting isn't wired in yet; see
[Displays, full screen and AirPlay](#displays-full-screen-and-airplay)
for AirPlay displays and speakers.

While a tile casts, it says where it plays and how the file gets there (as
it is, repackaged, or re-encoded), and its bar, buttons and shortcuts drive
the device, whose position moves the bar. Speakers are offered for
anything with sound; a video's sound goes to one while its picture plays
on in the tile, in step with what the speaker plays, and the tile's
buttons and shortcuts drive both.

The subtitles picked go along and change there as they change on the
tile. Picture subtitles (PGS, DVD) in the file go drawn into the picture
where it is re-encoded, and stay here where it isn't. Another audio or
video track, or subtitles drawn in changing, loads the file there again
from where it is.

Playing it here again (This Computer, Play Here, or the display in the
menu), closing the tile, or the device going away ends the cast, and the
tile goes on from where the device was. Quitting stops the devices cast
to.

A Chromecast, Google TV or TV with Cast built in is offered for anything,
and a Nest speaker or speaker group for anything with sound, a video's
sound alone going to it as HLS while the picture plays on here, following
it. A file goes to it as it is where its model plays that kind of file
(MP4, WebM, MP3, AAC, FLAC, Ogg, WAV; just MP4 on a TV of another make),
else as HLS, re-encoded where it needs to be: H.264 up to 1080p on the
first Chromecasts and other makes' TVs, HEVC, VP9 and HDR up to 4K on an
Ultra or a Google TV, AV1 too on a Google TV Streamer. Subtitles show
there as Settings → Subtitles has them look (size, color, outline or
shadow and its color, the box behind them); turning them off and on, or
changing how they look, changes them there as it plays, and picking other
subtitles gives it the file again from where it is. The tile's volume is
that of what plays there, so the device's own stays where it is.

A DLNA TV or speaker is offered by what it lists: one that lists only
sound, for anything with sound. A file goes to it as it is where it lists
that kind of file, else as a stream of MPEG-TS, re-encoded where it needs
to be; a video's sound goes to a speaker as a stream of AAC or FLAC, as
internet radio does, while the picture plays on here, following it. A
speaker can't seek in such a stream, so it's given the stream from where
the tile is, and again from elsewhere to seek. Picking other subtitles
gives it the file again from where it is, as DLNA takes subtitles only
with the file. The tile's volume scales the TV's own: where it was when
cast, the TV stays where it was.

### Screenshots

`S` or Video → Take Screenshot saves the frame the tile shows, at the
video's full resolution (whatever the decode limit), turned, reshaped and
cropped as it shows, with the subtitles showing drawn in. Files are named
after the video and the time, `Movie 01-23-45.678.png`, and never replace
another. A note says which folder it went to.

Settings → Screenshots picks the folder (Pictures at first; Show opens
it), PNG or JPEG and the JPEG's quality, whether subtitles go in, and how
HDR video is saved: as an SDR screen shows it (tone mapped, with the HDR
gamma), or as it is, in a 16-bit PNG tagged BT.2020 PQ or HLG (its cICP
chunk), which only HDR-aware apps show right. Text subtitles are laid out
as on a tile showing just the picture, in the style Settings give them;
bitmap ones go where the stream puts them.

### Settings

The Settings window has panes for Playback, Audio, Video, Subtitles,
Casting, Screenshots, Hotkeys, Appearance and About: the theme, master
volume, the audio output, playing a window sent to a display through its
speakers, default volume, autoplay, loop, resuming files where they were
left, seek step,
chapter notches on the bar, fit/fill scaling, gamma for SDR and for HDR
video (applied to every open video as you drag), decode resolution limit,
hardware decoding, subtitle style, the protocols that look for devices to
cast to and what they found, where screenshots go and what they are like,
the keys for every shortcut, grid columns, controls visibility, how the
panels over the video look, and file names. Settings → About lists the
version, the day it was built, and the credits, each of which opens to its
license; on macOS, About Plays in the app menu shows the icon, version and
build day in a small window of its own.

Settings are saved as JSON in the user config folder:
`~/Library/Application Support/plays/settings.json` on macOS,
`~/.config/plays/settings.json` on Linux. A value in it Plays can't read
leaves only that setting at its default, and the file as it was is kept
beside it as `settings.json.bad`. Settings File, at the foot of the
sidebar, opens its folder, exports a copy, imports settings from one, or
resets every setting to its default (after asking).

## Limitations

- On Linux, frames decoded on the GPU are copied back to memory and
  uploaded again to be drawn, though converting them to RGB happens on the
  GPU. Dolby Vision, and formats other than 8- and 10-bit 4:2:0, are
  converted on the CPU. Lower the decode resolution limit in Settings when
  playing many high-resolution files. On macOS, videos VideoToolbox can't
  decode, such as VP9, are converted on the CPU too.
- HDR goes brighter than SDR white only on macOS, and only for video
  decoded by VideoToolbox; software-decoded HDR is tone mapped to SDR. HDR
  metadata (mastering display, content light level, HDR10+ and Dolby
  Vision's per-scene brightness and trims) isn't used. Dolby Vision
  profiles with an HDR10 or HLG picture (7, 8) show that picture; profile
  7's enhancement layer is ignored.
- Resuming a paused live stream continues from where it paused rather than
  jumping to the live edge, and after a long pause the server may have
  closed the connection.
- Subtitles: ASS styling (fonts, colors, sizes, positions, effects) isn't
  used, only italics, bold and top/middle/bottom placement. A cue in the
  file that started before the keyframe a seek lands on is missed (one in
  a subtitle file beside it isn't, unless it started over 30 seconds
  before). Subtitle files can't be dropped on a tile. Subtitles in the
  video file itself are read about a second ahead with it, so moved
  earlier than that they show late. In a live stream, a new track starts
  with its next packets rather than at the playhead.
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
- On Linux, outputs aren't watched for coming and going while the one
  wanted is playing: the device list in Settings → Audio is read again
  when its menu opens. ALSA's own error messages about plugins with
  nothing behind them show in the terminal when it does.
- On Linux, ALSA names a TV's HDMI output after the sound card (`HDMI 0`),
  not the TV, so a window sent to a TV keeps its sound unless an output is
  named after the display, as one defined in `~/.asoundrc` can be. Whether
  macOS names them right has been checked with displays named as their
  outputs, not on a TV yet.
- On GNOME Wayland (client-side decorations) the window can be moved from
  the toolbar but has no resize handles.
- Windows can't float on top under Wayland, which has no way for an app to
  ask for it; on X11 it takes a window manager that keeps windows above
  (`_NET_WM_STATE_ABOVE`), as most do.
- Casting re-encodes video only on a hardware encoder, so a computer
  without one (Linux without NVENC or VA-API) can't cast video a device
  doesn't play as it is. Video decodes in software for it. HDR re-encoded
  for a device without HDR isn't tone mapped, and re-encoded HDR keeps no
  mastering or light level metadata. Picture subtitles (PGS, DVD) go only
  drawn into video re-encoded: where the video goes as it is they stay
  out, as do those in a file of their own, and one already showing where
  a seek starts the stream again shows only from the next. Text subtitles
  lose their placement, and a track in the media is read through the whole
  file first.
- While a tile casts, delays don't apply, as the device plays the picture
  and the sound; gamma, crop, rotation, flips, aspect ratio, and picture
  subtitles where the video isn't re-encoded, stay on this computer (the
  tile says so), and frames can't be stepped.
- Casting is to Chromecast and DLNA so far. Plays can't find or connect to
  AirPlay receivers itself: macOS keeps that to its own picker and to
  AVPlayer, which Plays doesn't play through. Connect one from Control
  Center first. Mirrored, an AirPlay display shows the whole screen; as a
  separate display, just the window. The Output Device menus are macOS
  only.
- Casting to a Chromecast hasn't been checked with a real one yet, only
  a pretend one: none was on hand. What one plays goes by the model it
  says it is, and a Google TV that calls itself just "Chromecast" is
  taken for one of the first, getting H.264 up to 1080p; a TV of another
  make gets H.264 and AAC in MP4 or HLS, which it may play more than.
  Surround sound goes as stereo AAC, and Dolby Vision is re-encoded. A
  receiver that applies the stream's volume to the device's own turns
  that instead. Subtitles take Settings' font, size, colors, edge and box,
  the weight only as bold or not, and not their placement or the area they
  go in. An HLS stream from the internet is handed over as it is, and one
  whose server doesn't let other sites have it (CORS) won't play.
- Plays takes whatever certificate a Chromecast offers, not checking it
  against Google's device certificates, so it can't tell a genuine device
  from something on the network pretending to be one.
- DLNA renderers are found over IPv4 only. Subtitles on a DLNA TV depend
  on the TV, as DLNA has none of its own: most take SRT one way or the
  other, some neither, and changing them loads the file again. A DLNA TV
  has only its own volume, which the tile's scales. One that doesn't say
  what it plays gets H.264 and AAC in MPEG-TS, which it may still not
  play, and one that won't seek while stopped starts from the beginning a
  moment first. A renderer that neither sends events nor answers when
  asked where it is can't be followed.
- The stream server listens on every interface, so the firewall may ask to
  let Plays take incoming connections, and on macOS 15 and later looking
  for devices asks to use the local network.

## Working on Plays

- [ARCHITECTURE.md](ARCHITECTURE.md): how Plays works, with diagrams of
  its parts, the media pipeline and casting.
- [CONTRIBUTING.md](CONTRIBUTING.md): building it on macOS and Linux, the
  macOS app bundle, licenses, tests and smoke runs.
