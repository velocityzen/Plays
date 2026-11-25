# Commands

Every command Plays has, by namespace, as `plays actions --markdown` writes them. A terminal runs one with `plays do ID [KEY=VALUE…]`, naming the window or block it acts on with `-w` and `-b`; keys run those that have them, and Settings → Hotkeys changes those. The keys here are macOS's: elsewhere Ctrl stands for ⌘, and Alt for ⌥. What each tells is in [Events](#events).

## app

### Inspect Plays

`app.inspect`: Say what Plays is: its version, its player windows, and where the sound goes.

- Acts on the app.
- Hands back: {version, windows: [as window.ls], output: as output.ls}.

### Open Paths or URLs

`app.open`: Play files, the media files in folders, and streams, in the window named, or the one in front, or a new one if none is open: each in a block, or one after another in one, as Settings say.

- Acts on a player window.
- Arguments:
  - `urls` (a list of text): Each a file's or folder's full path, a file:// URL, or a stream's URL.
- Hands back: {window}: the id of the window they open in.
- Tells: `window.created`, `window.active_changed`, `block.created`, `window.layout_changed`, `block.active_changed`, `playback.opened`.

### Clear Recent Files

`app.clear_recent_files`: Forget the files played lately, which File → Open Recent and Go to… offer.

- Acts on the app.

### About Plays

`app.about`: Show Plays' version, and what it's made with.

- Acts on the app.

### Make Plays the Default Player…

`app.default_player`: Open Settings at Default Player, to pick the formats Plays opens when their files are opened: from Finder or a file manager, say.

- Acts on the app.

### Check for Updates…

`app.check_for_updates`: Open Settings at About, and check for a newer Plays: one found downloads in the background, to install as Plays restarts or quits.

- Acts on the app.

### Restart to Update

`app.restart_to_update`: Install the update downloaded, and start Plays again to run it.

- Acts on the app.

### Copy Version Info

`app.copy_version_info`: Copy Plays' version and the day it was built.

- Acts on the app.
- Hands back: What it copied, such as Plays 0.1.0 (18 November 2026).

### Copy Debug Info

`app.copy_debug_info`: Copy what a bug report needs: Plays' version, the system, FFmpeg's version, where the sound goes, the windows and blocks, and the settings changed.

- Acts on the app.
- Hands back: What it copied, a line for each thing.

### Show/Hide All in Every Window

`app.toggle_ui`: Hide every panel over the video in every window while Settings show any, else show them all.

- Acts on the app.
- Tells: `app.settings.changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

### Show Controls in Every Window

`app.toggle_controls`: Show the seek bar and buttons at the bottom of every block in every window, and those opened later, or hide them, as Settings → Appearance does.

- Acts on the app.
- Tells: `app.settings.changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

### Show Top Bar in Every Window

`app.toggle_top_bar`: Show the file's name and buttons at the top of every block in every window, and those opened later, or hide them, as Settings → Appearance does.

- Acts on the app.
- Tells: `app.settings.changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

### Show Track Switcher in Every Window

`app.toggle_track_switcher`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over every block in every window, and those opened later, or switch without it, as Settings → Appearance does.

- Acts on the app.
- Tells: `app.settings.changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

### Show Notes in Every Window

`app.toggle_notes`: Show notes on what a shortcut changed, like the volume, over every block in every window, and those opened later, or not, as Settings → Appearance does. Notes on what's wrong still show.

