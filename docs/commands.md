# Commands

Every command Plays has, by namespace, as `plays actions --markdown` writes them. A terminal runs one with `plays do ID [KEY=VALUE…]`, naming the window or tile it acts on with `-w` and `-t`; keys run those that have them, and Settings → Hotkeys changes those. The keys here are macOS's: elsewhere Ctrl stands for ⌘, and Alt for ⌥.

## app

### Inspect Plays

`app.inspect`: Say what Plays is: its version, its player windows, and where the sound goes.

- Acts on the app.
- Hands back: {version, windows: [as window.ls], output: as output.ls}.

### Open Paths or URLs

`app.open`: Play files, the media files in folders, and streams, in the window named, or the one in front, or a new one if none is open: each in a tile, or one after another in one, as Settings say.

- Acts on a player window.
- Arguments:
  - `urls` (a list of text): Each a file's or folder's full path, a file:// URL, or a stream's URL.
- Hands back: {window}: the id of the window they open in.

### Clear Recent Files

`app.clear_recent_files`: Forget the files played lately, which File → Open Recent and Go to… offer.

- Acts on the app.

### About Plays

`app.about`: Show Plays' version, and what it's made with.

- Acts on the app.

### Settings…

`app.settings`: Open the Settings window, or bring it to the front.

- Acts on the app.
- Keys: ⌘,

### Copy Version Info

`app.copy_version_info`: Copy Plays' version and the day it was built.

- Acts on the app.
- Hands back: What it copied, such as Plays 0.1.0 (18 November 2026).

### Copy Debug Info

`app.copy_debug_info`: Copy what a bug report needs: Plays' version, the system, FFmpeg's version, where the sound goes, the windows and tiles, and the settings changed.

- Acts on the app.
- Hands back: What it copied, a line for each thing.

### Quit Plays

`app.quit`: Quit, keeping where each file got to and stopping the devices cast to.

- Acts on the app.
- Keys: ⌘Q

## window

### List Windows

`window.ls`: List the player windows, in the order ⌘1 to ⌘9 number them.

- Acts on the app.
- Hands back: [{id, label, title, number, front, tiles}]: each window's id, label (or null) and title, as its title bar shows it, its number from 1, whether it's in front, and how many tiles it has.

### Inspect Window

`window.inspect`: Say all about the window: where it is, how it shows, its output and its tiles.

- Acts on a player window.
- Hands back: {id, label, title, number, front, tiles: [as tile.ls], bounds: {x, y, width, height}, display, full_screen, on_top, output: {id, name} or null}: as window.ls says, its tiles as tile.ls lists them, where it is in points, the id of the display it's on, whether it's full screen and floats on top, and its own output.

### New Window

`window.new`: Open an empty player window, in front, numbered after the others.

- Acts on the app.
- Keys: ⌘N
- Hands back: {window}: the new window's id.

### Open…

`window.open_files`: Choose files or folders to play in the window.

- Acts on a player window.
- Keys: ⌘O

### Open URL…

`window.open_url`: Type the URL of a stream to play in the window.

- Acts on a player window.
- Keys: ⇧⌘O

### Command Palette…

`window.command_palette`: List every command that can run in the window, to find one by name and run it, picking or typing what it takes.

- Acts on a player window.
- Keys: ⇧⌘P

### Go to…

`window.go_to`: List the windows, their tiles, the files in the front tile's folder and those played lately, to find one by name and go there: bring it to the front, play the file in the tile, or open it.

- Acts on a player window.
- Keys: ⌘P

### Close Window

`window.close`: Close the window and the files in it; with none named, the window in front, Settings and About too.

- Acts on a window, Settings and About too.
- Keys: ⌥⌘W, ⌘W

### Close Other Windows

`window.close_others`: Close every player window but this one, and the files in them.

- Acts on a player window.

### Rename Window…

`window.rename`: Label the window: its title shows the label, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a player window.
- Arguments:
  - `label` (text, optional): What to call it; no two windows are called the same.

### Go to Window

`window.focus`: Bring the window to the front. ⌘1 to ⌘9 go to the first nine, by their numbers.

