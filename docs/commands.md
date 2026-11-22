# Commands

Every command Plays has, by namespace, as `plays actions --markdown` writes them. A terminal runs one with `plays do ID [KEY=VALUE…]`, naming the window or tile it acts on with `-w` and `-t`; keys run those that have them, and Settings → Hotkeys changes those. The keys here are macOS's: elsewhere Ctrl stands for ⌘, and Alt for ⌥. What each tells is in [Events](#events).

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
- Tells: `window_created`, `active_window_changed`, `tile_created`, `window_layout_changed`, `active_tile_changed`, `opened`.

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
- Tells: `tile_closed`, `window_closed`, `active_window_changed`, `quit`.

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
- Tells: `window_created`, `active_window_changed`.

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
- Tells: `tile_closed`, `window_closed`, `active_window_changed`.

### Close Other Windows

`window.close_others`: Close every player window but this one, and the files in them.

- Acts on a player window.
- Tells: `tile_closed`, `window_closed`.

### Rename Window…

`window.rename`: Label the window: its title shows the label, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a player window.
- Arguments:
  - `label` (text, optional): What to call it; no two windows are called the same.
- Tells: `window_label_changed`.

### Go to Window

`window.focus`: Bring the window to the front. ⌘1 to ⌘9 go to the first nine, by their numbers.

- Acts on a player window.
- Tells: `active_window_changed`.

### Next Window

`window.next`: Bring the window after the one in front to the front: the first after the last.

- Acts on the app.
- Keys: ⌃⇥
- Tells: `active_window_changed`.

### Previous Window

`window.previous`: Bring the window before the one in front to the front: the last before the first.

- Acts on the app.
- Keys: ⌃⇧⇥
- Tells: `active_window_changed`.

### Go Back One Window

`window.back`: Bring back the player window in front before this one; again, the one before that, and so on, until something else is done.

- Acts on the app.
- Tells: `active_window_changed`.

### Go to Recent Window

`window.recent`: Bring back the player window in front last; again, the one in front first.

- Acts on the app.
- Keys: ⌘'
- Tells: `active_window_changed`.

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
- Tells: `full_screen_changed`.

### Exit Full Screen

`window.exit_full_screen`: Show the window as it was before it was full screen.

- Acts on a player window.
- Keys: ⎋
- Tells: `full_screen_changed`.

### Match Video Size

`window.match_video_size`: Size the window for its videos to show at their own size, as far as the screen allows.

- Acts on a player window.
- Keys: ⌥⌘0

### Float on Top

`window.toggle_float_on_top`: Keep player windows above other apps' windows, or stop.

- Acts on the app.
- Tells: `settings_changed`.

### Float on Top While Playing

`window.toggle_float_on_top_while_playing`: Keep player windows above other apps' windows while a file in them plays, or stop.

- Acts on the app.
- Tells: `settings_changed`.

### Show on Display

`window.show_on_display`: Show the window full screen on a display, coming back where it was once it leaves full screen there.

- Acts on a player window.
- Arguments:
  - `display` (a whole number, at least 0): The display's id.
- Tells: `full_screen_changed`.

## view

### Show Controls

`view.toggle_controls`: Show the seek bar and buttons at the bottom of each tile, or never show them.

- Acts on the app.
- Tells: `settings_changed`.

### Show Top Bar

`view.toggle_top_bar`: Show the file's name and buttons at the top of each tile, or never show them.

- Acts on the app.
- Tells: `settings_changed`.

### Show Track Switcher

`view.toggle_track_switcher`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, or switch without it.

- Acts on the app.
- Tells: `settings_changed`.

### Show Notes

`view.toggle_notes`: Show notes on what a shortcut changed, like the volume, or not. Notes on what's wrong still show.

- Acts on the app.
- Tells: `settings_changed`.

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
- Tells: `active_window_changed`, `active_tile_changed`.

### Close File

`tile.close`: Close the tile's file. The window stays, empty after the last.

- Acts on a tile.
- Keys: ⌘W
- Tells: `tile_closed`, `window_layout_changed`, `active_tile_changed`.

### Close All Files

`tile.close_all`: Close every file in the window. The window stays, empty.

- Acts on every tile in a window.
- Keys: ⇧⌘W
- Tells: `tile_closed`, `window_layout_changed`, `active_tile_changed`.

### Close Other Files

`tile.close_others`: Close every file in the window but the tile's.

- Acts on a tile.
- Tells: `tile_closed`, `window_layout_changed`.