- Acts on the app.
- Tells: `app.settings.changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

### Quit Plays

`app.quit`: Quit, keeping where each file got to and stopping the devices cast to.

- Acts on the app.
- Keys: ⌘Q
- Tells: `block.closed`, `window.closed`, `window.active_changed`, `app.quit`.

## window

### List Windows

`window.ls`: List the player windows, in the order ⌘1 to ⌘9 number them.

- Acts on the app.
- Hands back: [{id, label, title, number, front, blocks}]: each window's id, label (or null) and title, as its title bar shows it, its number from 1, whether it's in front, and how many blocks it has.

### Inspect Window

`window.inspect`: Say all about the window: where it is, how it shows and its blocks.

- Acts on a player window.
- Hands back: {id, label, title, number, front, blocks: [as block.ls], bounds: {x, y, width, height}, display, full_screen, on_top}: as window.ls says, its blocks as block.ls lists them, where it is in points, the id of the display it's on, and whether it's full screen and floats on top.

### New Window

`window.new`: Open an empty player window, in front, numbered after the others.

- Acts on the app.
- Keys: ⌘N
- Hands back: {window}: the new window's id.
- Tells: `window.created`, `window.active_changed`.

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

`window.go_to`: List the windows, their blocks, the files in the front block's folder and those played lately, to find one by name and go there: bring it to the front, play the file in the block, or open it.

- Acts on a player window.
- Keys: ⌘P

### Close Window

`window.close`: Close the window and the files in it; with none named, the window in front, Settings and About too.

- Acts on a window, Settings and About too.
- Keys: ⌥⌘W, ⌘W
- Tells: `block.closed`, `window.closed`, `window.active_changed`.

### Close Other Windows

`window.close_others`: Close every player window but this one, and the files in them.

- Acts on a player window.
- Tells: `block.closed`, `window.closed`.

### Rename Window…

`window.rename`: Label the window: its title shows the label, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a player window.
- Arguments:
  - `label` (text, optional): What to call it; no two windows are called the same.
- Tells: `window.label_changed`.

### Go to Window

`window.focus`: Bring the window to the front. ⌘1 to ⌘9 go to the first nine, by their numbers.

- Acts on a player window.
- Tells: `window.active_changed`.

### Next Window

`window.next`: Bring the window after the one in front to the front: the first after the last.

- Acts on the app.
- Keys: ⌃⇥
- Tells: `window.active_changed`.

### Previous Window

`window.previous`: Bring the window before the one in front to the front: the last before the first.

- Acts on the app.
- Keys: ⌃⇧⇥
- Tells: `window.active_changed`.

### Go Back One Window

`window.back`: Bring back the player window in front before this one; again, the one before that, and so on, until something else is done.

- Acts on the app.
- Tells: `window.active_changed`.

### Go to Recent Window

`window.recent`: Bring back the player window in front last; again, the one in front first.

- Acts on the app.
- Keys: ⌘'
- Tells: `window.active_changed`.

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
- Tells: `window.full_screen_changed`.

### Exit Full Screen

`window.exit_full_screen`: Show the window as it was before it was full screen.

- Acts on a player window.
- Keys: ⎋
- Tells: `window.full_screen_changed`.

### Match Video Size

`window.match_video_size`: Size the window for its videos to show at their own size, as far as the screen allows.

- Acts on a player window.
- Keys: ⌥⌘0

### Float on Top

`window.toggle_float_on_top`: Keep player windows above other apps' windows, or stop.

- Acts on the app.
- Tells: `app.settings.changed`.

### Float on Top While Playing

`window.toggle_float_on_top_while_playing`: Keep player windows above other apps' windows while a file in them plays, or stop.

- Acts on the app.
- Tells: `app.settings.changed`.

### Show on Display

`window.show_on_display`: Show the window full screen on a display, coming back where it was once it leaves full screen there.

- Acts on a player window.
- Arguments:
  - `display` (a whole number, at least 0): The display's id.
- Tells: `window.full_screen_changed`.

### Show/Hide All in Window

`window.toggle_ui`: Hide every panel over the video in each block in the window while the window shows any, else show them all.

- Acts on a player window.
- Tells: `window.ui_changed`, `block.ui_changed`.

### Show Controls in Window

`window.toggle_controls`: Show the seek bar and buttons at the bottom of each block in the window, and those opened in it, or hide them.

- Acts on a player window.
- Tells: `window.ui_changed`, `block.ui_changed`.

### Show Top Bar in Window

`window.toggle_top_bar`: Show the file's name and buttons at the top of each block in the window, and those opened in it, or hide them.

- Acts on a player window.
- Tells: `window.ui_changed`, `block.ui_changed`.

### Show Track Switcher in Window

`window.toggle_track_switcher`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over each block in the window, and those opened in it, or switch without it.

- Acts on a player window.
- Tells: `window.ui_changed`, `block.ui_changed`.

### Show Notes in Window

`window.toggle_notes`: Show notes on what a shortcut changed, like the volume, over each block in the window, and those opened in it, or not. Notes on what's wrong still show.

- Acts on a player window.
- Tells: `window.ui_changed`, `block.ui_changed`.

## block

### List Blocks

`block.ls`: List the blocks in the window, in the grid's order.

- Acts on a player window.
- Hands back: [{id, label, title, file, url, playing, number, front}]: each block's id and label (or null), its file's name, the file's path or the stream's URL (without a user, password or query; the other null), whether it plays, its place from 1, and whether keys go to it.

### Inspect Block

`block.inspect`: Say all about the block: what it plays, where it is, and how it's set to play.

- Acts on a block.
- Hands back: {as block.ls, window, window_title, position, duration, frame_rate, ended, volume, muted, looping, tracks: {video, audio, subtitles: [{number, label, language, title, codec, format, default, forced, hearing_impaired, file}]}, picked: {video, audio, subtitles}, delays: {audio, subtitles}, picture: {turns, mirrored, aspect_ratio, crop}, chapter, chapters: [{number, name, start, end}], casting: {id, kind, name} or null, output: {id, name} or null}: its window's id and title, times in seconds (duration null for a live stream), frames a second (null for sound alone), volume from 0 to 100, tracks and chapters numbered from 1, the tracks picked and the chapter by number or null, delays in seconds, quarter turns clockwise, and the output it plays through of its own.

### Go to Block

`block.focus`: Put the block in front, keys going to it, and bring its window to the front.

- Acts on a block.
- Tells: `window.active_changed`, `block.active_changed`.

### Close File

`block.close`: Close the block's file. The window stays, empty after the last.

- Acts on a block.
- Keys: ⌘W
- Tells: `block.closed`, `window.layout_changed`, `block.active_changed`.

### Close All Files

`block.close_all`: Close every file in the window. The window stays, empty.

- Acts on every block in a window.
- Keys: ⇧⌘W
- Tells: `block.closed`, `window.layout_changed`, `block.active_changed`.

### Close Other Files

`block.close_others`: Close every file in the window but the block's.

- Acts on a block.
- Tells: `block.closed`, `window.layout_changed`.

### Focus Block to the Left

`block.focus.left`: Send keys to the block left of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘←
- Tells: `block.active_changed`.

### Focus Block to the Right

`block.focus.right`: Send keys to the block right of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘→
- Tells: `block.active_changed`.

### Focus Block Above

`block.focus.up`: Send keys to the block above the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↑
- Tells: `block.active_changed`.

### Focus Block Below

`block.focus.down`: Send keys to the block below the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↓
- Tells: `block.active_changed`.

### Focus Previous Block

`block.focus.previous`: Send keys to the block before the one they go to, the last before the first.

- Acts on a player window.
- Keys: ⌘[
- Tells: `block.active_changed`.

### Focus Next Block

`block.focus.next`: Send keys to the block after the one they go to, the first after the last.

- Acts on a player window.
- Keys: ⌘]
- Tells: `block.active_changed`.

### Go Back One Block

`block.back`: Send keys to the block clicked or gone to before the one they go to; again, the one before that, and so on, until something else is done.

- Acts on a player window.
- Tells: `block.active_changed`.

### Go to Recent Block

`block.recent`: Send keys to the block clicked or gone to last but the one they go to; again, back to that one.

- Acts on a player window.
- Tells: `block.active_changed`.

### Move Block Left

`block.move.left`: Swap the block keys go to with the one to its left.

- Acts on a player window.
- Keys: ⌥⇧⌘←
- Tells: `window.layout_changed`.

### Move Block Right

`block.move.right`: Swap the block keys go to with the one to its right.

- Acts on a player window.
- Keys: ⌥⇧⌘→
- Tells: `window.layout_changed`.

### Move Block Up

`block.move.up`: Swap the block keys go to with the one above it.

- Acts on a player window.
- Keys: ⌥⇧⌘↑
- Tells: `window.layout_changed`.

### Move Block Down

`block.move.down`: Swap the block keys go to with the one below it.

- Acts on a player window.
- Keys: ⌥⇧⌘↓
- Tells: `window.layout_changed`.

### Maximize Block

`block.zoom`: Fill the window with the block, or show every block again; a window's only block fills the screen instead.

- Acts on a block.
- Keys: ⇧⌘↩
- Tells: `window.full_screen_changed`, `window.layout_changed`.

### Rename Block…

`block.rename`: Label the block: its header shows the label in place of the file's name, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a block.
- Arguments:
  - `label` (text, optional): What to call it; no two blocks are called the same, in whichever windows.
- Tells: `block.label_changed`.

### Move Block To

`block.move`: Put the block in another place in its window's grid, the blocks between moving over, as dragging it onto another does.

- Acts on a block.
- Arguments:
  - `position` (a whole number, at least 1): Where, from 1, the first block's place.
- Tells: `window.layout_changed`.

### Move Block to Window

`block.move_to_window`: Move the block to another window, last there, where it plays on.

- Acts on a block.
- Arguments:
  - `window` (text): The window's id, its label, or its number, from 1.
- Tells: `block.moved`, `window.layout_changed`, `block.active_changed`.

### Move Block to New Window

`block.move_to_new_window`: Move the block to a window of its own beside this one, where it plays on.

- Acts on a block.
- Hands back: {window}: the new window's id.
- Tells: `window.created`, `block.moved`, `window.layout_changed`, `block.active_changed`.

### Copy File Path

`block.copy_path`: Copy the path of the block's file, or the URL of its stream; hands back what was copied.

- Acts on a block.
- Keys: ⌥⌘C
- Hands back: The path or URL copied.

### Copy Current Time

`block.copy_time`: Copy where the block's file is, as its bar shows it, such as 1:23:45; hands back what was copied.

- Acts on a block.
- Hands back: The time copied, such as 1:23:45.

### Picture in Picture

`block.toggle_picture_in_picture`: Play the block in a small window floating above every app's, or in its block again.

- Acts on a block.
- Keys: ⌃⌘P
- Tells: `block.picture_in_picture_changed`.

### Show/Hide All

`block.toggle_ui`: Hide every panel over the block's video while any shows, else show them all.

- Acts on a block.
- Tells: `block.ui_changed`.

### Show Controls

`block.toggle_controls`: Show the seek bar and buttons at the bottom of the block, or hide them.

- Acts on a block.
- Tells: `block.ui_changed`.

### Show Top Bar

`block.toggle_top_bar`: Show the file's name and buttons at the top of the block, or hide them.

- Acts on a block.
- Tells: `block.ui_changed`.

### Show Track Switcher

`block.toggle_track_switcher`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over the block, or switch without it.

- Acts on a block.
- Tells: `block.ui_changed`.

### Show Notes

`block.toggle_notes`: Show notes on what a shortcut changed, like the volume, over the block, or not. Notes on what's wrong still show.

- Acts on a block.
- Tells: `block.ui_changed`.

### Show/Hide Console

`block.toggle_console`: Show what the block's file is and how it plays, over its picture, or hide it.

- Acts on a block.
- Keys: ~, `