- Acts on a player window.

### Next Window

`window.next`: Bring the window after the one in front to the front: the first after the last.

- Acts on the app.
- Keys: ⌃⇥

### Previous Window

`window.previous`: Bring the window before the one in front to the front: the last before the first.

- Acts on the app.
- Keys: ⌃⇧⇥

### Go Back One Window

`window.back`: Bring back the player window in front before this one; again, the one before that, and so on, until something else is done.

- Acts on the app.

### Go to Recent Window

`window.recent`: Bring back the player window in front last; again, the one in front first.

- Acts on the app.
- Keys: ⌘'

### Move Window To

`window.move`: Give the window another number, from 1, which ⌘1 to ⌘9 go to it by: the windows between move up or down one.

- Acts on a player window.
- Arguments:
  - `position` (a whole number, at least 1): Its number.

### Move Window Backward

`window.move.backward`: Give the window the number before its own, the window there taking its own.

- Acts on a player window.

### Move Window Forward

`window.move.forward`: Give the window the number after its own, the window there taking its own.

- Acts on a player window.

### Full Screen

`window.toggle_full_screen`: Show the window full screen, or as it was again.

- Acts on a player window.
- Keys: ⌃⌘F, F

### Exit Full Screen

`window.exit_full_screen`: Show the window as it was before it was full screen.

- Acts on a player window.
- Keys: ⎋

### Match Video Size

`window.match_video_size`: Size the window for its videos to show at their own size, as far as the screen allows.

- Acts on a player window.
- Keys: ⌥⌘0

### Float on Top

`window.toggle_float_on_top`: Keep player windows above other apps' windows, or stop.

- Acts on the app.

### Float on Top While Playing

`window.toggle_float_on_top_while_playing`: Keep player windows above other apps' windows while a file in them plays, or stop.

- Acts on the app.

### Show on Display

`window.show_on_display`: Show the window full screen on a display, coming back where it was once it leaves full screen there.

- Acts on a player window.
- Arguments:
  - `display` (a whole number, at least 0): The display's id.

## view

### Show Controls

`view.toggle_controls`: Show the seek bar and buttons at the bottom of each tile, or never show them.

- Acts on the app.

### Show Top Bar

`view.toggle_top_bar`: Show the file's name and buttons at the top of each tile, or never show them.

- Acts on the app.

### Show Track Switcher

`view.toggle_track_switcher`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, or switch without it.

- Acts on the app.

### Show Notes

`view.toggle_notes`: Show notes on what a shortcut changed, like the volume, or not. Notes on what's wrong still show.

- Acts on the app.

## tile

### List Tiles

`tile.ls`: List the tiles in the window, in the grid's order.

- Acts on a player window.
- Hands back: [{id, label, title, file, url, playing, number, front}]: each tile's id and label (or null), its file's name, the file's path or the stream's URL (without a user, password or query; the other null), whether it plays, its place from 1, and whether keys go to it.

### Inspect Tile

`tile.inspect`: Say all about the tile: what it plays, where it is, and how it's set to play.

- Acts on a tile.
- Hands back: {as tile.ls, window, window_title, position, duration, frame_rate, ended, volume, muted, looping, tracks: {video, audio, subtitles: [{number, label, language, title, codec, format, default, forced, hearing_impaired, file}]}, picked: {video, audio, subtitles}, delays: {audio, subtitles}, picture: {turns, mirrored, aspect_ratio, crop}, chapter, chapters: [{number, name, start, end}], casting: {id, kind, name} or null}: its window's id and title, times in seconds (duration null for a live stream), frames a second (null for sound alone), volume from 0 to 100, tracks and chapters numbered from 1, the tracks picked and the chapter by number or null, delays in seconds, and quarter turns clockwise.

### Go to Tile

`tile.focus`: Put the tile in front, keys going to it, and bring its window to the front.

- Acts on a tile.

### Close File

`tile.close`: Close the tile's file. The window stays, empty after the last.

- Acts on a tile.
- Keys: ⌘W

### Close All Files

`tile.close_all`: Close every file in the window. The window stays, empty.