### Focus Tile to the Left

`tile.focus.left`: Send keys to the tile left of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘←
- Tells: `active_tile_changed`.

### Focus Tile to the Right

`tile.focus.right`: Send keys to the tile right of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘→
- Tells: `active_tile_changed`.

### Focus Tile Above

`tile.focus.up`: Send keys to the tile above the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↑
- Tells: `active_tile_changed`.

### Focus Tile Below

`tile.focus.down`: Send keys to the tile below the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↓
- Tells: `active_tile_changed`.

### Focus Previous Tile

`tile.focus.previous`: Send keys to the tile before the one they go to, the last before the first.

- Acts on a player window.
- Keys: ⌘[
- Tells: `active_tile_changed`.

### Focus Next Tile

`tile.focus.next`: Send keys to the tile after the one they go to, the first after the last.

- Acts on a player window.
- Keys: ⌘]
- Tells: `active_tile_changed`.

### Go Back One Tile

`tile.back`: Send keys to the tile clicked or gone to before the one they go to; again, the one before that, and so on, until something else is done.

- Acts on a player window.
- Tells: `active_tile_changed`.

### Go to Recent Tile

`tile.recent`: Send keys to the tile clicked or gone to last but the one they go to; again, back to that one.

- Acts on a player window.
- Tells: `active_tile_changed`.

### Move Tile Left

`tile.move.left`: Swap the tile keys go to with the one to its left.

- Acts on a player window.
- Keys: ⌥⇧⌘←
- Tells: `window_layout_changed`.

### Move Tile Right

`tile.move.right`: Swap the tile keys go to with the one to its right.

- Acts on a player window.
- Keys: ⌥⇧⌘→
- Tells: `window_layout_changed`.

### Move Tile Up

`tile.move.up`: Swap the tile keys go to with the one above it.

- Acts on a player window.
- Keys: ⌥⇧⌘↑
- Tells: `window_layout_changed`.

### Move Tile Down

`tile.move.down`: Swap the tile keys go to with the one below it.

- Acts on a player window.
- Keys: ⌥⇧⌘↓
- Tells: `window_layout_changed`.

### Maximize Tile

`tile.zoom`: Fill the window with the tile, or show every tile again; a window's only tile fills the screen instead.

- Acts on a tile.
- Keys: ⇧⌘↩
- Tells: `full_screen_changed`, `window_layout_changed`.

### Rename Tile…

`tile.rename`: Label the tile: its header shows the label in place of the file's name, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a tile.
- Arguments:
  - `label` (text, optional): What to call it; no two tiles are called the same, in whichever windows.
- Tells: `tile_label_changed`.

### Move Tile To

`tile.move`: Put the tile in another place in its window's grid, the tiles between moving over, as dragging it onto another does.

- Acts on a tile.
- Arguments:
  - `position` (a whole number, at least 1): Where, from 1, the first tile's place.
- Tells: `window_layout_changed`.

### Move Tile to Window

`tile.move_to_window`: Move the tile to another window, last there, where it plays on.

- Acts on a tile.
- Arguments:
  - `window` (text): The window's id, its label, or its number, from 1.
- Tells: `tile_moved`, `window_layout_changed`, `active_tile_changed`.

### Move Tile to New Window

`tile.move_to_new_window`: Move the tile to a window of its own beside this one, where it plays on.

- Acts on a tile.
- Hands back: {window}: the new window's id.
- Tells: `window_created`, `tile_moved`, `window_layout_changed`, `active_tile_changed`.

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
- Tells: `picture_in_picture_changed`.

### Show/Hide Console

`tile.toggle_console`: Show what the tile's file is and how it plays, over its picture, or hide it.