## playback

### Play/Pause

`playback.play_pause`: Pause the block if it plays, else play it.

- Acts on a block.
- Keys: Space, P
- Tells: `playback.playing`, `playback.paused`.

### Play

`playback.play`: Play the block, unless it plays already; from the start if it played to its end.

- Acts on a block.
- Tells: `playback.playing`.

### Pause

`playback.pause`: Pause the block, unless it's paused already.

- Acts on a block.
- Tells: `playback.paused`.

### Seek To

`playback.seek`: Play from a time in the file, here or on the device it's cast to.

- Acts on a block.
- Arguments:
  - `time` (a number, at least 0): Seconds from the start; past the end is the end.
- Tells: `playback.seeked`, `playback.ended`, `playback.chapter_changed`.

### Seek Forward

`playback.seek_forward`: Skip ahead by the seek step set in Settings.

- Acts on a block.
- Keys: →
- Tells: `playback.seeked`, `playback.ended`, `playback.chapter_changed`.

### Seek Backward

`playback.seek_backward`: Skip back by the seek step set in Settings.

- Acts on a block.
- Keys: ←
- Tells: `playback.seeked`, `playback.chapter_changed`.

### Next Frame

`playback.next_frame`: Pause and show the next frame.

- Acts on a block.
- Keys: ⌥→, ., >
- Tells: `playback.paused`, `playback.seeked`.