- Acts on every tile in a window.
- Keys: ⇧⌘W

### Close Other Files

`tile.close_others`: Close every file in the window but the tile's.

- Acts on a tile.

### Focus Tile to the Left

`tile.focus.left`: Send keys to the tile left of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘←

### Focus Tile to the Right

`tile.focus.right`: Send keys to the tile right of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘→

### Focus Tile Above

`tile.focus.up`: Send keys to the tile above the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↑

### Focus Tile Below

`tile.focus.down`: Send keys to the tile below the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↓

### Focus Previous Tile

`tile.focus.previous`: Send keys to the tile before the one they go to, the last before the first.

- Acts on a player window.
- Keys: ⌘[

### Focus Next Tile

`tile.focus.next`: Send keys to the tile after the one they go to, the first after the last.

- Acts on a player window.
- Keys: ⌘]

### Go Back One Tile

`tile.back`: Send keys to the tile clicked or gone to before the one they go to; again, the one before that, and so on, until something else is done.

- Acts on a player window.

### Go to Recent Tile

`tile.recent`: Send keys to the tile clicked or gone to last but the one they go to; again, back to that one.

- Acts on a player window.

### Move Tile Left

`tile.move.left`: Swap the tile keys go to with the one to its left.

- Acts on a player window.
- Keys: ⌥⇧⌘←

### Move Tile Right

`tile.move.right`: Swap the tile keys go to with the one to its right.

- Acts on a player window.
- Keys: ⌥⇧⌘→

### Move Tile Up

`tile.move.up`: Swap the tile keys go to with the one above it.

- Acts on a player window.
- Keys: ⌥⇧⌘↑

### Move Tile Down

`tile.move.down`: Swap the tile keys go to with the one below it.

- Acts on a player window.
- Keys: ⌥⇧⌘↓

### Maximize Tile

`tile.zoom`: Fill the window with the tile, or show every tile again; a window's only tile fills the screen instead.

- Acts on a tile.
- Keys: ⇧⌘↩

### Rename Tile…

`tile.rename`: Label the tile: its header shows the label in place of the file's name, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a tile.
- Arguments:
  - `label` (text, optional): What to call it; no two tiles are called the same, in whichever windows.

### Move Tile To

`tile.move`: Put the tile in another place in its window's grid, the tiles between moving over, as dragging it onto another does.

- Acts on a tile.
- Arguments:
  - `position` (a whole number, at least 1): Where, from 1, the first tile's place.

### Move Tile to Window

`tile.move_to_window`: Move the tile to another window, last there, where it plays on.

- Acts on a tile.
- Arguments:
  - `window` (text): The window's id, its label, or its number, from 1.

### Move Tile to New Window

`tile.move_to_new_window`: Move the tile to a window of its own beside this one, where it plays on.

- Acts on a tile.
- Hands back: {window}: the new window's id.

### Copy File Path

`tile.copy_path`: Copy the path of the tile's file, or the URL of its stream; hands back what was copied.

- Acts on a tile.
- Keys: ⌥⌘C
- Hands back: The path or URL copied.

### Copy Current Time

`tile.copy_time`: Copy where the tile's file is, as its bar shows it, such as 1:23:45; hands back what was copied.

- Acts on a tile.
- Hands back: The time copied, such as 1:23:45.

### Picture in Picture

`tile.toggle_picture_in_picture`: Play the tile in a small window floating above every app's, or in its tile again.

- Acts on a tile.
- Keys: ⌃⌘P

### Show/Hide Console

`tile.toggle_console`: Show what the tile's file is and how it plays, over its picture, or hide it.