- Acts on a tile.
- Keys: ~, `

## playback

### Play/Pause

`playback.play_pause`: Pause the tile if it plays, else play it.

- Acts on a tile.
- Keys: Space, P
- Tells: `playing`, `paused`.

### Seek To

`playback.seek`: Play from a time in the file, here or on the device it's cast to.

- Acts on a tile.
- Arguments:
  - `time` (a number, at least 0): Seconds from the start; past the end is the end.
- Tells: `seeked`, `ended`, `chapter_changed`.

### Seek Forward

`playback.seek_forward`: Skip ahead by the seek step set in Settings.

- Acts on a tile.
- Keys: →
- Tells: `seeked`, `ended`, `chapter_changed`.

### Seek Backward

`playback.seek_backward`: Skip back by the seek step set in Settings.

- Acts on a tile.
- Keys: ←
- Tells: `seeked`, `chapter_changed`.

### Next Frame

`playback.next_frame`: Pause and show the next frame.

- Acts on a tile.
- Keys: ⌥→, ., >
- Tells: `paused`, `seeked`.

### Previous Frame

`playback.previous_frame`: Pause and show the frame before.

- Acts on a tile.
- Keys: ⌥←, ,, <
- Tells: `paused`, `seeked`.

### Next Chapter

`playback.next_chapter`: Play from the start of the next chapter.

- Acts on a tile.
- Keys: ⇟
- Tells: `seeked`, `chapter_changed`.

### Previous Chapter

`playback.previous_chapter`: Play from the start of this chapter, or of the one before near its start.

- Acts on a tile.
- Keys: ⇞
- Tells: `seeked`, `chapter_changed`.

### Go to Chapter

`playback.go_to_chapter`: Play from the start of a chapter.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1): Which chapter, from 1.
- Tells: `seeked`, `chapter_changed`.

### Restart

`playback.restart`: Play from the start.

- Acts on a tile.
- Keys: ↖
- Tells: `seeked`, `chapter_changed`.

### Previous File

`playback.previous_file`: Play the file before this one in its playlist, or else its folder, instead.

- Acts on a tile.
- Keys: ⌘←
- Tells: `opened`, `playing`.

### Next File

`playback.next_file`: Play the file after this one in its playlist, or else its folder, instead.

- Acts on a tile.
- Keys: ⌘→
- Tells: `opened`, `playing`.

### Play File Instead

`playback.play_file`: Play another file in the tile instead, one in its folder say, at the same volume, muted and looping as this one is, and playing if it is.

- Acts on a tile.
- Arguments:
  - `path` (text): The file's full path.
- Tells: `opened`, `playing`.

### Loop

`playback.toggle_loop`: Play the file again from the start at its end, or stop at its end.

- Acts on a tile.
- Keys: ⇧⌘L

### Play/Pause All

`playback.play_pause_all`: Pause every file in the window if any plays, else play them all.

- Acts on every tile in a window.
- Keys: ⇧Space
- Tells: `playing`, `paused`.

### Play All

`playback.play_all`: Play every file in the window.

- Acts on every tile in a window.
- Tells: `playing`.

### Pause All

`playback.pause_all`: Pause every file in the window.

- Acts on every tile in a window.
- Tells: `paused`.

### Restart All

`playback.restart_all`: Play every file in the window from the start.

- Acts on every tile in a window.
- Keys: ⇧↖
- Tells: `seeked`, `chapter_changed`.

### Seek All Forward

`playback.seek_forward_all`: Skip every file in the window ahead by the seek step.

- Acts on every tile in a window.
- Keys: ⇧→
- Tells: `seeked`, `ended`, `chapter_changed`.

### Seek All Backward

`playback.seek_backward_all`: Skip every file in the window back by the seek step.

- Acts on every tile in a window.
- Keys: ⇧←
- Tells: `seeked`, `chapter_changed`.

### Next Frame on All

`playback.next_frame_all`: Pause every file in the window and show its next frame.

- Acts on every tile in a window.
- Keys: ⌥⇧→
- Tells: `paused`, `seeked`.

### Previous Frame on All

`playback.previous_frame_all`: Pause every file in the window and show the frame before.

- Acts on every tile in a window.
- Keys: ⌥⇧←
- Tells: `paused`, `seeked`.

### Sync All to This Tile

`playback.sync_all`: Bring every file in the window to where the tile keys go to is, playing or paused as it is.

- Acts on every tile in a window.
- Keys: ⇧S
- Tells: `seeked`, `playing`, `paused`.

## audio

### Volume Up

`audio.volume_up`: Turn the tile's volume up, unmuting it.

- Acts on a tile.
- Keys: ↑
- Tells: `volume_changed`.

### Volume Down

`audio.volume_down`: Turn the tile's volume down.

- Acts on a tile.
- Keys: ↓
- Tells: `volume_changed`.

### Set Volume

`audio.set_volume`: Set the tile's volume, unmuting it unless it's none.

- Acts on a tile.
- Arguments:
  - `volume` (a number from 0 to 100): How loud, from 0 to 100.
- Tells: `volume_changed`.

### Mute/Unmute

`audio.toggle_mute`: Mute the tile, or play its sound again.

- Acts on a tile.
- Keys: ⌘/, M
- Tells: `volume_changed`.

### Next Audio Track

`audio.next_track`: Play the file's next audio track, the first after the last.

- Acts on a tile.
- Keys: ⌃⌘A
- Tells: `track_changed`.

### Pick Audio Track

`audio.select_track`: Play one of the file's audio tracks.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Tells: `track_changed`.

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
- Tells: `volume_changed`.

### Volume Down on All

`audio.volume_down_all`: Turn every file's volume in the window down, each from its own.

- Acts on every tile in a window.
- Keys: ⇧↓
- Tells: `volume_changed`.

### Mute/Unmute All

`audio.toggle_mute_all`: Mute every file in the window if any plays its sound, else unmute them all.

- Acts on every tile in a window.
- Keys: ⇧M
- Tells: `volume_changed`.

## subtitles

### Next Subtitles

`subtitles.next_track`: Show the file's next subtitles, and none after the last.

- Acts on a tile.
- Keys: ⌃⌘S
- Tells: `track_changed`.

### Pick Subtitles

`subtitles.select_track`: Show one of the file's subtitles, or none.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1, optional): Which, from 1, as the tracks menu lists them; none for no subtitles.
- Tells: `track_changed`.

### Show/Hide Subtitles

`subtitles.toggle`: Hide the subtitles showing, or show the ones hidden.

- Acts on a tile.
- Keys: V
- Tells: `track_changed`.

### Bigger Subtitles

`subtitles.bigger`: Make every file's subtitles bigger.

- Acts on a tile.
- Keys: ⇧G
- Tells: `settings_changed`.

### Smaller Subtitles

`subtitles.smaller`: Make every file's subtitles smaller.

- Acts on a tile.
- Keys: ⇧F
- Tells: `settings_changed`.

### Move Subtitles Up

`subtitles.up`: Show every file's subtitles higher up.

- Acts on a tile.
- Keys: R
- Tells: `settings_changed`.

### Move Subtitles Down

`subtitles.down`: Show every file's subtitles lower down.

- Acts on a tile.
- Keys: ⇧R, T
- Tells: `settings_changed`.

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
- Tells: `track_changed`.

### Pick Video Track

`video.select_track`: Show one of the file's video tracks.

- Acts on a tile.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Tells: `track_changed`.

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
- Tells: `cast_started`.

### Play Here

`cast.stop`: Stop casting the tile, and play it here from where the device got to.

- Acts on a tile.
- Tells: `cast_ended`.

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
- Tells: `route_changed`, `settings_changed`.

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
- Tells: `settings_changed`, `keymap_changed`.

### Reset Setting

`settings.reset`: Put a setting back as Plays starts it out, and save it.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
- Hands back: Its value now.
- Tells: `settings_changed`, `keymap_changed`.

### Show Settings File

`settings.reveal_file`: Show the settings file where the system shows files, saving it first if it isn't there yet.

- Acts on the app.
- Hands back: The file's path.

### Reload Settings

`settings.reload`: Read the settings file again, after it was changed by hand.

- Acts on the app.
- Hands back: [key]: the settings the file changed.
- Tells: `settings_changed`, `keymap_changed`.

## cli

### Install Command Line Tool

`cli.install`: Link plays into /usr/local/bin, so a terminal can run it: to open files, run commands and hear what changes.

- Acts on the app.
- Hands back: The link's path.

# Events

What changes in Plays, its windows and their tiles, by namespace: `plays events` prints each as it comes, and `plays wait` waits for one. Each is a JSON object, `{type, payload}`. The payload of one about a window starts with `window_id` and `window_title`, as its title bar shows it; of one about a tile, with `tile_id` and `tile_title` too, as its header shows it. A listener hears the app's, one window's, or all of them. Readers ignore events and fields they don't know.

## app

### command_run

A command is about to run: told before anything it changes.

- Heard by those listening to the app's events.
- Payload:
  - `command`: Its id.
  - `source`: Where it was run from: action (a key, a menu item, or a control that sends one), ui (a control that runs it itself), palette, or cli.
  - `args`, only sometimes: The arguments it was given, if any.
  - `target`, only sometimes: The window and the tile it was aimed at, by name, if either was.
- Follows: every command, as it runs.

### client_connected

A client connected to the socket, as the command line does.

- Heard by those listening to the app's events.
- Payload:
  - `client_id`: A number for it, which its `client_left` has too.
- Follows: a client connecting.

### client_left

A client went from the socket.

- Heard by those listening to the app's events.
- Payload:
  - `client_id`: The number its `client_connected` had.
- Follows: a client going or being cut off, as when control from the command line is turned off.

### quit

Plays is quitting: told last, after each window still open is told closing.

- Heard by every listener.
- Payload: none.
- Follows: `app.quit`, or Plays quitting from the Dock or with the system.

## window

### window_created

A player window opened.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `label`: Its label; none without one.
- Follows: `app.open`, `window.new`, or `tile.move_to_new_window`.

### window_closed

A player window closed: told after each of its tiles is told closed.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
- Follows: `app.quit`, `window.close`, `window.close_others`, or Plays quitting.

### window_label_changed

A window was labeled, or its label taken away.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `label`: Its label; none once it's taken away.
- Follows: `window.rename`.

### active_window_changed

Another player window came to the front, or none is in front: its `window_id` and `window_title` are none then.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
- Follows: `app.open`, `app.quit`, `window.new`, `window.close`, `window.focus`, `window.next`, `window.previous`, `window.back`, `window.recent`, `tile.focus`, or a click on another window.

### full_screen_changed

A window went full screen, or came back from it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `full_screen`: Whether it's full screen now.
- Follows: `window.toggle_full_screen`, `window.exit_full_screen`, `window.show_on_display`, `tile.zoom`, or the window's own full screen button.

### window_layout_changed

Tiles came into a window, went, or changed places, or one filled it or stopped filling it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `tiles`: The ids of its tiles, in the order its grid shows them.
  - `maximized`: The id of the tile filling it; none if none does.
- Follows: `app.open`, `tile.close`, `tile.close_all`, `tile.close_others`, `tile.move.left`, `tile.move.right`, `tile.move.up`, `tile.move.down`, `tile.zoom`, `tile.move`, `tile.move_to_window`, `tile.move_to_new_window`, or files dropped on a window or picked when `window.open_files` or `window.open_url` asks.

## tile

### tile_created

A tile came into a window, for a file to play in.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `label`: Its label; none without one.
- Follows: `app.open`, or files dropped on a window or picked when `window.open_files` or `window.open_url` asks.

### tile_moved

A tile moved to another window: those listening to the window it left hear it too.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `from_window_id`: The id of the window it left.
  - `from_window_title`: That window's title.
- Follows: `tile.move_to_window`, or `tile.move_to_new_window`.

### tile_closed

A tile closed, and the file in it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
- Follows: `app.quit`, `window.close`, `window.close_others`, `tile.close`, `tile.close_all`, `tile.close_others`, or Plays quitting.

### tile_label_changed

A tile was labeled, or its label taken away.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `label`: Its label; none once it's taken away.
- Follows: `tile.rename`.

### active_tile_changed

Keys go to another tile in a window, or to none.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `tile_id`: The tile's id; none if keys go to none.
  - `tile_title`: Its title, as its header shows it; none without a tile.
- Follows: `app.open`, `tile.focus`, `tile.close`, `tile.close_all`, `tile.focus.left`, `tile.focus.right`, `tile.focus.up`, `tile.focus.down`, `tile.focus.previous`, `tile.focus.next`, `tile.back`, `tile.recent`, `tile.move_to_window`, `tile.move_to_new_window`, or a click on a tile or the mouse going to one.

### picture_in_picture_changed

A tile began playing in a small window floating above every app's, or in its tile again.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `picture_in_picture`: Whether it plays in the small window now.
- Follows: `tile.toggle_picture_in_picture`, or the small window's own buttons.

## playback

### opened

A file opened in a tile, paused at its start.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `title`: What its header shows without a label: the file's name, or the stream's.
  - `file`: Its path; none for a stream.
  - `url`: The stream's URL; none for a file.
  - `duration`: How long it plays, in seconds; none for a live stream.
- Follows: `app.open`, `playback.previous_file`, `playback.next_file`, `playback.play_file`, files dropped on a window or picked when `window.open_files` or `window.open_url` asks, or a playlist going on to its next file, once one plays to its end.

### open_failed

A file couldn't be opened.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `error`: Why.
- Follows: a file Plays can't play or a stream it can't reach.

### failed

A file stopped playing on an error.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `error`: Why.
- Follows: a file that's damaged or a stream that went.

### playing

A file began playing.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.play_pause`, `playback.previous_file`, `playback.next_file`, `playback.play_file`, `playback.play_pause_all`, `playback.play_all`, `playback.sync_all`, a file opening while Autoplay is on, or a playlist going on to its next file, once one plays to its end.