### Previous Frame

`playback.previous_frame`: Pause and show the frame before.

- Acts on a block.
- Keys: ⌥←, ,, <
- Tells: `playback.paused`, `playback.seeked`.

### Next Chapter

`playback.next_chapter`: Play from the start of the next chapter.

- Acts on a block.
- Keys: ⇟
- Tells: `playback.seeked`, `playback.chapter_changed`.

### Previous Chapter

`playback.previous_chapter`: Play from the start of this chapter, or of the one before near its start.

- Acts on a block.
- Keys: ⇞
- Tells: `playback.seeked`, `playback.chapter_changed`.

### Go to Chapter

`playback.go_to_chapter`: Play from the start of a chapter.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1): Which chapter, from 1.
- Tells: `playback.seeked`, `playback.chapter_changed`.

### Restart

`playback.restart`: Play from the start.

- Acts on a block.
- Keys: ↖
- Tells: `playback.seeked`, `playback.chapter_changed`.

### Previous File

`playback.previous_file`: Play the file before this one in its playlist, or else its folder, instead.

- Acts on a block.
- Keys: ⌘←
- Tells: `playback.opened`, `playback.playing`.

### Next File

`playback.next_file`: Play the file after this one in its playlist, or else its folder, instead.

- Acts on a block.
- Keys: ⌘→
- Tells: `playback.opened`, `playback.playing`.

### Play File Instead

`playback.play_file`: Play another file in the block instead, one in its folder say, at the same volume, muted and looping as this one is, and playing if it is.

- Acts on a block.
- Arguments:
  - `path` (text): The file's full path.
- Tells: `playback.opened`, `playback.playing`.

### Loop

`playback.toggle_loop`: Play the file again from the start at its end, or stop at its end.

- Acts on a block.
- Keys: ⇧⌘L

### Play/Pause All

`playback.play_pause_all`: Pause every file in every window if any plays, else play them all.

- Acts on every block in every window.
- Keys: ⇧Space
- Tells: `playback.playing`, `playback.paused`.

### Play All

`playback.play_all`: Play every file in every window.

- Acts on every block in every window.
- Tells: `playback.playing`.

### Pause All

`playback.pause_all`: Pause every file in every window.

- Acts on every block in every window.
- Tells: `playback.paused`.

### Play Window

`playback.play_window`: Play every file in the window.

- Acts on every block in a window.
- Tells: `playback.playing`.

### Pause Window

`playback.pause_window`: Pause every file in the window.

- Acts on every block in a window.
- Tells: `playback.paused`.

### Restart All

`playback.restart_all`: Play every file in every window from the start.

- Acts on every block in every window.
- Keys: ⇧↖
- Tells: `playback.seeked`, `playback.chapter_changed`.

### Seek All Forward

`playback.seek_forward_all`: Skip every file in every window ahead by the seek step.

- Acts on every block in every window.
- Keys: ⇧→
- Tells: `playback.seeked`, `playback.ended`, `playback.chapter_changed`.

### Seek All Backward

`playback.seek_backward_all`: Skip every file in every window back by the seek step.

- Acts on every block in every window.
- Keys: ⇧←
- Tells: `playback.seeked`, `playback.chapter_changed`.

### Next Frame on All

`playback.next_frame_all`: Pause every file in every window and show its next frame.

- Acts on every block in every window.
- Keys: ⌥⇧→
- Tells: `playback.paused`, `playback.seeked`.

### Previous Frame on All

`playback.previous_frame_all`: Pause every file in every window and show the frame before.

- Acts on every block in every window.
- Keys: ⌥⇧←
- Tells: `playback.paused`, `playback.seeked`.

### Sync All to This Block

`playback.sync_all`: Bring every file in every window to where the block is, playing or paused as it is.

- Acts on a block.
- Keys: ⇧S
- Tells: `playback.seeked`, `playback.playing`, `playback.paused`.

## audio

### Volume Up

`audio.volume_up`: Turn the block's volume up, unmuting it.

- Acts on a block.
- Keys: ↑
- Tells: `audio.volume_changed`.

### Volume Down

`audio.volume_down`: Turn the block's volume down.

- Acts on a block.
- Keys: ↓
- Tells: `audio.volume_changed`.

### Set Volume

`audio.set_volume`: Set the block's volume, unmuting it unless it's none.

- Acts on a block.
- Arguments:
  - `volume` (a number from 0 to 100): How loud, from 0 to 100.