- Acts on a tile.
- Keys: ~, `

## playback

### Play/Pause

`playback.play_pause`: Pause the tile if it plays, else play it.

- Acts on a tile.
- Keys: Space, P

### Seek To

`playback.seek`: Play from a time in the file, here or on the device it's cast to.

- Acts on a tile.
- Arguments:
  - `time` (a number, at least 0): Seconds from the start; past the end is the end.

### Seek Forward

`playback.seek_forward`: Skip ahead by the seek step set in Settings.

- Acts on a tile.
- Keys: →

### Seek Backward

`playback.seek_backward`: Skip back by the seek step set in Settings.

- Acts on a tile.
- Keys: ←

### Next Frame

`playback.next_frame`: Pause and show the next frame.

- Acts on a tile.
- Keys: ⌥→, ., >

### Previous Frame

`playback.previous_frame`: Pause and show the frame before.

- Acts on a tile.
- Keys: ⌥←, ,, <

### Next Chapter

`playback.next_chapter`: Play from the start of the next chapter.

- Acts on a tile.
- Keys: ⇟

### Previous Chapter

`playback.previous_chapter`: Play from the start of this chapter, or of the one before near its start.

- Acts on a tile.
- Keys: ⇞

### Go to Chapter

`playback.go_to_chapter`: Play from the start of a chapter.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1): Which chapter, from 1.

### Restart

`playback.restart`: Play from the start.

- Acts on a tile.
- Keys: ↖

### Previous File

`playback.previous_file`: Play the file before this one in its playlist, or else its folder, instead.

- Acts on a tile.
- Keys: ⌘←

### Next File

`playback.next_file`: Play the file after this one in its playlist, or else its folder, instead.

- Acts on a tile.
- Keys: ⌘→

### Play File Instead

`playback.play_file`: Play another file in the tile instead, one in its folder say, at the same volume, muted and looping as this one is, and playing if it is.

- Acts on a tile.
- Arguments:
  - `path` (text): The file's full path.

### Loop

`playback.toggle_loop`: Play the file again from the start at its end, or stop at its end.

- Acts on a tile.
- Keys: ⇧⌘L

### Play/Pause All

`playback.play_pause_all`: Pause every file in the window if any plays, else play them all.

- Acts on every tile in a window.
- Keys: ⇧Space

### Play All

`playback.play_all`: Play every file in the window.

- Acts on every tile in a window.

### Pause All

`playback.pause_all`: Pause every file in the window.

- Acts on every tile in a window.

### Restart All

`playback.restart_all`: Play every file in the window from the start.

- Acts on every tile in a window.
- Keys: ⇧↖

### Seek All Forward

`playback.seek_forward_all`: Skip every file in the window ahead by the seek step.

- Acts on every tile in a window.
- Keys: ⇧→

### Seek All Backward

`playback.seek_backward_all`: Skip every file in the window back by the seek step.

- Acts on every tile in a window.
- Keys: ⇧←

### Next Frame on All

`playback.next_frame_all`: Pause every file in the window and show its next frame.

- Acts on every tile in a window.
- Keys: ⌥⇧→

### Previous Frame on All

`playback.previous_frame_all`: Pause every file in the window and show the frame before.

- Acts on every tile in a window.
- Keys: ⌥⇧←

### Sync All to This Tile

`playback.sync_all`: Bring every file in the window to where the tile keys go to is, playing or paused as it is.

- Acts on every tile in a window.
- Keys: ⇧S

## audio

### Volume Up

`audio.volume_up`: Turn the tile's volume up, unmuting it.

- Acts on a tile.
- Keys: ↑

### Volume Down

`audio.volume_down`: Turn the tile's volume down.

- Acts on a tile.
- Keys: ↓

### Set Volume

`audio.set_volume`: Set the tile's volume, unmuting it unless it's none.

- Acts on a tile.
- Arguments:
  - `volume` (a number from 0 to 100): How loud, from 0 to 100.

### Mute/Unmute

`audio.toggle_mute`: Mute the tile, or play its sound again.

- Acts on a tile.
- Keys: ⌘/, M

### Next Audio Track

`audio.next_track`: Play the file's next audio track, the first after the last.

- Acts on a tile.
- Keys: ⌃⌘A

### Pick Audio Track

`audio.select_track`: Play one of the file's audio tracks.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.

### Audio Earlier

`audio.earlier`: Play the tile's sound a little earlier than its picture.

- Acts on a tile.
- Keys: ⌃-

### Audio Later

`audio.later`: Play the tile's sound a little later than its picture.

- Acts on a tile.
- Keys: ⌃=

### Reset Audio Delay

`audio.reset_delay`: Play the tile's sound with its picture again.

- Acts on a tile.
- Keys: ⌃⌫

### Volume Up on All

`audio.volume_up_all`: Turn every file's volume in the window up, each from its own.

- Acts on every tile in a window.
- Keys: ⇧↑

### Volume Down on All

`audio.volume_down_all`: Turn every file's volume in the window down, each from its own.

- Acts on every tile in a window.
- Keys: ⇧↓

### Mute/Unmute All

`audio.toggle_mute_all`: Mute every file in the window if any plays its sound, else unmute them all.

- Acts on every tile in a window.
- Keys: ⇧M

## subtitles

### Next Subtitles

`subtitles.next_track`: Show the file's next subtitles, and none after the last.

- Acts on a tile.
- Keys: ⌃⌘S

### Pick Subtitles

`subtitles.select_track`: Show one of the file's subtitles, or none.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1, optional): Which, from 1, as the tracks menu lists them; none for no subtitles.

### Show/Hide Subtitles

`subtitles.toggle`: Hide the subtitles showing, or show the ones hidden.

- Acts on a tile.
- Keys: V

### Bigger Subtitles

`subtitles.bigger`: Make every file's subtitles bigger.

- Acts on a tile.
- Keys: ⇧G

### Smaller Subtitles

`subtitles.smaller`: Make every file's subtitles smaller.

- Acts on a tile.
- Keys: ⇧F

### Move Subtitles Up

`subtitles.up`: Show every file's subtitles higher up.

- Acts on a tile.
- Keys: R

### Move Subtitles Down

`subtitles.down`: Show every file's subtitles lower down.

- Acts on a tile.
- Keys: ⇧R, T

### Subtitles Earlier

`subtitles.earlier`: Show the tile's subtitles a little earlier.

- Acts on a tile.
- Keys: Z

### Subtitles Later

`subtitles.later`: Show the tile's subtitles a little later.

- Acts on a tile.
- Keys: X

### Reset Subtitle Delay

`subtitles.reset_delay`: Show the tile's subtitles at their own times again.

- Acts on a tile.
- Keys: ⇧⌫

## video

### Next Video Track

`video.next_track`: Show the file's next video track, the first after the last.

- Acts on a tile.
- Keys: ⌃⌘V

### Pick Video Track

`video.select_track`: Show one of the file's video tracks.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.

### Rotate Right

`video.rotate_right`: Turn the picture a quarter turn clockwise.

- Acts on a tile.
- Keys: ⌘R

### Rotate Left

`video.rotate_left`: Turn the picture a quarter turn counterclockwise.

- Acts on a tile.
- Keys: ⌘L

### Flip Horizontally

`video.flip_horizontally`: Mirror the picture left to right.

- Acts on a tile.
- Keys: ⇧⌘H

### Flip Vertically

`video.flip_vertically`: Turn the picture upside down, mirrored.

- Acts on a tile.
- Keys: ⇧⌘V

### Next Aspect Ratio

`video.next_aspect_ratio`: Show the picture at the next aspect ratio, then as the file is.

- Acts on a tile.
- Keys: A

### Aspect Ratio

`video.set_aspect_ratio`: Show the picture at an aspect ratio, or as the file is.

- Acts on a tile.
- Arguments:
  - `ratio` (one of 1:1, 4:3, 16:10, 16:9, 1.85:1, 2.35:1, optional): The aspect ratio; none for the file's own.

### Next Crop

`video.next_crop`: Crop the picture to the next shape, then not at all.

- Acts on a tile.
- Keys: C

### Crop

`video.set_crop`: Crop the picture to a shape, or not at all.

- Acts on a tile.
- Arguments:
  - `crop` (one of 1:1, 4:3, 16:9, 1.85:1, 2.39:1, optional): The shape; none for no crop.

### Reset Picture

`video.reset_picture`: Show the picture as the file is: not turned, flipped, reshaped or cropped.

- Acts on a tile.
- Keys: ⌘0

### Take Screenshot

`video.take_screenshot`: Save the frame showing as a file, and say where it went.

- Acts on a tile.
- Keys: S

## cast

### List Devices

`cast.ls`: List the TVs and speakers found on the network to play on, by how they're reached.

- Acts on the app.
- Hands back: {Chromecast: [{id, name, shows}], AirPlay: […], DLNA: […]}: each protocol's devices found, by name, shows being video for a TV and sound for a speaker; a protocol with none found isn't there.

### Play On

`cast.to`: Play the tile on a TV or speaker found on the network, from where it is.

- Acts on a tile.
- Arguments:
  - `device` (text): The device's id.
  - `kind` (text, optional): How it's reached, such as Chromecast, AirPlay or DLNA, for an id two share.

### Play Here

`cast.stop`: Stop casting the tile, and play it here from where the device got to.

- Acts on a tile.

### Pair and Play On

`cast.pair`: Pair with a TV that shows a PIN to type, such as an Apple TV, then play the tile on it.

- Acts on a tile.
- Arguments:
  - `device` (text): The device's id.
  - `kind` (text, optional): How it's reached, such as Chromecast, AirPlay or DLNA, for an id two share.

## output

### List Outputs

`output.ls`: List the outputs connected, the one picked, and where the sound goes now.

- Acts on the app.
- Hands back: {outputs: [{id, name, kind}], device: {id, name} or null, route}: the outputs connected (kind AirPlay for a speaker on the network, else null), the one picked (null for the system's default), and the route: {playing: {device, rate, channels, instead_of}} (instead_of naming the one picked, when the system's default stands in for it), {waiting: name}, {opening: name} or {unavailable: why}.

### Output Device

`output.set_device`: Play the app's sound through an output, or the system's default. A Bluetooth one paired and not connected is connected.

- Acts on the app.
- Arguments:
  - `id` (text, optional): The output's id; none for the system's default, or for a window, the app's output.
  - `name` (text, optional): What it's called, for an output not connected now, which is waited for.

### Window Output Device

`output.set_window_device`: Play the window's sound through an output of its own, or through the app's again.

- Acts on a player window.
- Arguments:
  - `id` (text, optional): The output's id; none for the system's default, or for a window, the app's output.
  - `name` (text, optional): What it's called, for an output not connected now, which is waited for.

## display

### List Displays

`display.ls`: List the displays connected, in the system's order.

- Acts on the app.
- Hands back: [{id, name, bounds: {x, y, width, height}}]: each display's id, as window.show_on_display and window.inspect have it, its name, and where it is, in points.

## settings

### List Settings

`settings.ls`: List every setting: its key, what Settings calls it and the pane it's on, what it takes, and its value.

- Acts on the app.
- Hands back: [{key, title, pane, kind, value, default}], in the order Settings has them: kind is toggle, choice (with choices: [{value, label}]), number (with min and max), font, folder, output, keys or languages (a list, by name or code, kept by code).

### Get Setting

`settings.get`: Read a setting by its key, such as subtitles.size.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
- Hands back: Its value, as the settings file keeps it.

### List Shortcuts

`settings.keymap`: List each command keys can run, with the keys that run it now, as Settings → Hotkeys has them.

- Acts on the app.
- Hands back: [{command, title, keys, shown}]: each command's id and title, its keys as the settings file keeps them, and as the menus show them; none for a command with no keys.

### Change Setting

`settings.set`: Change a setting by its key, to a value Settings offers, and save it.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
  - `value` (any value): What to set it to, as the settings file keeps it: true, 0.5, "dark".
- Hands back: Its value now.

### Reset Setting

`settings.reset`: Put a setting back as Plays starts it out, and save it.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
- Hands back: Its value now.

### Show Settings File

`settings.reveal_file`: Show the settings file where the system shows files, saving it first if it isn't there yet.

- Acts on the app.
- Hands back: The file's path.

### Reload Settings

`settings.reload`: Read the settings file again, after it was changed by hand.

- Acts on the app.
- Hands back: [key]: the settings the file changed.

## cli

### Install Command Line Tool

`cli.install`: Link plays into /usr/local/bin, so a terminal can run it: to open files, run commands and hear what changes.

- Acts on the app.
- Hands back: The link's path.