### paused

A file paused.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.play_pause`, `playback.next_frame`, `playback.previous_frame`, `playback.play_pause_all`, `playback.pause_all`, `playback.next_frame_all`, `playback.previous_frame_all`, or `playback.sync_all`.

### seeked

A file was sought to another time, here or on the device it's cast to: where it went.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.seek`, `playback.seek_forward`, `playback.seek_backward`, `playback.next_frame`, `playback.previous_frame`, `playback.next_chapter`, `playback.previous_chapter`, `playback.go_to_chapter`, `playback.restart`, `playback.restart_all`, `playback.seek_forward_all`, `playback.seek_backward_all`, `playback.next_frame_all`, `playback.previous_frame_all`, or `playback.sync_all`.

### ended

A file played to its end, and stopped there.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.seek`, `playback.seek_forward`, `playback.seek_forward_all`, or a file playing to its end.

### position

Where a file is as it plays: told only to those who ask for positions, at most once a second, and only once it has moved.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
  - `duration`: How long it plays, in seconds; none for a live stream.
- Follows: a file playing.

### chapter_changed

A file is in another chapter.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `chapter`: The chapter, from 1; none before the first.
- Follows: `playback.seek`, `playback.seek_forward`, `playback.seek_backward`, `playback.next_chapter`, `playback.previous_chapter`, `playback.go_to_chapter`, `playback.restart`, `playback.restart_all`, `playback.seek_forward_all`, `playback.seek_backward_all`, or a file playing into its next chapter.

### track_changed

Another of a file's tracks plays, or none of its kind.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `kind`: The track's: video, audio or subtitles.
  - `track`: The one playing, from 1 among its kind's, as the tracks menu lists them; none for none.
- Follows: `audio.next_track`, `audio.select_track`, `subtitles.next_track`, `subtitles.select_track`, `subtitles.toggle`, `video.next_track`, or `video.select_track`.

## audio

### volume_changed

A tile's volume changed, or it was muted or unmuted.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `volume`: Its volume, in percent, as `audio.set_volume` takes it.
  - `muted`: Whether it's muted.
- Follows: `audio.volume_up`, `audio.volume_down`, `audio.set_volume`, `audio.toggle_mute`, `audio.volume_up_all`, `audio.volume_down_all`, or `audio.toggle_mute_all`.

## cast

### cast_devices_changed

TVs or speakers to cast to were found on the network, or went.

- Heard by those listening to the app's events.
- Payload:
  - `devices`: Those found now, as `cast.ls` lists them.
- Follows: devices found on the network or gone from it.

### cast_started

A tile began playing on a TV or speaker.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `device`: The device: its id, kind and name.
- Follows: `cast.to`, or a TV paired with `cast.pair` once its PIN is typed.

### cast_ended

A tile stopped playing on a TV or speaker.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `tile_id`, `tile_title`: its window's and its tile's.
  - `device`: The device it played on: its id, kind and name.
- Follows: `cast.stop`, or the device stopping or leaving the network.

## output

### outputs_changed

Outputs were connected or disconnected.

- Heard by those listening to the app's events.
- Payload:
  - `outputs`: Those connected now, as `output.ls` lists them.
- Follows: an output connected or disconnected.

### route_changed

The app's sound goes elsewhere.

- Heard by those listening to the app's events.
- Payload:
  - `route`: Where it goes now, as `output.ls` says.
- Follows: `output.set_device`, or the system's own output changing.

## settings

### settings_changed

Settings changed.

- Heard by those listening to the app's events.
- Payload:
  - `keys`: The keys of those that changed, as `settings.get` takes them.
- Follows: `window.toggle_float_on_top`, `window.toggle_float_on_top_while_playing`, `view.toggle_controls`, `view.toggle_top_bar`, `view.toggle_track_switcher`, `view.toggle_notes`, `subtitles.bigger`, `subtitles.smaller`, `subtitles.up`, `subtitles.down`, `output.set_device`, `settings.set`, `settings.reset`, `settings.reload`, or Settings moving a slider, importing a settings file or resetting every setting.

### keymap_changed

The keys that run commands changed.

- Heard by those listening to the app's events.
- Payload:
  - `commands`: The ids of the commands whose keys changed.
- Follows: `settings.set`, `settings.reset`, `settings.reload`, or Settings importing a settings file or resetting every setting.