- Tells: `audio.volume_changed`.

### Mute/Unmute

`audio.toggle_mute`: Mute the block, or play its sound again.

- Acts on a block.
- Keys: ⌘/, M
- Tells: `audio.volume_changed`.

### Next Audio Track

`audio.next_track`: Play the file's next audio track, the first after the last.

- Acts on a block.
- Keys: ⌃⌘A
- Tells: `playback.track_changed`.

### Pick Audio Track

`audio.select_track`: Play one of the file's audio tracks.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Tells: `playback.track_changed`.

### Audio Earlier

`audio.earlier`: Play the block's sound a little earlier than its picture.

- Acts on a block.
- Keys: ⌃-

### Audio Later

`audio.later`: Play the block's sound a little later than its picture.

- Acts on a block.
- Keys: ⌃=

### Reset Audio Delay

`audio.reset_delay`: Play the block's sound with its picture again.

- Acts on a block.
- Keys: ⌃⌫

### Volume Up on All

`audio.volume_up_all`: Turn every file's volume in every window up, each from its own.

- Acts on every block in every window.
- Keys: ⇧↑
- Tells: `audio.volume_changed`.

### Volume Down on All

`audio.volume_down_all`: Turn every file's volume in every window down, each from its own.

- Acts on every block in every window.
- Keys: ⇧↓
- Tells: `audio.volume_changed`.

### Mute/Unmute All

`audio.toggle_mute_all`: Mute every file in every window if any plays its sound, else unmute them all.

- Acts on every block in every window.
- Keys: ⇧M
- Tells: `audio.volume_changed`.

## subtitles

### Next Subtitles

`subtitles.next_track`: Show the file's next subtitles, and none after the last.

- Acts on a block.
- Keys: ⌃⌘S
- Tells: `playback.track_changed`.

### Pick Subtitles

`subtitles.select_track`: Show one of the file's subtitles, or none.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1, optional): Which, from 1, as the tracks menu lists them; none for no subtitles.
- Tells: `playback.track_changed`.

### Show/Hide Subtitles

`subtitles.toggle`: Hide the subtitles showing, or show the ones hidden.

- Acts on a block.
- Keys: V
- Tells: `playback.track_changed`.

### Bigger Subtitles

`subtitles.bigger`: Make every file's subtitles bigger.

- Acts on a block.
- Keys: ⇧G
- Tells: `app.settings.changed`.

### Smaller Subtitles

`subtitles.smaller`: Make every file's subtitles smaller.

- Acts on a block.
- Keys: ⇧F
- Tells: `app.settings.changed`.

### Move Subtitles Up

`subtitles.up`: Show every file's subtitles higher up.

- Acts on a block.
- Keys: R
- Tells: `app.settings.changed`.

### Move Subtitles Down

`subtitles.down`: Show every file's subtitles lower down.

- Acts on a block.
- Keys: ⇧R, T
- Tells: `app.settings.changed`.

### Subtitles Earlier

`subtitles.earlier`: Show the block's subtitles a little earlier.

- Acts on a block.
- Keys: Z

### Subtitles Later

`subtitles.later`: Show the block's subtitles a little later.

- Acts on a block.
- Keys: X

### Reset Subtitle Delay

`subtitles.reset_delay`: Show the block's subtitles at their own times again.

- Acts on a block.
- Keys: ⇧⌫

## video

### Next Video Track

`video.next_track`: Show the file's next video track, the first after the last.

- Acts on a block.
- Keys: ⌃⌘V
- Tells: `playback.track_changed`.

### Pick Video Track

`video.select_track`: Show one of the file's video tracks.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Tells: `playback.track_changed`.

### Rotate Right

`video.rotate_right`: Turn the picture a quarter turn clockwise.

- Acts on a block.
- Keys: ⌘R

### Rotate Left

`video.rotate_left`: Turn the picture a quarter turn counterclockwise.

- Acts on a block.
- Keys: ⌘L

### Flip Horizontally

`video.flip_horizontally`: Mirror the picture left to right.

- Acts on a block.
- Keys: ⇧⌘H

### Flip Vertically

`video.flip_vertically`: Turn the picture upside down, mirrored.

- Acts on a block.
- Keys: ⇧⌘V

### Next Aspect Ratio

`video.next_aspect_ratio`: Show the picture at the next aspect ratio, then as the file is.

- Acts on a block.
- Keys: A

### Aspect Ratio

`video.set_aspect_ratio`: Show the picture at an aspect ratio, or as the file is.

- Acts on a block.
- Arguments:
  - `ratio` (one of 1:1, 4:3, 16:10, 16:9, 1.85:1, 2.35:1, optional): The aspect ratio; none for the file's own.

### Next Crop

`video.next_crop`: Crop the picture to the next shape, then not at all.

- Acts on a block.
- Keys: C

### Crop

`video.set_crop`: Crop the picture to a shape, or not at all.

- Acts on a block.
- Arguments:
  - `crop` (one of 1:1, 4:3, 16:9, 1.85:1, 2.39:1, optional): The shape; none for no crop.

### Reset Picture

`video.reset_picture`: Show the picture as the file is: not turned, flipped, reshaped or cropped.

- Acts on a block.
- Keys: ⌘0

### Take Screenshot

`video.take_screenshot`: Save the frame showing as a file, and say where it went.

- Acts on a block.
- Keys: S

## cast

### List Devices

`cast.ls`: List the TVs and speakers found on the network to play on, by how they're reached.

- Acts on the app.
- Hands back: {Chromecast: [{id, name, shows}], AirPlay: […], DLNA: […]}: each protocol's devices found, by name, shows being video for a TV and sound for a speaker; a protocol with none found isn't there.

### Play On

`cast.to`: Play the block on a TV or speaker found on the network, from where it is.

- Acts on a block.
- Arguments:
  - `device` (text): The device's id.
  - `kind` (text, optional): How it's reached, such as Chromecast, AirPlay or DLNA, for an id two share.
- Tells: `cast.started`.

### Play Here

`cast.stop`: Stop casting the block, and play it here from where the device got to.

- Acts on a block.
- Tells: `cast.ended`.

### Pair and Play On

`cast.pair`: Pair with a TV that shows a PIN to type, such as an Apple TV, then play the block on it.

- Acts on a block.
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
  - `id` (text, optional): The output's id; none for the system's default, or for a block, the app's output.
  - `name` (text, optional): What it's called, for an output not connected now, which is waited for.
- Tells: `output.route_changed`, `app.settings.changed`.

### Block Output Device

`output.set_block_device`: Play the block's sound through an output of its own, or through the app's again.

- Acts on a block.
- Arguments:
  - `id` (text, optional): The output's id; none for the system's default, or for a block, the app's output.
  - `name` (text, optional): What it's called, for an output not connected now, which is waited for.

## display

### List Displays

`display.ls`: List the displays connected, in the system's order.

- Acts on the app.
- Hands back: [{id, name, bounds: {x, y, width, height}}]: each display's id, as window.show_on_display and window.inspect have it, its name, and where it is, in points.

## app.settings

### Settings…

`app.settings.open`: Open the Settings window, or bring it to the front.

- Acts on the app.
- Keys: ⌘,

### List Settings

`app.settings.ls`: List every setting: its key, what Settings calls it and the pane it's on, what it takes, and its value.

- Acts on the app.
- Hands back: [{key, title, pane, kind, value, default}], in the order Settings has them: kind is toggle, choice (with choices: [{value, label}]), number (with min and max), font, folder, output, keys or languages (a list, by name or code, kept by code).

### Get Setting

`app.settings.get`: Read a setting by its key, such as subtitles.size.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
- Hands back: Its value, as the settings file keeps it.

### List Shortcuts

`app.settings.keymap`: List each command keys can run, with the keys that run it now, as Settings → Hotkeys has them.

- Acts on the app.
- Hands back: [{command, title, keys, shown}]: each command's id and title, its keys as the settings file keeps them, and as the menus show them; none for a command with no keys.

### Change Setting

`app.settings.set`: Change a setting by its key, to a value Settings offers, and save it.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
  - `value` (any value): What to set it to, as the settings file keeps it: true, 0.5, "dark".
- Hands back: Its value now.
- Tells: `app.settings.changed`, `app.settings.keymap_changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

### Reset Setting

`app.settings.reset`: Put a setting back as Plays starts it out, and save it.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
- Hands back: Its value now.
- Tells: `app.settings.changed`, `app.settings.keymap_changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

### Show Settings File

`app.settings.reveal_file`: Show the settings file where the system shows files, saving it first if it isn't there yet.

- Acts on the app.
- Hands back: The file's path.

### Reload Settings

`app.settings.reload`: Read the settings file again, after it was changed by hand.

- Acts on the app.
- Hands back: [key]: the settings the file changed.
- Tells: `app.settings.changed`, `app.settings.keymap_changed`, `app.ui_changed`, `window.ui_changed`, `block.ui_changed`.

## cli

### Install Command Line Tool

`cli.install`: Link plays into /usr/local/bin, so a terminal can run it: to open files, run commands and hear what changes.

- Acts on the app.
- Hands back: The link's path.

# Events

What changes in Plays, its windows and their blocks, by namespace: `plays events` prints each as it comes, and `plays wait` waits for one. Each is a JSON object, `{type, payload}`. The payload of one about a window starts with `window_id` and `window_title`, as its title bar shows it; of one about a block, with `block_id` and `block_title` too, as its header shows it. A listener hears the app's, one window's, or all of them. Readers ignore events and fields they don't know.

A cue is a block reaching a time or a frame: `block.<block>.position@120.5s`, or `…@345f`, the block by id, label or number. `plays wait` waits for one and `plays events --cue` hears them: each once, the cue its type, with `playback.position`'s payload.

## app

### app.command_run

A command is about to run: told before anything it changes.

- Heard by those listening to the app's events.
- Payload:
  - `command`: Its id.
  - `source`: Where it was run from: action (a key, a menu item, or a control that sends one), ui (a control that runs it itself), palette, or cli.
  - `args`, only sometimes: The arguments it was given, if any.
  - `target`, only sometimes: The window and the block it was aimed at, by name, if either was.
- Follows: every command, as it runs.

### app.client_connected

A client connected to the socket, as the command line does.

- Heard by those listening to the app's events.
- Payload:
  - `client_id`: A number for it, which its `app.client_left` has too.
- Follows: a client connecting.

### app.client_left

A client went from the socket.

- Heard by those listening to the app's events.
- Payload:
  - `client_id`: The number its `app.client_connected` had.
- Follows: a client going or being cut off, as when control from the command line is turned off.

### app.ui_changed

The panels shown over the video in every window, as Settings have them, changed.

- Heard by those listening to the app's events.
- Payload:
  - `visible`: Those shown, of `controls`, `top_bar`, `track_switcher` and `notes`.
- Follows: `app.toggle_ui`, `app.toggle_controls`, `app.toggle_top_bar`, `app.toggle_track_switcher`, `app.toggle_notes`, `app.settings.set`, `app.settings.reset`, `app.settings.reload`, or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

### app.quit

Plays is quitting: told last, after each window still open is told closing.

- Heard by every listener.
- Payload: none.
- Follows: `app.quit`, or Plays quitting from the Dock or with the system.

## window

### window.created

A player window opened.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `label`: Its label; none without one.
- Follows: `app.open`, `window.new`, or `block.move_to_new_window`.

### window.closed

A player window closed: told after each of its blocks is told closed.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
- Follows: `app.quit`, `window.close`, `window.close_others`, or Plays quitting.

### window.label_changed

A window was labeled, or its label taken away.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `label`: Its label; none once it's taken away.
- Follows: `window.rename`.

### window.active_changed

Another player window came to the front, or none is in front: its `window_id` and `window_title` are none then.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
- Follows: `app.open`, `app.quit`, `window.new`, `window.close`, `window.focus`, `window.next`, `window.previous`, `window.back`, `window.recent`, `block.focus`, or a click on another window.

### window.full_screen_changed

A window went full screen, or came back from it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `full_screen`: Whether it's full screen now.
- Follows: `window.toggle_full_screen`, `window.exit_full_screen`, `window.show_on_display`, `block.zoom`, or the window's own full screen button.

### window.ui_changed

The panels shown over the video in a window's blocks, and those opened in it, changed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `visible`: Those shown, of `controls`, `top_bar`, `track_switcher` and `notes`.
- Follows: `app.toggle_ui`, `app.toggle_controls`, `app.toggle_top_bar`, `app.toggle_track_switcher`, `app.toggle_notes`, `window.toggle_ui`, `window.toggle_controls`, `window.toggle_top_bar`, `window.toggle_track_switcher`, `window.toggle_notes`, `app.settings.set`, `app.settings.reset`, `app.settings.reload`, or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

### window.layout_changed

Blocks came into a window, went, or changed places, or one filled it or stopped filling it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `blocks`: The ids of its blocks, in the order its grid shows them.
  - `maximized`: The id of the block filling it; none if none does.
- Follows: `app.open`, `block.close`, `block.close_all`, `block.close_others`, `block.move.left`, `block.move.right`, `block.move.up`, `block.move.down`, `block.zoom`, `block.move`, `block.move_to_window`, `block.move_to_new_window`, or files dropped on a window or picked when `window.open_files` or `window.open_url` asks.

## block

### block.created

A block came into a window, for a file to play in.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `label`: Its label; none without one.
- Follows: `app.open`, or files dropped on a window or picked when `window.open_files` or `window.open_url` asks.

### block.moved

A block moved to another window: those listening to the window it left hear it too.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `from_window_id`: The id of the window it left.
  - `from_window_title`: That window's title.
- Follows: `block.move_to_window`, or `block.move_to_new_window`.

### block.closed

A block closed, and the file in it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Follows: `app.quit`, `window.close`, `window.close_others`, `block.close`, `block.close_all`, `block.close_others`, or Plays quitting.

### block.label_changed

A block was labeled, or its label taken away.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `label`: Its label; none once it's taken away.
- Follows: `block.rename`.

### block.active_changed

Keys go to another block in a window, or to none.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `block_id`: The block's id; none if keys go to none.
  - `block_title`: Its title, as its header shows it; none without a block.
- Follows: `app.open`, `block.focus`, `block.close`, `block.close_all`, `block.focus.left`, `block.focus.right`, `block.focus.up`, `block.focus.down`, `block.focus.previous`, `block.focus.next`, `block.back`, `block.recent`, `block.move_to_window`, `block.move_to_new_window`, or a click on a block or the mouse going to one.

### block.picture_in_picture_changed

A block began playing in a small window floating above every app's, or in its block again.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `picture_in_picture`: Whether it plays in the small window now.
- Follows: `block.toggle_picture_in_picture`, or the small window's own buttons.

### block.ui_changed

The panels shown over a block's video changed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `visible`: Those shown, of `controls`, `top_bar`, `track_switcher` and `notes`.
- Follows: `app.toggle_ui`, `app.toggle_controls`, `app.toggle_top_bar`, `app.toggle_track_switcher`, `app.toggle_notes`, `window.toggle_ui`, `window.toggle_controls`, `window.toggle_top_bar`, `window.toggle_track_switcher`, `window.toggle_notes`, `block.toggle_ui`, `block.toggle_controls`, `block.toggle_top_bar`, `block.toggle_track_switcher`, `block.toggle_notes`, `app.settings.set`, `app.settings.reset`, `app.settings.reload`, or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

## playback

### playback.opened

A file opened in a block, paused at its start.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `title`: What its header shows without a label: the file's name, or the stream's.
  - `file`: Its path; none for a stream.
  - `url`: The stream's URL; none for a file.
  - `duration`: How long it plays, in seconds; none for a live stream.
- Follows: `app.open`, `playback.previous_file`, `playback.next_file`, `playback.play_file`, files dropped on a window or picked when `window.open_files` or `window.open_url` asks, or a playlist going on to its next file, once one plays to its end.

### playback.open_failed

A file couldn't be opened.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `error`: Why.
- Follows: a file Plays can't play or a stream it can't reach.

### playback.failed

A file stopped playing on an error.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `error`: Why.
- Follows: a file that's damaged or a stream that went.

### playback.playing

A file began playing.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.play_pause`, `playback.play`, `playback.previous_file`, `playback.next_file`, `playback.play_file`, `playback.play_pause_all`, `playback.play_all`, `playback.play_window`, `playback.sync_all`, a file opening while Autoplay is on, or a playlist going on to its next file, once one plays to its end.

### playback.paused

A file paused.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.play_pause`, `playback.pause`, `playback.next_frame`, `playback.previous_frame`, `playback.play_pause_all`, `playback.pause_all`, `playback.pause_window`, `playback.next_frame_all`, `playback.previous_frame_all`, or `playback.sync_all`.

### playback.seeked

A file was sought to another time, here or on the device it's cast to: where it went.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.seek`, `playback.seek_forward`, `playback.seek_backward`, `playback.next_frame`, `playback.previous_frame`, `playback.next_chapter`, `playback.previous_chapter`, `playback.go_to_chapter`, `playback.restart`, `playback.restart_all`, `playback.seek_forward_all`, `playback.seek_backward_all`, `playback.next_frame_all`, `playback.previous_frame_all`, or `playback.sync_all`.

### playback.ended

A file played to its end, and stopped there.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Follows: `playback.seek`, `playback.seek_forward`, `playback.seek_forward_all`, or a file playing to its end.

### playback.position

Where a file is as it plays: told only to those who ask for positions, at most once a second, and only once it has moved.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
  - `duration`: How long it plays, in seconds; none for a live stream.
- Follows: a file playing.

### playback.chapter_changed

A file is in another chapter.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `chapter`: The chapter, from 1; none before the first.
- Follows: `playback.seek`, `playback.seek_forward`, `playback.seek_backward`, `playback.next_chapter`, `playback.previous_chapter`, `playback.go_to_chapter`, `playback.restart`, `playback.restart_all`, `playback.seek_forward_all`, `playback.seek_backward_all`, or a file playing into its next chapter.

### playback.track_changed

Another of a file's tracks plays, or none of its kind.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `kind`: The track's: video, audio or subtitles.
  - `track`: The one playing, from 1 among its kind's, as the tracks menu lists them; none for none.
- Follows: `audio.next_track`, `audio.select_track`, `subtitles.next_track`, `subtitles.select_track`, `subtitles.toggle`, `video.next_track`, or `video.select_track`.

## audio

### audio.volume_changed

A block's volume changed, or it was muted or unmuted.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `volume`: Its volume, in percent, as `audio.set_volume` takes it.
  - `muted`: Whether it's muted.
- Follows: `audio.volume_up`, `audio.volume_down`, `audio.set_volume`, `audio.toggle_mute`, `audio.volume_up_all`, `audio.volume_down_all`, or `audio.toggle_mute_all`.

## cast

### cast.devices_changed

TVs or speakers to cast to were found on the network, or went.

- Heard by those listening to the app's events.
- Payload:
  - `devices`: Those found now, as `cast.ls` lists them.
- Follows: devices found on the network or gone from it.

### cast.started

A block began playing on a TV or speaker.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `device`: The device: its id, kind and name.
- Follows: `cast.to`, or a TV paired with `cast.pair` once its PIN is typed.

### cast.ended

A block stopped playing on a TV or speaker.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `device`: The device it played on: its id, kind and name.
- Follows: `cast.stop`, or the device stopping or leaving the network.

## output

### output.devices_changed

Outputs were connected or disconnected.

- Heard by those listening to the app's events.
- Payload:
  - `outputs`: Those connected now, as `output.ls` lists them.
- Follows: an output connected or disconnected.

### output.route_changed

The app's sound goes elsewhere.

- Heard by those listening to the app's events.
- Payload:
  - `route`: Where it goes now, as `output.ls` says.
- Follows: `output.set_device`, or the system's own output changing.

## app.settings

### app.settings.changed

Settings changed.

- Heard by those listening to the app's events.
- Payload:
  - `keys`: The keys of those that changed, as `app.settings.get` takes them.
- Follows: `app.toggle_ui`, `app.toggle_controls`, `app.toggle_top_bar`, `app.toggle_track_switcher`, `app.toggle_notes`, `window.toggle_float_on_top`, `window.toggle_float_on_top_while_playing`, `subtitles.bigger`, `subtitles.smaller`, `subtitles.up`, `subtitles.down`, `output.set_device`, `app.settings.set`, `app.settings.reset`, `app.settings.reload`, or Settings moving a slider, importing a settings file or resetting every setting.

### app.settings.keymap_changed

The keys that run commands changed.

- Heard by those listening to the app's events.
- Payload:
  - `commands`: The ids of the commands whose keys changed.
- Follows: `app.settings.set`, `app.settings.reset`, `app.settings.reload`, or Settings importing a settings file or resetting every setting.
