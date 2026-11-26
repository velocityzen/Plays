# Commands and events

Every command Plays has and every event it tells, by what they're about, as `plays actions --markdown` writes them.

## Contents

- [Commands](#commands) and [Events](#events): running the one, hearing the other
- [Playback](#playback): [in a block](#playback-in-a-block), [in a window](#playback-in-a-window), [in every window](#playback-in-every-window), [events](#playback-events)
- [Audio](#audio): [in a block](#audio-in-a-block), [in a window](#audio-in-a-window), [in every window](#audio-in-every-window), [events](#audio-events)
- [Subtitles](#subtitles): [in a block](#subtitles-in-a-block), [in a window](#subtitles-in-a-window), [in every window](#subtitles-in-every-window), [events](#subtitles-events)
- [Video](#video): [in a block](#video-in-a-block), [in a window](#video-in-a-window), [in every window](#video-in-every-window), [events](#video-events)
- [Interface](#interface): [in a block](#interface-in-a-block), [in a window](#interface-in-a-window), [in every window](#interface-in-every-window), [events](#interface-events)
- [Blocks](#blocks): [commands](#block-commands), [events](#block-events)
- [Windows](#windows): [commands](#window-commands), [events](#window-events)
- [Casting](#casting): [commands](#casting-commands), [events](#casting-events)
- [Audio output](#audio-output): [commands](#audio-output-commands), [events](#audio-output-events)
- [Displays](#displays): [commands](#display-commands)
- [Settings](#settings): [commands](#settings-commands), [events](#settings-events)
- [App](#app): [commands](#app-commands), [events](#app-events)
- [Command line](#command-line): [commands](#command-line-commands)

## Commands

A terminal runs one with `plays do ID [KEY=VALUE…]`, naming the window or block it acts on with `-w` and `-b`; keys run those that have them, and Settings → Hotkeys changes those. The keys here are macOS's: elsewhere Ctrl stands for ⌘, and Alt for ⌥. A command for a window, or for every window, does to each block in it what the block's does, its title ending in Window, or in Every Window. Each lists its arguments, given as `KEY=VALUE`; what it returns, as `plays api call` prints it; and the events it causes.

## Events

What changes in Plays, its windows and their blocks: `plays events` prints each as it comes, and `plays wait` waits for one. Each is a JSON object, `{type, payload}`. The payload of one about a window starts with `window_id` and `window_title`, as its title bar shows it; of one about a block, with `block_id` and `block_title` too, as its header shows it. A listener hears the app's, one window's, or all of them. Readers ignore events and fields they don't know. An event is heard, not run: it has a payload, not arguments or a return value. Each lists who hears it, its payload, and what causes it.

A cue is a block reaching a time or a frame: `block.<block>.position@120.5s`, or `…@345f`, the block by id, label or number. `plays wait` waits for one and `plays events --cue` hears them: each once, the cue its type, with `block.playback.position`'s payload.

## Playback

### Playback in a block

#### Play/Pause

`block.playback.play_pause`: Pause the block if it plays, else play it.

- Acts on a block.
- Keys: Space, P
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.playing`](#blockplaybackplaying), [`block.playback.paused`](#blockplaybackpaused).

#### Play

`block.playback.play`: Play the block, unless it plays already; from the start if it played to its end.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.playing`](#blockplaybackplaying).

#### Pause

`block.playback.pause`: Pause the block, unless it's paused already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused).

#### Seek To

`block.playback.seek`: Play from a time in the file, here or on the device it's cast to.

- Acts on a block.
- Arguments:
  - `time` (a number, at least 0): Seconds from the start; past the end is the end.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.ended`](#blockplaybackended), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Seek Forward

`block.playback.seek_forward`: Skip ahead by the seek step set in Settings.

- Acts on a block.
- Keys: →
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.ended`](#blockplaybackended), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Seek Backward

`block.playback.seek_backward`: Skip back by the seek step set in Settings.

- Acts on a block.
- Keys: ←
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Next Frame

`block.playback.next_frame`: Pause and show the next frame.

- Acts on a block.
- Keys: ⌥→, ., >
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused), [`block.playback.seeked`](#blockplaybackseeked).

#### Previous Frame

`block.playback.previous_frame`: Pause and show the frame before.

- Acts on a block.
- Keys: ⌥←, ,, <
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused), [`block.playback.seeked`](#blockplaybackseeked).

#### Next Chapter

`block.playback.next_chapter`: Play from the start of the next chapter.

- Acts on a block.
- Keys: ⇟
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Previous Chapter

`block.playback.previous_chapter`: Play from the start of this chapter, or of the one before near its start.

- Acts on a block.
- Keys: ⇞
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Go to Chapter

`block.playback.go_to_chapter`: Play from the start of a chapter.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1): Which chapter, from 1.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Restart

`block.playback.restart`: Play from the start.

- Acts on a block.
- Keys: ↖
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Previous File

`block.playback.previous_file`: Play the file before this one in its playlist, or else its folder, instead.

- Acts on a block.
- Keys: ⌘←
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Next File

`block.playback.next_file`: Play the file after this one in its playlist, or else its folder, instead.

- Acts on a block.
- Keys: ⌘→
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Play File Instead

`block.playback.play_file`: Play another file in the block instead, one in its folder say, at the same volume, muted and looping as this one is, and playing if it is.

- Acts on a block.
- Arguments:
  - `path` (text): The file's full path.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Loop

`block.playback.loop.toggle`: Play the file again from the start at its end, or stop at its end.

- Acts on a block.
- Keys: ⇧⌘L
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.on`](#blockplaybackloopon), [`block.playback.loop.off`](#blockplaybackloopoff).

#### Start Looping

`block.playback.loop.on`: Play the file again from the start at its end, unless it does already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.on`](#blockplaybackloopon).

#### Stop Looping

`block.playback.loop.off`: Stop the file at its end, unless it does already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.off`](#blockplaybackloopoff).

#### Sync All to This Block

`block.playback.sync_all`: Bring every file in every window to where the block is, playing or paused as it is.

- Acts on a block.
- Keys: ⇧S
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.playing`](#blockplaybackplaying), [`block.playback.paused`](#blockplaybackpaused).

### Playback in a window

#### Play/Pause in Window

`window.playback.play_pause`: Pause every file in the window if any plays, else play them all.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.playing`](#blockplaybackplaying), [`block.playback.paused`](#blockplaybackpaused).

#### Play in Window

`window.playback.play`: Play every file in the window.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.playing`](#blockplaybackplaying).

#### Pause in Window

`window.playback.pause`: Pause every file in the window.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused).

#### Seek To in Window

`window.playback.seek`: Play every file in the window from a time in it.

- Acts on every block in a window.
- Arguments:
  - `time` (a number, at least 0): Seconds from the start; past the end is the end.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.ended`](#blockplaybackended), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Seek Forward in Window

`window.playback.seek_forward`: Skip every file in the window ahead by the seek step.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.ended`](#blockplaybackended), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Seek Backward in Window

`window.playback.seek_backward`: Skip every file in the window back by the seek step.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Next Frame in Window

`window.playback.next_frame`: Pause every file in the window and show its next frame.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused), [`block.playback.seeked`](#blockplaybackseeked).

#### Previous Frame in Window

`window.playback.previous_frame`: Pause every file in the window and show the frame before.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused), [`block.playback.seeked`](#blockplaybackseeked).

#### Next Chapter in Window

`window.playback.next_chapter`: Play every file in the window from the start of its next chapter.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Previous Chapter in Window

`window.playback.previous_chapter`: Play every file in the window from the start of its chapter, or of the one before near its start.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Go to Chapter in Window

`window.playback.go_to_chapter`: Play every file in the window that has a chapter from its start.

- Acts on every block in a window.
- Arguments:
  - `number` (a whole number, at least 1): Which chapter, from 1.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Restart in Window

`window.playback.restart`: Play every file in the window from the start.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Previous File in Window

`window.playback.previous_file`: Play the file before each of every file in the window in its playlist, or else its folder, instead.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Next File in Window

`window.playback.next_file`: Play the file after each of every file in the window in its playlist, or else its folder, instead.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Play File Instead in Window

`window.playback.play_file`: Play another file instead of every file in the window, each at its volume, muted and looping as it is, and playing if it is.

- Acts on every block in a window.
- Arguments:
  - `path` (text): The file's full path.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Loop in Window

`window.playback.loop.toggle`: Play every file in the window again from the start at its end, unless all do already; else stop them all at their end.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.on`](#blockplaybackloopon), [`block.playback.loop.off`](#blockplaybackloopoff).

#### Start Looping in Window

`window.playback.loop.on`: Play every file in the window again from the start at its end.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.on`](#blockplaybackloopon).

#### Stop Looping in Window

`window.playback.loop.off`: Stop every file in the window at its end.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.off`](#blockplaybackloopoff).

### Playback in every window

#### Play/Pause in Every Window

`app.playback.play_pause`: Pause every file in every window if any plays, else play them all.

- Acts on every block in every window.
- Keys: ⇧Space
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.playing`](#blockplaybackplaying), [`block.playback.paused`](#blockplaybackpaused).

#### Play in Every Window

`app.playback.play`: Play every file in every window.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.playing`](#blockplaybackplaying).

#### Pause in Every Window

`app.playback.pause`: Pause every file in every window.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused).

#### Seek To in Every Window

`app.playback.seek`: Play every file in every window from a time in it.

- Acts on every block in every window.
- Arguments:
  - `time` (a number, at least 0): Seconds from the start; past the end is the end.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.ended`](#blockplaybackended), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Seek Forward in Every Window

`app.playback.seek_forward`: Skip every file in every window ahead by the seek step.

- Acts on every block in every window.
- Keys: ⇧→
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.ended`](#blockplaybackended), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Seek Backward in Every Window

`app.playback.seek_backward`: Skip every file in every window back by the seek step.

- Acts on every block in every window.
- Keys: ⇧←
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Next Frame in Every Window

`app.playback.next_frame`: Pause every file in every window and show its next frame.

- Acts on every block in every window.
- Keys: ⌥⇧→
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused), [`block.playback.seeked`](#blockplaybackseeked).

#### Previous Frame in Every Window

`app.playback.previous_frame`: Pause every file in every window and show the frame before.

- Acts on every block in every window.
- Keys: ⌥⇧←
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.paused`](#blockplaybackpaused), [`block.playback.seeked`](#blockplaybackseeked).

#### Next Chapter in Every Window

`app.playback.next_chapter`: Play every file in every window from the start of its next chapter.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Previous Chapter in Every Window

`app.playback.previous_chapter`: Play every file in every window from the start of its chapter, or of the one before near its start.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Go to Chapter in Every Window

`app.playback.go_to_chapter`: Play every file in every window that has a chapter from its start.

- Acts on every block in every window.
- Arguments:
  - `number` (a whole number, at least 1): Which chapter, from 1.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Restart in Every Window

`app.playback.restart`: Play every file in every window from the start.

- Acts on every block in every window.
- Keys: ⇧↖
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.seeked`](#blockplaybackseeked), [`block.playback.chapter_changed`](#blockplaybackchapter_changed).

#### Previous File in Every Window

`app.playback.previous_file`: Play the file before each of every file in every window in its playlist, or else its folder, instead.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Next File in Every Window

`app.playback.next_file`: Play the file after each of every file in every window in its playlist, or else its folder, instead.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Play File Instead in Every Window

`app.playback.play_file`: Play another file instead of every file in every window, each at its volume, muted and looping as it is, and playing if it is.

- Acts on every block in every window.
- Arguments:
  - `path` (text): The file's full path.
- Returns: nothing.
- Events: [`block.playback.opened`](#blockplaybackopened), [`block.playback.playing`](#blockplaybackplaying).

#### Loop in Every Window

`app.playback.loop.toggle`: Play every file in every window again from the start at its end, unless all do already; else stop them all at their end.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.on`](#blockplaybackloopon), [`block.playback.loop.off`](#blockplaybackloopoff).

#### Start Looping in Every Window

`app.playback.loop.on`: Play every file in every window again from the start at its end.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.on`](#blockplaybackloopon).

#### Stop Looping in Every Window

`app.playback.loop.off`: Stop every file in every window at its end.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.loop.off`](#blockplaybackloopoff).

### Playback events

#### block.playback.loop.on

A block's file is set to play again from the start at its end.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.playback.loop.toggle`](#loop), [`block.playback.loop.on`](#blockplaybackloopon), [`window.playback.loop.toggle`](#loop-in-window), [`window.playback.loop.on`](#start-looping-in-window), [`app.playback.loop.toggle`](#loop-in-every-window), or [`app.playback.loop.on`](#start-looping-in-every-window).

#### block.playback.loop.off

A block's file is set to stop at its end.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.playback.loop.toggle`](#loop), [`block.playback.loop.off`](#blockplaybackloopoff), [`window.playback.loop.toggle`](#loop-in-window), [`window.playback.loop.off`](#stop-looping-in-window), [`app.playback.loop.toggle`](#loop-in-every-window), or [`app.playback.loop.off`](#stop-looping-in-every-window).

#### block.playback.opened

A file opened in a block, paused at its start.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `title`: What its header shows without a label: the file's name, or the stream's.
  - `file`: Its path; none for a stream.
  - `url`: The stream's URL; none for a file.
  - `duration`: How long it plays, in seconds; none for a live stream.
- Caused by: [`app.open`](#open-paths-or-urls), [`block.playback.previous_file`](#previous-file), [`block.playback.next_file`](#next-file), [`block.playback.play_file`](#play-file-instead), [`window.playback.previous_file`](#previous-file-in-window), [`window.playback.next_file`](#next-file-in-window), [`window.playback.play_file`](#play-file-instead-in-window), [`app.playback.previous_file`](#previous-file-in-every-window), [`app.playback.next_file`](#next-file-in-every-window), [`app.playback.play_file`](#play-file-instead-in-every-window), files dropped on a window or picked when `window.open_files` or `window.open_url` asks, or a playlist going on to its next file, once one plays to its end.

#### block.playback.open_failed

A file couldn't be opened.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `error`: Why.
- Caused by: a file Plays can't play or a stream it can't reach.

#### block.playback.failed

A file stopped playing on an error.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `error`: Why.
- Caused by: a file that's damaged or a stream that went.

#### block.playback.playing

A file began playing: told once it moves, not as it's set to play while what it plays from isn't there yet, as just after it opens or a seek.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Caused by: [`block.playback.play_pause`](#playpause), [`block.playback.play`](#play), [`block.playback.previous_file`](#previous-file), [`block.playback.next_file`](#next-file), [`block.playback.play_file`](#play-file-instead), [`block.playback.sync_all`](#sync-all-to-this-block), [`window.playback.play_pause`](#playpause-in-window), [`window.playback.play`](#play-in-window), [`window.playback.previous_file`](#previous-file-in-window), [`window.playback.next_file`](#next-file-in-window), [`window.playback.play_file`](#play-file-instead-in-window), [`app.playback.play_pause`](#playpause-in-every-window), [`app.playback.play`](#play-in-every-window), [`app.playback.previous_file`](#previous-file-in-every-window), [`app.playback.next_file`](#next-file-in-every-window), [`app.playback.play_file`](#play-file-instead-in-every-window), a file opening while Autoplay is on, or a playlist going on to its next file, once one plays to its end.

#### block.playback.paused

A file paused.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Caused by: [`block.playback.play_pause`](#playpause), [`block.playback.pause`](#pause), [`block.playback.next_frame`](#next-frame), [`block.playback.previous_frame`](#previous-frame), [`block.playback.sync_all`](#sync-all-to-this-block), [`window.playback.play_pause`](#playpause-in-window), [`window.playback.pause`](#pause-in-window), [`window.playback.next_frame`](#next-frame-in-window), [`window.playback.previous_frame`](#previous-frame-in-window), [`app.playback.play_pause`](#playpause-in-every-window), [`app.playback.pause`](#pause-in-every-window), [`app.playback.next_frame`](#next-frame-in-every-window), or [`app.playback.previous_frame`](#previous-frame-in-every-window).

#### block.playback.seeked

A file was sought to another time, here or on the device it's cast to: where it went.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Caused by: [`block.playback.seek`](#seek-to), [`block.playback.seek_forward`](#seek-forward), [`block.playback.seek_backward`](#seek-backward), [`block.playback.next_frame`](#next-frame), [`block.playback.previous_frame`](#previous-frame), [`block.playback.next_chapter`](#next-chapter), [`block.playback.previous_chapter`](#previous-chapter), [`block.playback.go_to_chapter`](#go-to-chapter), [`block.playback.restart`](#restart), [`block.playback.sync_all`](#sync-all-to-this-block), [`window.playback.seek`](#seek-to-in-window), [`window.playback.seek_forward`](#seek-forward-in-window), [`window.playback.seek_backward`](#seek-backward-in-window), [`window.playback.next_frame`](#next-frame-in-window), [`window.playback.previous_frame`](#previous-frame-in-window), [`window.playback.next_chapter`](#next-chapter-in-window), [`window.playback.previous_chapter`](#previous-chapter-in-window), [`window.playback.go_to_chapter`](#go-to-chapter-in-window), [`window.playback.restart`](#restart-in-window), [`app.playback.seek`](#seek-to-in-every-window), [`app.playback.seek_forward`](#seek-forward-in-every-window), [`app.playback.seek_backward`](#seek-backward-in-every-window), [`app.playback.next_frame`](#next-frame-in-every-window), [`app.playback.previous_frame`](#previous-frame-in-every-window), [`app.playback.next_chapter`](#next-chapter-in-every-window), [`app.playback.previous_chapter`](#previous-chapter-in-every-window), [`app.playback.go_to_chapter`](#go-to-chapter-in-every-window), or [`app.playback.restart`](#restart-in-every-window).

#### block.playback.ended

A file played to its end, and stopped there.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
- Caused by: [`block.playback.seek`](#seek-to), [`block.playback.seek_forward`](#seek-forward), [`window.playback.seek`](#seek-to-in-window), [`window.playback.seek_forward`](#seek-forward-in-window), [`app.playback.seek`](#seek-to-in-every-window), [`app.playback.seek_forward`](#seek-forward-in-every-window), or a file playing to its end.

#### block.playback.position

Where a file is as it plays: told only to those who ask for positions, at most once a second, and only once it has moved.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `position`: How far through the file it is, from 0 to 1; none while its duration isn't known, as a live stream's.
  - `time`: Where it is, in seconds.
  - `frame`: The number of the frame showing then, from 0; none for sound alone.
  - `duration`: How long it plays, in seconds; none for a live stream.
- Caused by: a file playing.

#### block.playback.chapter_changed

A file is in another chapter.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `chapter`: The chapter, from 1; none before the first.
- Caused by: [`block.playback.seek`](#seek-to), [`block.playback.seek_forward`](#seek-forward), [`block.playback.seek_backward`](#seek-backward), [`block.playback.next_chapter`](#next-chapter), [`block.playback.previous_chapter`](#previous-chapter), [`block.playback.go_to_chapter`](#go-to-chapter), [`block.playback.restart`](#restart), [`window.playback.seek`](#seek-to-in-window), [`window.playback.seek_forward`](#seek-forward-in-window), [`window.playback.seek_backward`](#seek-backward-in-window), [`window.playback.next_chapter`](#next-chapter-in-window), [`window.playback.previous_chapter`](#previous-chapter-in-window), [`window.playback.go_to_chapter`](#go-to-chapter-in-window), [`window.playback.restart`](#restart-in-window), [`app.playback.seek`](#seek-to-in-every-window), [`app.playback.seek_forward`](#seek-forward-in-every-window), [`app.playback.seek_backward`](#seek-backward-in-every-window), [`app.playback.next_chapter`](#next-chapter-in-every-window), [`app.playback.previous_chapter`](#previous-chapter-in-every-window), [`app.playback.go_to_chapter`](#go-to-chapter-in-every-window), [`app.playback.restart`](#restart-in-every-window), or a file playing into its next chapter.

#### block.playback.track_changed

Another of a file's tracks plays, or none of its kind.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `kind`: The track's: video, audio or subtitles.
  - `track`: The one playing, from 1 among its kind's, as the tracks menu lists them; none for none.
- Caused by: [`block.audio.next_track`](#next-audio-track), [`block.audio.select_track`](#pick-audio-track), [`window.audio.next_track`](#next-audio-track-in-window), [`window.audio.select_track`](#pick-audio-track-in-window), [`app.audio.next_track`](#next-audio-track-in-every-window), [`app.audio.select_track`](#pick-audio-track-in-every-window), [`block.subtitles.next_track`](#next-subtitles), [`block.subtitles.select_track`](#pick-subtitles), [`block.subtitles.toggle`](#showhide-subtitles), [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff), [`window.subtitles.next_track`](#next-subtitles-in-window), [`window.subtitles.select_track`](#pick-subtitles-in-window), [`window.subtitles.toggle`](#showhide-subtitles-in-window), [`window.subtitles.on`](#show-subtitles-in-window), [`window.subtitles.off`](#hide-subtitles-in-window), [`app.subtitles.next_track`](#next-subtitles-in-every-window), [`app.subtitles.select_track`](#pick-subtitles-in-every-window), [`app.subtitles.toggle`](#showhide-subtitles-in-every-window), [`app.subtitles.on`](#show-subtitles-in-every-window), [`app.subtitles.off`](#hide-subtitles-in-every-window), [`block.video.next_track`](#next-video-track), [`block.video.select_track`](#pick-video-track), [`window.video.next_track`](#next-video-track-in-window), [`window.video.select_track`](#pick-video-track-in-window), [`app.video.next_track`](#next-video-track-in-every-window), or [`app.video.select_track`](#pick-video-track-in-every-window).

## Audio

### Audio in a block

#### Volume Up

`block.audio.volume_up`: Turn the block's volume up, unmuting it.

- Acts on a block.
- Keys: ↑
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Volume Down

`block.audio.volume_down`: Turn the block's volume down.

- Acts on a block.
- Keys: ↓
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Set Volume

`block.audio.set_volume`: Set the block's volume, unmuting it unless it's none.

- Acts on a block.
- Arguments:
  - `volume` (a number from 0 to 100): How loud, from 0 to 100.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Mute/Unmute

`block.audio.mute.toggle`: Mute the block, or play its sound again.

- Acts on a block.
- Keys: ⌘/, M
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.on`](#blockaudiomuteon), [`block.audio.mute.off`](#blockaudiomuteoff).

#### Mute

`block.audio.mute.on`: Mute the block, unless it's muted already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.on`](#blockaudiomuteon).

#### Unmute

`block.audio.mute.off`: Play the block's sound again, unless it plays already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.off`](#blockaudiomuteoff).

#### Next Audio Track

`block.audio.next_track`: Play the file's next audio track, the first after the last.

- Acts on a block.
- Keys: ⌃⌘A
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Pick Audio Track

`block.audio.select_track`: Play one of the file's audio tracks.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Audio Earlier

`block.audio.earlier`: Play the block's sound a little earlier than its picture.

- Acts on a block.
- Keys: ⌃-
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Audio Later

`block.audio.later`: Play the block's sound a little later than its picture.

- Acts on a block.
- Keys: ⌃=
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Reset Audio Delay

`block.audio.reset_delay`: Play the block's sound with its picture again.

- Acts on a block.
- Keys: ⌃⌫
- Arguments: none.
- Returns: nothing.
- Events: none.

### Audio in a window

#### Volume Up in Window

`window.audio.volume_up`: Turn every file in the window up, each from its own volume, unmuting it.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Volume Down in Window

`window.audio.volume_down`: Turn every file in the window down, each from its own volume.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Set Volume in Window

`window.audio.set_volume`: Set the volume of every file in the window, unmuting them unless it's none.

- Acts on every block in a window.
- Arguments:
  - `volume` (a number from 0 to 100): How loud, from 0 to 100.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Mute/Unmute in Window

`window.audio.mute.toggle`: Mute every file in the window if any plays its sound, else unmute them all.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.on`](#blockaudiomuteon), [`block.audio.mute.off`](#blockaudiomuteoff).

#### Mute in Window

`window.audio.mute.on`: Mute every file in the window.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.on`](#blockaudiomuteon).

#### Unmute in Window

`window.audio.mute.off`: Play the sound of every file in the window again.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.off`](#blockaudiomuteoff).

#### Next Audio Track in Window

`window.audio.next_track`: Play the next audio track of every file in the window, the first after the last.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Pick Audio Track in Window

`window.audio.select_track`: Play an audio track of every file in the window that has it.

- Acts on every block in a window.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Audio Earlier in Window

`window.audio.earlier`: Play the sound of every file in the window a little earlier than its picture.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Audio Later in Window

`window.audio.later`: Play the sound of every file in the window a little later than its picture.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Reset Audio Delay in Window

`window.audio.reset_delay`: Play the sound of every file in the window with its picture again.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

### Audio in every window

#### Volume Up in Every Window

`app.audio.volume_up`: Turn every file in every window up, each from its own volume, unmuting it.

- Acts on every block in every window.
- Keys: ⇧↑
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Volume Down in Every Window

`app.audio.volume_down`: Turn every file in every window down, each from its own volume.

- Acts on every block in every window.
- Keys: ⇧↓
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Set Volume in Every Window

`app.audio.set_volume`: Set the volume of every file in every window, unmuting them unless it's none.

- Acts on every block in every window.
- Arguments:
  - `volume` (a number from 0 to 100): How loud, from 0 to 100.
- Returns: nothing.
- Events: [`block.audio.volume_changed`](#blockaudiovolume_changed).

#### Mute/Unmute in Every Window

`app.audio.mute.toggle`: Mute every file in every window if any plays its sound, else unmute them all.

- Acts on every block in every window.
- Keys: ⇧M
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.on`](#blockaudiomuteon), [`block.audio.mute.off`](#blockaudiomuteoff).

#### Mute in Every Window

`app.audio.mute.on`: Mute every file in every window.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.on`](#blockaudiomuteon).

#### Unmute in Every Window

`app.audio.mute.off`: Play the sound of every file in every window again.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.audio.mute.off`](#blockaudiomuteoff).

#### Next Audio Track in Every Window

`app.audio.next_track`: Play the next audio track of every file in every window, the first after the last.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Pick Audio Track in Every Window

`app.audio.select_track`: Play an audio track of every file in every window that has it.

- Acts on every block in every window.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Audio Earlier in Every Window

`app.audio.earlier`: Play the sound of every file in every window a little earlier than its picture.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Audio Later in Every Window

`app.audio.later`: Play the sound of every file in every window a little later than its picture.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Reset Audio Delay in Every Window

`app.audio.reset_delay`: Play the sound of every file in every window with its picture again.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

### Audio events

#### block.audio.volume_changed

A block's volume changed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `volume`: Its volume, in percent, as `block.audio.set_volume` takes it.
- Caused by: [`block.audio.volume_up`](#volume-up), [`block.audio.volume_down`](#volume-down), [`block.audio.set_volume`](#set-volume), [`window.audio.volume_up`](#volume-up-in-window), [`window.audio.volume_down`](#volume-down-in-window), [`window.audio.set_volume`](#set-volume-in-window), [`app.audio.volume_up`](#volume-up-in-every-window), [`app.audio.volume_down`](#volume-down-in-every-window), or [`app.audio.set_volume`](#set-volume-in-every-window).

#### block.audio.mute.on

A block was muted.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.audio.mute.toggle`](#muteunmute), [`block.audio.mute.on`](#blockaudiomuteon), [`window.audio.mute.toggle`](#muteunmute-in-window), [`window.audio.mute.on`](#mute-in-window), [`app.audio.mute.toggle`](#muteunmute-in-every-window), or [`app.audio.mute.on`](#mute-in-every-window).

#### block.audio.mute.off

A block was unmuted.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.audio.mute.toggle`](#muteunmute), [`block.audio.mute.off`](#blockaudiomuteoff), [`window.audio.mute.toggle`](#muteunmute-in-window), [`window.audio.mute.off`](#unmute-in-window), [`app.audio.mute.toggle`](#muteunmute-in-every-window), [`app.audio.mute.off`](#unmute-in-every-window), or its volume turned up.

## Subtitles

### Subtitles in a block

#### Next Subtitles

`block.subtitles.next_track`: Show the file's next subtitles, and none after the last.

- Acts on a block.
- Keys: ⌃⌘S
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed), [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff).

#### Pick Subtitles

`block.subtitles.select_track`: Show one of the file's subtitles, or none.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1, optional): Which, from 1, as the tracks menu lists them; none for no subtitles.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed), [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff).

#### Show/Hide Subtitles

`block.subtitles.toggle`: Hide the subtitles showing, or show the ones hidden.

- Acts on a block.
- Keys: V
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Show Subtitles

`block.subtitles.on`: Show the subtitles hidden, or the file's first, unless some show already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.on`](#blocksubtitleson), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Hide Subtitles

`block.subtitles.off`: Hide the subtitles showing, unless none show.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.off`](#blocksubtitlesoff), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Bigger Subtitles

`block.subtitles.bigger`: Make every file's subtitles bigger.

- Acts on a block.
- Keys: ⇧G
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged).

#### Smaller Subtitles

`block.subtitles.smaller`: Make every file's subtitles smaller.

- Acts on a block.
- Keys: ⇧F
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged).

#### Move Subtitles Up

`block.subtitles.up`: Show every file's subtitles higher up.

- Acts on a block.
- Keys: R
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged).

#### Move Subtitles Down

`block.subtitles.down`: Show every file's subtitles lower down.

- Acts on a block.
- Keys: ⇧R, T
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged).

#### Subtitles Earlier

`block.subtitles.earlier`: Show the block's subtitles a little earlier.

- Acts on a block.
- Keys: Z
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Subtitles Later

`block.subtitles.later`: Show the block's subtitles a little later.

- Acts on a block.
- Keys: X
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Reset Subtitle Delay

`block.subtitles.reset_delay`: Show the block's subtitles at their own times again.

- Acts on a block.
- Keys: ⇧⌫
- Arguments: none.
- Returns: nothing.
- Events: none.

### Subtitles in a window

#### Next Subtitles in Window

`window.subtitles.next_track`: Show the next subtitles of every file in the window, and none after the last.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed), [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff).

#### Pick Subtitles in Window

`window.subtitles.select_track`: Show subtitles of every file in the window that has them, or none.

- Acts on every block in a window.
- Arguments:
  - `number` (a whole number, at least 1, optional): Which, from 1, as the tracks menu lists them; none for no subtitles.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed), [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff).

#### Show/Hide Subtitles in Window

`window.subtitles.toggle`: Hide the subtitles of every file in the window if each that has some shows them, else show them in all.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Show Subtitles in Window

`window.subtitles.on`: Show the subtitles hidden, or the first, of every file in the window.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.on`](#blocksubtitleson), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Hide Subtitles in Window

`window.subtitles.off`: Hide the subtitles of every file in the window.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.off`](#blocksubtitlesoff), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Subtitles Earlier in Window

`window.subtitles.earlier`: Show the subtitles of every file in the window a little earlier.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Subtitles Later in Window

`window.subtitles.later`: Show the subtitles of every file in the window a little later.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Reset Subtitle Delay in Window

`window.subtitles.reset_delay`: Show the subtitles of every file in the window at their own times again.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

### Subtitles in every window

#### Next Subtitles in Every Window

`app.subtitles.next_track`: Show the next subtitles of every file in every window, and none after the last.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed), [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff).

#### Pick Subtitles in Every Window

`app.subtitles.select_track`: Show subtitles of every file in every window that has them, or none.

- Acts on every block in every window.
- Arguments:
  - `number` (a whole number, at least 1, optional): Which, from 1, as the tracks menu lists them; none for no subtitles.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed), [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff).

#### Show/Hide Subtitles in Every Window

`app.subtitles.toggle`: Hide the subtitles of every file in every window if each that has some shows them, else show them in all.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.on`](#blocksubtitleson), [`block.subtitles.off`](#blocksubtitlesoff), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Show Subtitles in Every Window

`app.subtitles.on`: Show the subtitles hidden, or the first, of every file in every window.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.on`](#blocksubtitleson), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Hide Subtitles in Every Window

`app.subtitles.off`: Hide the subtitles of every file in every window.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.subtitles.off`](#blocksubtitlesoff), [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Subtitles Earlier in Every Window

`app.subtitles.earlier`: Show the subtitles of every file in every window a little earlier.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Subtitles Later in Every Window

`app.subtitles.later`: Show the subtitles of every file in every window a little later.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Reset Subtitle Delay in Every Window

`app.subtitles.reset_delay`: Show the subtitles of every file in every window at their own times again.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

### Subtitles events

#### block.subtitles.on

A block's subtitles show: a subtitle track was picked where none was.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.subtitles.next_track`](#next-subtitles), [`block.subtitles.select_track`](#pick-subtitles), [`block.subtitles.toggle`](#showhide-subtitles), [`block.subtitles.on`](#blocksubtitleson), [`window.subtitles.next_track`](#next-subtitles-in-window), [`window.subtitles.select_track`](#pick-subtitles-in-window), [`window.subtitles.toggle`](#showhide-subtitles-in-window), [`window.subtitles.on`](#show-subtitles-in-window), [`app.subtitles.next_track`](#next-subtitles-in-every-window), [`app.subtitles.select_track`](#pick-subtitles-in-every-window), [`app.subtitles.toggle`](#showhide-subtitles-in-every-window), or [`app.subtitles.on`](#show-subtitles-in-every-window).

#### block.subtitles.off

A block's subtitles hid: no subtitle track is picked now.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.subtitles.next_track`](#next-subtitles), [`block.subtitles.select_track`](#pick-subtitles), [`block.subtitles.toggle`](#showhide-subtitles), [`block.subtitles.off`](#blocksubtitlesoff), [`window.subtitles.next_track`](#next-subtitles-in-window), [`window.subtitles.select_track`](#pick-subtitles-in-window), [`window.subtitles.toggle`](#showhide-subtitles-in-window), [`window.subtitles.off`](#hide-subtitles-in-window), [`app.subtitles.next_track`](#next-subtitles-in-every-window), [`app.subtitles.select_track`](#pick-subtitles-in-every-window), [`app.subtitles.toggle`](#showhide-subtitles-in-every-window), or [`app.subtitles.off`](#hide-subtitles-in-every-window).

## Video

### Video in a block

#### Picture in Picture

`block.picture_in_picture.toggle`: Play the block in a small window floating above every app's, or in its block again.

- Acts on a block.
- Keys: ⌃⌘P
- Arguments: none.
- Returns: nothing.
- Events: [`block.picture_in_picture.on`](#blockpicture_in_pictureon), [`block.picture_in_picture.off`](#blockpicture_in_pictureoff).

#### Enter Picture in Picture

`block.picture_in_picture.on`: Play the block in a small window floating above every app's, unless it does already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.picture_in_picture.on`](#blockpicture_in_pictureon).

#### Exit Picture in Picture

`block.picture_in_picture.off`: Play the block in its block again, unless it does already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.picture_in_picture.off`](#blockpicture_in_pictureoff).

#### Show/Hide Console

`block.console.toggle`: Show what the block's file is and how it plays, over its picture, or hide it.

- Acts on a block.
- Keys: ~, `
- Arguments: none.
- Returns: nothing.
- Events: [`block.console.on`](#blockconsoleon), [`block.console.off`](#blockconsoleoff).

#### Show Console

`block.console.on`: Show what the block's file is and how it plays, over its picture, unless it shows already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.console.on`](#blockconsoleon).

#### Hide Console

`block.console.off`: Hide what the block's file is and how it plays, unless it's hidden already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.console.off`](#blockconsoleoff).

#### Next Video Track

`block.video.next_track`: Show the file's next video track, the first after the last.

- Acts on a block.
- Keys: ⌃⌘V
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Pick Video Track

`block.video.select_track`: Show one of the file's video tracks.

- Acts on a block.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Rotate Right

`block.video.rotate_right`: Turn the picture a quarter turn clockwise.

- Acts on a block.
- Keys: ⌘R
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Rotate Left

`block.video.rotate_left`: Turn the picture a quarter turn counterclockwise.

- Acts on a block.
- Keys: ⌘L
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Flip Horizontally

`block.video.flip_horizontally`: Mirror the picture left to right.

- Acts on a block.
- Keys: ⇧⌘H
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Flip Vertically

`block.video.flip_vertically`: Turn the picture upside down, mirrored.

- Acts on a block.
- Keys: ⇧⌘V
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Next Aspect Ratio

`block.video.next_aspect_ratio`: Show the picture at the next aspect ratio, then as the file is.

- Acts on a block.
- Keys: A
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Aspect Ratio

`block.video.set_aspect_ratio`: Show the picture at an aspect ratio, or as the file is.

- Acts on a block.
- Arguments:
  - `ratio` (one of 1:1, 4:3, 16:10, 16:9, 1.85:1, 2.35:1, optional): The aspect ratio; none for the file's own.
- Returns: nothing.
- Events: none.

#### Next Crop

`block.video.next_crop`: Crop the picture to the next shape, then not at all.

- Acts on a block.
- Keys: C
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Crop

`block.video.set_crop`: Crop the picture to a shape, or not at all.

- Acts on a block.
- Arguments:
  - `crop` (one of 1:1, 4:3, 16:9, 1.85:1, 2.39:1, optional): The shape; none for no crop.
- Returns: nothing.
- Events: none.

#### Reset Picture

`block.video.reset_picture`: Show the picture as the file is: not turned, flipped, reshaped or cropped.

- Acts on a block.
- Keys: ⌘0
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Take Screenshot

`block.video.take_screenshot`: Save the frame showing as a file, and say where it went.

- Acts on a block.
- Keys: S
- Arguments: none.
- Returns: nothing.
- Events: none.

### Video in a window

#### Next Video Track in Window

`window.video.next_track`: Show the next video track of every file in the window, the first after the last.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Pick Video Track in Window

`window.video.select_track`: Show a video track of every file in the window that has it.

- Acts on every block in a window.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Rotate Right in Window

`window.video.rotate_right`: Turn the picture of every file in the window a quarter turn clockwise.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Rotate Left in Window

`window.video.rotate_left`: Turn the picture of every file in the window a quarter turn counterclockwise.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Flip Horizontally in Window

`window.video.flip_horizontally`: Mirror the picture of every file in the window left to right.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Flip Vertically in Window

`window.video.flip_vertically`: Turn the picture of every file in the window upside down, mirrored.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Next Aspect Ratio in Window

`window.video.next_aspect_ratio`: Show the picture of every file in the window at its next aspect ratio, then as the file is.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Aspect Ratio in Window

`window.video.set_aspect_ratio`: Show the picture of every file in the window at an aspect ratio, or as the file is.

- Acts on every block in a window.
- Arguments:
  - `ratio` (one of 1:1, 4:3, 16:10, 16:9, 1.85:1, 2.35:1, optional): The aspect ratio; none for the file's own.
- Returns: nothing.
- Events: none.

#### Next Crop in Window

`window.video.next_crop`: Crop the picture of every file in the window to its next shape, then not at all.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Crop in Window

`window.video.set_crop`: Crop the picture of every file in the window to a shape, or not at all.

- Acts on every block in a window.
- Arguments:
  - `crop` (one of 1:1, 4:3, 16:9, 1.85:1, 2.39:1, optional): The shape; none for no crop.
- Returns: nothing.
- Events: none.

#### Reset Picture in Window

`window.video.reset_picture`: Show the picture of every file in the window as the file is: not turned, flipped, reshaped or cropped.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Take Screenshot in Window

`window.video.take_screenshot`: Save the frame showing in every file in the window as a file, and say where each went.

- Acts on every block in a window.
- Arguments: none.
- Returns: nothing.
- Events: none.

### Video in every window

#### Next Video Track in Every Window

`app.video.next_track`: Show the next video track of every file in every window, the first after the last.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Pick Video Track in Every Window

`app.video.select_track`: Show a video track of every file in every window that has it.

- Acts on every block in every window.
- Arguments:
  - `number` (a whole number, at least 1): Which, from 1, as the tracks menu lists them.
- Returns: nothing.
- Events: [`block.playback.track_changed`](#blockplaybacktrack_changed).

#### Rotate Right in Every Window

`app.video.rotate_right`: Turn the picture of every file in every window a quarter turn clockwise.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Rotate Left in Every Window

`app.video.rotate_left`: Turn the picture of every file in every window a quarter turn counterclockwise.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Flip Horizontally in Every Window

`app.video.flip_horizontally`: Mirror the picture of every file in every window left to right.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Flip Vertically in Every Window

`app.video.flip_vertically`: Turn the picture of every file in every window upside down, mirrored.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Next Aspect Ratio in Every Window

`app.video.next_aspect_ratio`: Show the picture of every file in every window at its next aspect ratio, then as the file is.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Aspect Ratio in Every Window

`app.video.set_aspect_ratio`: Show the picture of every file in every window at an aspect ratio, or as the file is.

- Acts on every block in every window.
- Arguments:
  - `ratio` (one of 1:1, 4:3, 16:10, 16:9, 1.85:1, 2.35:1, optional): The aspect ratio; none for the file's own.
- Returns: nothing.
- Events: none.

#### Next Crop in Every Window

`app.video.next_crop`: Crop the picture of every file in every window to its next shape, then not at all.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Crop in Every Window

`app.video.set_crop`: Crop the picture of every file in every window to a shape, or not at all.

- Acts on every block in every window.
- Arguments:
  - `crop` (one of 1:1, 4:3, 16:9, 1.85:1, 2.39:1, optional): The shape; none for no crop.
- Returns: nothing.
- Events: none.

#### Reset Picture in Every Window

`app.video.reset_picture`: Show the picture of every file in every window as the file is: not turned, flipped, reshaped or cropped.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Take Screenshot in Every Window

`app.video.take_screenshot`: Save the frame showing in every file in every window as a file, and say where each went.

- Acts on every block in every window.
- Arguments: none.
- Returns: nothing.
- Events: none.

### Video events

#### block.picture_in_picture.on

A block began playing in a small window floating above every app's.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.picture_in_picture.toggle`](#picture-in-picture), or [`block.picture_in_picture.on`](#blockpicture_in_pictureon).

#### block.picture_in_picture.off

A block plays in its block again, not in the small window.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.picture_in_picture.toggle`](#picture-in-picture), [`block.picture_in_picture.off`](#blockpicture_in_pictureoff), or the small window's own buttons.

#### block.console.on

What a block's file is and how it plays showed over its picture.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.console.toggle`](#showhide-console), or [`block.console.on`](#blockconsoleon).

#### block.console.off

What a block's file is and how it plays hid.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`block.console.toggle`](#showhide-console), or [`block.console.off`](#blockconsoleoff).

## Interface

### Interface in a block

#### Show/Hide All

`block.ui.toggle`: Hide every panel over the block's video while any shows, else show them all.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.ui.on`](#blockuion), [`block.ui.off`](#blockuioff), [`block.controls.on`](#blockcontrolson), [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.on`](#blocktop_baron), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.track_switcher.off`](#blocktrack_switcheroff), [`block.notes.on`](#blocknoteson), [`block.notes.off`](#blocknotesoff).

#### Show All

`block.ui.on`: Show every panel over the block's video.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.ui.on`](#blockuion), [`block.controls.on`](#blockcontrolson), [`block.top_bar.on`](#blocktop_baron), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.notes.on`](#blocknoteson).

#### Hide All

`block.ui.off`: Hide every panel over the block's video.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.ui.off`](#blockuioff), [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.off`](#blocktrack_switcheroff), [`block.notes.off`](#blocknotesoff).

#### Show/Hide Controls

`block.controls.toggle`: Show the seek bar and buttons at the bottom of the block, or hide them.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.controls.on`](#blockcontrolson), [`block.controls.off`](#blockcontrolsoff).

#### Show Controls

`block.controls.on`: Show the seek bar and buttons at the bottom of the block, unless they show already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.controls.on`](#blockcontrolson).

#### Hide Controls

`block.controls.off`: Hide the seek bar and buttons at the bottom of the block, unless they're hidden already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.controls.off`](#blockcontrolsoff).

#### Show/Hide Top Bar

`block.top_bar.toggle`: Show the file's name and buttons at the top of the block, or hide them.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.top_bar.on`](#blocktop_baron), [`block.top_bar.off`](#blocktop_baroff).

#### Show Top Bar

`block.top_bar.on`: Show the file's name and buttons at the top of the block, unless they show already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.top_bar.on`](#blocktop_baron).

#### Hide Top Bar

`block.top_bar.off`: Hide the file's name and buttons at the top of the block, unless they're hidden already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.top_bar.off`](#blocktop_baroff).

#### Show/Hide Track Switcher

`block.track_switcher.toggle`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over the block, or switch without it.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.track_switcher.on`](#blocktrack_switcheron), [`block.track_switcher.off`](#blocktrack_switcheroff).

#### Show Track Switcher

`block.track_switcher.on`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over the block, unless it shows already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.track_switcher.on`](#blocktrack_switcheron).

#### Hide Track Switcher

`block.track_switcher.off`: Switch tracks over the block without the reel, unless it's hidden already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.track_switcher.off`](#blocktrack_switcheroff).

#### Show/Hide Notes

`block.notes.toggle`: Show notes on what a shortcut changed, like the volume, over the block, or not. Notes on what's wrong still show.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.notes.on`](#blocknoteson), [`block.notes.off`](#blocknotesoff).

#### Show Notes

`block.notes.on`: Show notes on what a shortcut changed, like the volume, over the block, unless they show already.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.notes.on`](#blocknoteson).

#### Hide Notes

`block.notes.off`: Hide notes on what a shortcut changed over the block, unless they're hidden already. Notes on what's wrong still show.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.notes.off`](#blocknotesoff).

### Interface in a window

#### Show/Hide All in Window

`window.ui.toggle`: Hide every panel over the video in each block in the window while the window shows any, else show them all.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.ui.on`](#windowuion), [`window.ui.off`](#windowuioff), [`window.controls.on`](#windowcontrolson), [`window.controls.off`](#windowcontrolsoff), [`window.top_bar.on`](#windowtop_baron), [`window.top_bar.off`](#windowtop_baroff), [`window.track_switcher.on`](#windowtrack_switcheron), [`window.track_switcher.off`](#windowtrack_switcheroff), [`window.notes.on`](#windownoteson), [`window.notes.off`](#windownotesoff), [`block.ui.on`](#blockuion), [`block.ui.off`](#blockuioff), [`block.controls.on`](#blockcontrolson), [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.on`](#blocktop_baron), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.track_switcher.off`](#blocktrack_switcheroff), [`block.notes.on`](#blocknoteson), [`block.notes.off`](#blocknotesoff).

#### Show All in Window

`window.ui.on`: Show every panel over the video in each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.ui.on`](#windowuion), [`window.controls.on`](#windowcontrolson), [`window.top_bar.on`](#windowtop_baron), [`window.track_switcher.on`](#windowtrack_switcheron), [`window.notes.on`](#windownoteson), [`block.ui.on`](#blockuion), [`block.controls.on`](#blockcontrolson), [`block.top_bar.on`](#blocktop_baron), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.notes.on`](#blocknoteson).

#### Hide All in Window

`window.ui.off`: Hide every panel over the video in each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.ui.off`](#windowuioff), [`window.controls.off`](#windowcontrolsoff), [`window.top_bar.off`](#windowtop_baroff), [`window.track_switcher.off`](#windowtrack_switcheroff), [`window.notes.off`](#windownotesoff), [`block.ui.off`](#blockuioff), [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.off`](#blocktrack_switcheroff), [`block.notes.off`](#blocknotesoff).

#### Show/Hide Controls in Window

`window.controls.toggle`: Show the seek bar and buttons at the bottom of each block in the window, and those opened in it, or hide them.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.controls.on`](#windowcontrolson), [`window.controls.off`](#windowcontrolsoff), [`block.controls.on`](#blockcontrolson), [`block.controls.off`](#blockcontrolsoff).

#### Show Controls in Window

`window.controls.on`: Show the seek bar and buttons at the bottom of each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.controls.on`](#windowcontrolson), [`block.controls.on`](#blockcontrolson).

#### Hide Controls in Window

`window.controls.off`: Hide the seek bar and buttons at the bottom of each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.controls.off`](#windowcontrolsoff), [`block.controls.off`](#blockcontrolsoff).

#### Show/Hide Top Bar in Window

`window.top_bar.toggle`: Show the file's name and buttons at the top of each block in the window, and those opened in it, or hide them.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.top_bar.on`](#windowtop_baron), [`window.top_bar.off`](#windowtop_baroff), [`block.top_bar.on`](#blocktop_baron), [`block.top_bar.off`](#blocktop_baroff).

#### Show Top Bar in Window

`window.top_bar.on`: Show the file's name and buttons at the top of each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.top_bar.on`](#windowtop_baron), [`block.top_bar.on`](#blocktop_baron).

#### Hide Top Bar in Window

`window.top_bar.off`: Hide the file's name and buttons at the top of each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.top_bar.off`](#windowtop_baroff), [`block.top_bar.off`](#blocktop_baroff).

#### Show/Hide Track Switcher in Window

`window.track_switcher.toggle`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over each block in the window, and those opened in it, or switch without it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.track_switcher.on`](#windowtrack_switcheron), [`window.track_switcher.off`](#windowtrack_switcheroff), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.track_switcher.off`](#blocktrack_switcheroff).

#### Show Track Switcher in Window

`window.track_switcher.on`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.track_switcher.on`](#windowtrack_switcheron), [`block.track_switcher.on`](#blocktrack_switcheron).

#### Hide Track Switcher in Window

`window.track_switcher.off`: Switch tracks without the reel in each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.track_switcher.off`](#windowtrack_switcheroff), [`block.track_switcher.off`](#blocktrack_switcheroff).

#### Show/Hide Notes in Window

`window.notes.toggle`: Show notes on what a shortcut changed, like the volume, over each block in the window, and those opened in it, or not. Notes on what's wrong still show.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.notes.on`](#windownoteson), [`window.notes.off`](#windownotesoff), [`block.notes.on`](#blocknoteson), [`block.notes.off`](#blocknotesoff).

#### Show Notes in Window

`window.notes.on`: Show notes on what a shortcut changed, like the volume, over each block in the window, and those opened in it.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.notes.on`](#windownoteson), [`block.notes.on`](#blocknoteson).

#### Hide Notes in Window

`window.notes.off`: Hide notes on what a shortcut changed over each block in the window, and those opened in it. Notes on what's wrong still show.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.notes.off`](#windownotesoff), [`block.notes.off`](#blocknotesoff).

### Interface in every window

#### Show/Hide All in Every Window

`app.ui.toggle`: Hide every panel over the video in every window while Settings show any, else show them all.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.ui.on`](#appuion), [`app.ui.off`](#appuioff), [`app.controls.on`](#appcontrolson), [`app.controls.off`](#appcontrolsoff), [`app.top_bar.on`](#apptop_baron), [`app.top_bar.off`](#apptop_baroff), [`app.track_switcher.on`](#apptrack_switcheron), [`app.track_switcher.off`](#apptrack_switcheroff), [`app.notes.on`](#appnoteson), [`app.notes.off`](#appnotesoff), [`window.ui.on`](#windowuion), [`window.ui.off`](#windowuioff), [`window.controls.on`](#windowcontrolson), [`window.controls.off`](#windowcontrolsoff), [`window.top_bar.on`](#windowtop_baron), [`window.top_bar.off`](#windowtop_baroff), [`window.track_switcher.on`](#windowtrack_switcheron), [`window.track_switcher.off`](#windowtrack_switcheroff), [`window.notes.on`](#windownoteson), [`window.notes.off`](#windownotesoff), [`block.ui.on`](#blockuion), [`block.ui.off`](#blockuioff), [`block.controls.on`](#blockcontrolson), [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.on`](#blocktop_baron), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.track_switcher.off`](#blocktrack_switcheroff), [`block.notes.on`](#blocknoteson), [`block.notes.off`](#blocknotesoff).

#### Show All in Every Window

`app.ui.on`: Show every panel over the video in every window, and those opened later.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.ui.on`](#appuion), [`app.controls.on`](#appcontrolson), [`app.top_bar.on`](#apptop_baron), [`app.track_switcher.on`](#apptrack_switcheron), [`app.notes.on`](#appnoteson), [`window.ui.on`](#windowuion), [`window.controls.on`](#windowcontrolson), [`window.top_bar.on`](#windowtop_baron), [`window.track_switcher.on`](#windowtrack_switcheron), [`window.notes.on`](#windownoteson), [`block.ui.on`](#blockuion), [`block.controls.on`](#blockcontrolson), [`block.top_bar.on`](#blocktop_baron), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.notes.on`](#blocknoteson).

#### Hide All in Every Window

`app.ui.off`: Hide every panel over the video in every window, and those opened later.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.ui.off`](#appuioff), [`app.controls.off`](#appcontrolsoff), [`app.top_bar.off`](#apptop_baroff), [`app.track_switcher.off`](#apptrack_switcheroff), [`app.notes.off`](#appnotesoff), [`window.ui.off`](#windowuioff), [`window.controls.off`](#windowcontrolsoff), [`window.top_bar.off`](#windowtop_baroff), [`window.track_switcher.off`](#windowtrack_switcheroff), [`window.notes.off`](#windownotesoff), [`block.ui.off`](#blockuioff), [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.off`](#blocktrack_switcheroff), [`block.notes.off`](#blocknotesoff).

#### Show/Hide Controls in Every Window

`app.controls.toggle`: Show the seek bar and buttons at the bottom of every block in every window, and those opened later, or hide them, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.controls.on`](#appcontrolson), [`app.controls.off`](#appcontrolsoff), [`window.controls.on`](#windowcontrolson), [`window.controls.off`](#windowcontrolsoff), [`block.controls.on`](#blockcontrolson), [`block.controls.off`](#blockcontrolsoff).

#### Show Controls in Every Window

`app.controls.on`: Show the seek bar and buttons at the bottom of every block in every window, and those opened later, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.controls.on`](#appcontrolson), [`window.controls.on`](#windowcontrolson), [`block.controls.on`](#blockcontrolson).

#### Hide Controls in Every Window

`app.controls.off`: Hide the seek bar and buttons at the bottom of every block in every window, and those opened later, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.controls.off`](#appcontrolsoff), [`window.controls.off`](#windowcontrolsoff), [`block.controls.off`](#blockcontrolsoff).

#### Show/Hide Top Bar in Every Window

`app.top_bar.toggle`: Show the file's name and buttons at the top of every block in every window, and those opened later, or hide them, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.top_bar.on`](#apptop_baron), [`app.top_bar.off`](#apptop_baroff), [`window.top_bar.on`](#windowtop_baron), [`window.top_bar.off`](#windowtop_baroff), [`block.top_bar.on`](#blocktop_baron), [`block.top_bar.off`](#blocktop_baroff).

#### Show Top Bar in Every Window

`app.top_bar.on`: Show the file's name and buttons at the top of every block in every window, and those opened later, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.top_bar.on`](#apptop_baron), [`window.top_bar.on`](#windowtop_baron), [`block.top_bar.on`](#blocktop_baron).

#### Hide Top Bar in Every Window

`app.top_bar.off`: Hide the file's name and buttons at the top of every block in every window, and those opened later, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.top_bar.off`](#apptop_baroff), [`window.top_bar.off`](#windowtop_baroff), [`block.top_bar.off`](#blocktop_baroff).

#### Show/Hide Track Switcher in Every Window

`app.track_switcher.toggle`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over every block in every window, and those opened later, or switch without it, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.track_switcher.on`](#apptrack_switcheron), [`app.track_switcher.off`](#apptrack_switcheroff), [`window.track_switcher.on`](#windowtrack_switcheron), [`window.track_switcher.off`](#windowtrack_switcheroff), [`block.track_switcher.on`](#blocktrack_switcheron), [`block.track_switcher.off`](#blocktrack_switcheroff).

#### Show Track Switcher in Every Window

`app.track_switcher.on`: Show the reel of tracks a shortcut rolls to the audio, video or subtitles picked, over every block in every window, and those opened later, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.track_switcher.on`](#apptrack_switcheron), [`window.track_switcher.on`](#windowtrack_switcheron), [`block.track_switcher.on`](#blocktrack_switcheron).

#### Hide Track Switcher in Every Window

`app.track_switcher.off`: Switch tracks without the reel in every block in every window, and those opened later, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.track_switcher.off`](#apptrack_switcheroff), [`window.track_switcher.off`](#windowtrack_switcheroff), [`block.track_switcher.off`](#blocktrack_switcheroff).

#### Show/Hide Notes in Every Window

`app.notes.toggle`: Show notes on what a shortcut changed, like the volume, over every block in every window, and those opened later, or not, as Settings → Appearance does. Notes on what's wrong still show.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.notes.on`](#appnoteson), [`app.notes.off`](#appnotesoff), [`window.notes.on`](#windownoteson), [`window.notes.off`](#windownotesoff), [`block.notes.on`](#blocknoteson), [`block.notes.off`](#blocknotesoff).

#### Show Notes in Every Window

`app.notes.on`: Show notes on what a shortcut changed, like the volume, over every block in every window, and those opened later, as Settings → Appearance does.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.notes.on`](#appnoteson), [`window.notes.on`](#windownoteson), [`block.notes.on`](#blocknoteson).

#### Hide Notes in Every Window

`app.notes.off`: Hide notes on what a shortcut changed over every block in every window, and those opened later, as Settings → Appearance does. Notes on what's wrong still show.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.notes.off`](#appnotesoff), [`window.notes.off`](#windownotesoff), [`block.notes.off`](#blocknotesoff).

### Interface events

#### app.ui.on

A panel showed over the video in every window, as Settings have them, where none did.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.ui.off

Every panel over the video in every window, as Settings have them, hid.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.controls.on

The seek bar and buttons at the bottom of every block, as Settings have them, showed.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.controls.toggle`](#showhide-controls-in-every-window), [`app.controls.on`](#appcontrolson), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.controls.off

The seek bar and buttons at the bottom of every block, as Settings have them, hid.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.controls.toggle`](#showhide-controls-in-every-window), [`app.controls.off`](#appcontrolsoff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.top_bar.on

The file's name and buttons at the top of every block, as Settings have them, showed.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.top_bar.toggle`](#showhide-top-bar-in-every-window), [`app.top_bar.on`](#apptop_baron), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.top_bar.off

The file's name and buttons at the top of every block, as Settings have them, hid.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.top_bar.toggle`](#showhide-top-bar-in-every-window), [`app.top_bar.off`](#apptop_baroff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.track_switcher.on

Every block, as Settings have it, shows the reel of tracks as a shortcut rolls to one.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.track_switcher.toggle`](#showhide-track-switcher-in-every-window), [`app.track_switcher.on`](#apptrack_switcheron), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.track_switcher.off

Every block, as Settings have it, switches tracks without the reel.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.track_switcher.toggle`](#showhide-track-switcher-in-every-window), [`app.track_switcher.off`](#apptrack_switcheroff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.notes.on

Every block, as Settings have it, shows notes on what a shortcut changed.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.notes.toggle`](#showhide-notes-in-every-window), [`app.notes.on`](#appnoteson), [`app.settings.reset`](#reset-setting), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### app.notes.off

Every block, as Settings have it, shows no notes on what a shortcut changed.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.notes.toggle`](#showhide-notes-in-every-window), [`app.notes.off`](#appnotesoff), [`app.settings.set`](#change-setting), [`app.settings.reload`](#reload-settings), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.ui.on

A panel showed over the video in a window's blocks, and those opened in it, where none did.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.ui.off

Every panel over the video in a window's blocks, and those opened in it, hid.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.controls.on

The seek bar and buttons at the bottom of a window's blocks, and those opened in it, showed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.controls.toggle`](#showhide-controls-in-every-window), [`app.controls.on`](#appcontrolson), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.controls.toggle`](#showhide-controls-in-window), [`window.controls.on`](#windowcontrolson), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.controls.off

The seek bar and buttons at the bottom of a window's blocks, and those opened in it, hid.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.controls.toggle`](#showhide-controls-in-every-window), [`app.controls.off`](#appcontrolsoff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.controls.toggle`](#showhide-controls-in-window), [`window.controls.off`](#windowcontrolsoff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.top_bar.on

The file's name and buttons at the top of a window's blocks, and those opened in it, showed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.top_bar.toggle`](#showhide-top-bar-in-every-window), [`app.top_bar.on`](#apptop_baron), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.top_bar.toggle`](#showhide-top-bar-in-window), [`window.top_bar.on`](#windowtop_baron), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.top_bar.off

The file's name and buttons at the top of a window's blocks, and those opened in it, hid.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.top_bar.toggle`](#showhide-top-bar-in-every-window), [`app.top_bar.off`](#apptop_baroff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.top_bar.toggle`](#showhide-top-bar-in-window), [`window.top_bar.off`](#windowtop_baroff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.track_switcher.on

A window's blocks, and those opened in it, show the reel of tracks as a shortcut rolls to one.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.track_switcher.toggle`](#showhide-track-switcher-in-every-window), [`app.track_switcher.on`](#apptrack_switcheron), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.track_switcher.toggle`](#showhide-track-switcher-in-window), [`window.track_switcher.on`](#windowtrack_switcheron), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.track_switcher.off

A window's blocks, and those opened in it, switch tracks without the reel.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.track_switcher.toggle`](#showhide-track-switcher-in-every-window), [`app.track_switcher.off`](#apptrack_switcheroff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.track_switcher.toggle`](#showhide-track-switcher-in-window), [`window.track_switcher.off`](#windowtrack_switcheroff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.notes.on

A window's blocks, and those opened in it, show notes on what a shortcut changed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.notes.toggle`](#showhide-notes-in-every-window), [`app.notes.on`](#appnoteson), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.notes.toggle`](#showhide-notes-in-window), [`window.notes.on`](#windownoteson), [`app.settings.reset`](#reset-setting), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### window.notes.off

A window's blocks, and those opened in it, show no notes on what a shortcut changed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.notes.toggle`](#showhide-notes-in-every-window), [`app.notes.off`](#appnotesoff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.notes.toggle`](#showhide-notes-in-window), [`window.notes.off`](#windownotesoff), [`app.settings.set`](#change-setting), [`app.settings.reload`](#reload-settings), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.ui.on

A panel showed over a block's video, where none did.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`block.ui.toggle`](#showhide-all), [`block.ui.on`](#blockuion), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.ui.off

Every panel over a block's video hid.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`block.ui.toggle`](#showhide-all), [`block.ui.off`](#blockuioff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.controls.on

The seek bar and buttons at the bottom of a block showed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.controls.toggle`](#showhide-controls-in-every-window), [`app.controls.on`](#appcontrolson), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.controls.toggle`](#showhide-controls-in-window), [`window.controls.on`](#windowcontrolson), [`block.ui.toggle`](#showhide-all), [`block.ui.on`](#blockuion), [`block.controls.toggle`](#showhide-controls), [`block.controls.on`](#blockcontrolson), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.controls.off

The seek bar and buttons at the bottom of a block hid.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.controls.toggle`](#showhide-controls-in-every-window), [`app.controls.off`](#appcontrolsoff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.controls.toggle`](#showhide-controls-in-window), [`window.controls.off`](#windowcontrolsoff), [`block.ui.toggle`](#showhide-all), [`block.ui.off`](#blockuioff), [`block.set_ui`](#show-panels), [`block.controls.toggle`](#showhide-controls), [`block.controls.off`](#blockcontrolsoff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.top_bar.on

The file's name and buttons at the top of a block showed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.top_bar.toggle`](#showhide-top-bar-in-every-window), [`app.top_bar.on`](#apptop_baron), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.top_bar.toggle`](#showhide-top-bar-in-window), [`window.top_bar.on`](#windowtop_baron), [`block.ui.toggle`](#showhide-all), [`block.ui.on`](#blockuion), [`block.top_bar.toggle`](#showhide-top-bar), [`block.top_bar.on`](#blocktop_baron), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.top_bar.off

The file's name and buttons at the top of a block hid.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.top_bar.toggle`](#showhide-top-bar-in-every-window), [`app.top_bar.off`](#apptop_baroff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.top_bar.toggle`](#showhide-top-bar-in-window), [`window.top_bar.off`](#windowtop_baroff), [`block.ui.toggle`](#showhide-all), [`block.ui.off`](#blockuioff), [`block.set_ui`](#show-panels), [`block.top_bar.toggle`](#showhide-top-bar), [`block.top_bar.off`](#blocktop_baroff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.track_switcher.on

A block shows the reel of tracks as a shortcut rolls to one.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.track_switcher.toggle`](#showhide-track-switcher-in-every-window), [`app.track_switcher.on`](#apptrack_switcheron), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.track_switcher.toggle`](#showhide-track-switcher-in-window), [`window.track_switcher.on`](#windowtrack_switcheron), [`block.ui.toggle`](#showhide-all), [`block.ui.on`](#blockuion), [`block.track_switcher.toggle`](#showhide-track-switcher), [`block.track_switcher.on`](#blocktrack_switcheron), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.track_switcher.off

A block switches tracks without the reel.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.track_switcher.toggle`](#showhide-track-switcher-in-every-window), [`app.track_switcher.off`](#apptrack_switcheroff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.track_switcher.toggle`](#showhide-track-switcher-in-window), [`window.track_switcher.off`](#windowtrack_switcheroff), [`block.ui.toggle`](#showhide-all), [`block.ui.off`](#blockuioff), [`block.set_ui`](#show-panels), [`block.track_switcher.toggle`](#showhide-track-switcher), [`block.track_switcher.off`](#blocktrack_switcheroff), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.notes.on

A block shows notes on what a shortcut changed.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.notes.toggle`](#showhide-notes-in-every-window), [`app.notes.on`](#appnoteson), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.on`](#windowuion), [`window.notes.toggle`](#showhide-notes-in-window), [`window.notes.on`](#windownoteson), [`block.ui.toggle`](#showhide-all), [`block.ui.on`](#blockuion), [`block.notes.toggle`](#showhide-notes), [`block.notes.on`](#blocknoteson), [`app.settings.reset`](#reset-setting), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

#### block.notes.off

A block shows no notes on what a shortcut changed; notes on what's wrong still show.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.off`](#appuioff), [`app.notes.toggle`](#showhide-notes-in-every-window), [`app.notes.off`](#appnotesoff), [`window.ui.toggle`](#showhide-all-in-window), [`window.ui.off`](#windowuioff), [`window.set_ui`](#show-panels-in-window), [`window.notes.toggle`](#showhide-notes-in-window), [`window.notes.off`](#windownotesoff), [`block.ui.toggle`](#showhide-all), [`block.ui.off`](#blockuioff), [`block.notes.toggle`](#showhide-notes), [`block.notes.off`](#blocknotesoff), [`app.settings.set`](#change-setting), [`app.settings.reload`](#reload-settings), or Settings turning a panel over the video on or off, importing a settings file or resetting every setting.

## Blocks

### Block commands

#### List Blocks

`block.ls`: List the blocks in the window, in the grid's order.

- Acts on a player window.
- Arguments: none.
- Returns: [{id, label, title, file, url, playing, opened, error, number, front}]: each block's id and label (or null), its file's name, the file's path or the stream's URL (without a user, password or query; the other null), whether it plays, whether its file has opened, why it couldn't open or play on (or null: still opening, if it hasn't opened), its place from 1, and whether keys go to it.
- Events: none.

#### Inspect Block

`block.inspect`: Say all about the block: what it plays, where it is, and how it's set to play.

- Acts on a block.
- Arguments: none.
- Returns: {as block.ls, window, window_title, position, duration, frame_rate, ended, buffering, volume, muted, looping, tracks: {video, audio, subtitles: [{number, label, language, title, codec, format, default, forced, hearing_impaired, file}]}, picked: {video, audio, subtitles}, delays: {audio, subtitles}, picture: {turns, mirrored, aspect_ratio, crop}, chapter, chapters: [{number, name, start, end}], casting: {id, kind, name} or null, output: {id, name} or null, visible}: its window's id and title, times in seconds (duration null for a live stream), frames a second (null for sound alone), whether it waits for data to play from (just opened or sought, or a stream run dry), volume from 0 to 100, tracks and chapters numbered from 1, the tracks picked and the chapter by number or null, delays in seconds, quarter turns clockwise, the output it plays through of its own, and the panels over its video it shows, of controls, top_bar, track_switcher and notes.
- Events: none.

#### Go to Block

`block.focus`: Put the block in front, keys going to it, and bring its window to the front.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`window.active_changed`](#windowactive_changed), [`block.active_changed`](#blockactive_changed).

#### Close File

`block.close`: Close the block's file. The window stays, empty after the last.

- Acts on a block.
- Keys: ⌘W
- Arguments: none.
- Returns: nothing.
- Events: [`block.closed`](#blockclosed), [`window.layout_changed`](#windowlayout_changed), [`block.active_changed`](#blockactive_changed).

#### Close All Files

`block.close_all`: Close every file in the window. The window stays, empty.

- Acts on every block in a window.
- Keys: ⇧⌘W
- Arguments: none.
- Returns: nothing.
- Events: [`block.closed`](#blockclosed), [`window.layout_changed`](#windowlayout_changed), [`block.active_changed`](#blockactive_changed).

#### Close Other Files

`block.close_others`: Close every file in the window but the block's.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`block.closed`](#blockclosed), [`window.layout_changed`](#windowlayout_changed).

#### Focus Block to the Left

`block.focus.left`: Send keys to the block left of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘←
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Focus Block to the Right

`block.focus.right`: Send keys to the block right of the one they go to.

- Acts on a player window.
- Keys: ⌥⌘→
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Focus Block Above

`block.focus.up`: Send keys to the block above the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↑
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Focus Block Below

`block.focus.down`: Send keys to the block below the one they go to.

- Acts on a player window.
- Keys: ⌥⌘↓
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Focus Previous Block

`block.focus.previous`: Send keys to the block before the one they go to, the last before the first.

- Acts on a player window.
- Keys: ⌘[
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Focus Next Block

`block.focus.next`: Send keys to the block after the one they go to, the first after the last.

- Acts on a player window.
- Keys: ⌘]
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Go Back One Block

`block.back`: Send keys to the block clicked or gone to before the one they go to; again, the one before that, and so on, until something else is done.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Go to Recent Block

`block.recent`: Send keys to the block clicked or gone to last but the one they go to; again, back to that one.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.active_changed`](#blockactive_changed).

#### Move Block Left

`block.move.left`: Swap the block keys go to with the one to its left.

- Acts on a player window.
- Keys: ⌥⇧⌘←
- Arguments: none.
- Returns: nothing.
- Events: [`window.layout_changed`](#windowlayout_changed).

#### Move Block Right

`block.move.right`: Swap the block keys go to with the one to its right.

- Acts on a player window.
- Keys: ⌥⇧⌘→
- Arguments: none.
- Returns: nothing.
- Events: [`window.layout_changed`](#windowlayout_changed).

#### Move Block Up

`block.move.up`: Swap the block keys go to with the one above it.

- Acts on a player window.
- Keys: ⌥⇧⌘↑
- Arguments: none.
- Returns: nothing.
- Events: [`window.layout_changed`](#windowlayout_changed).

#### Move Block Down

`block.move.down`: Swap the block keys go to with the one below it.

- Acts on a player window.
- Keys: ⌥⇧⌘↓
- Arguments: none.
- Returns: nothing.
- Events: [`window.layout_changed`](#windowlayout_changed).

#### Maximize Block

`block.zoom`: Fill the window with the block, or show every block again; a window's only block fills the screen instead.

- Acts on a block.
- Keys: ⇧⌘↩
- Arguments: none.
- Returns: nothing.
- Events: [`window.full_screen.on`](#windowfull_screenon), [`window.layout_changed`](#windowlayout_changed).

#### Rename Block…

`block.rename`: Label the block: its header shows the label in place of the file's name, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a block.
- Arguments:
  - `label` (text, optional): What to call it; no two blocks are called the same, in whichever windows.
- Returns: nothing.
- Events: [`block.label_changed`](#blocklabel_changed).

#### Move Block To

`block.move`: Put the block in another place in its window's grid, the blocks between moving over, as dragging it onto another does.

- Acts on a block.
- Arguments:
  - `position` (a whole number, at least 1): Where, from 1, the first block's place.
- Returns: nothing.
- Events: [`window.layout_changed`](#windowlayout_changed).

#### Move Block to Window

`block.move_to_window`: Move the block to another window, last there, where it plays on.

- Acts on a block.
- Arguments:
  - `window` (text): The window's id, its label, or its number, from 1.
- Returns: nothing.
- Events: [`block.moved`](#blockmoved), [`window.layout_changed`](#windowlayout_changed), [`block.active_changed`](#blockactive_changed).

#### Move Block to New Window

`block.move_to_new_window`: Move the block to a window of its own beside this one, where it plays on.

- Acts on a block.
- Arguments: none.
- Returns: {window}: the new window's id.
- Events: [`window.created`](#windowcreated), [`block.moved`](#blockmoved), [`window.layout_changed`](#windowlayout_changed), [`block.active_changed`](#blockactive_changed).

#### Copy File Path

`block.copy_path`: Copy the path of the block's file, or the URL of its stream; hands back what was copied.

- Acts on a block.
- Keys: ⌥⌘C
- Arguments: none.
- Returns: The path or URL copied.
- Events: none.

#### Copy Current Time

`block.copy_time`: Copy where the block's file is, as its bar shows it, such as 1:23:45; hands back what was copied.

- Acts on a block.
- Arguments: none.
- Returns: The time copied, such as 1:23:45.
- Events: none.

#### Show Panels

`block.set_ui`: Show the panels named over the block's video, and hide the rest.

- Acts on a block.
- Arguments:
  - `visible` (a list of any of controls, top_bar, track_switcher, notes, none too): The panels to show; those not named hide.
- Returns: nothing.
- Events: [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.off`](#blocktrack_switcheroff).

### Block events

#### block.created

A block came into a window, for a file to play in.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `label`: Its label; none without one.
- Caused by: [`app.open`](#open-paths-or-urls), or files dropped on a window or picked when `window.open_files` or `window.open_url` asks.

#### block.moved

A block moved to another window: those listening to the window it left hear it too.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `from_window_id`: The id of the window it left.
  - `from_window_title`: That window's title.
- Caused by: [`block.move_to_window`](#move-block-to-window), or [`block.move_to_new_window`](#move-block-to-new-window).

#### block.closed

A block closed, and the file in it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
- Caused by: [`app.quit`](#appquit), [`window.close`](#close-window), [`window.close_others`](#close-other-windows), [`block.close`](#close-file), [`block.close_all`](#close-all-files), [`block.close_others`](#close-other-files), or Plays quitting.

#### block.label_changed

A block was labeled, or its label taken away.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `label`: Its label; none once it's taken away.
- Caused by: [`block.rename`](#rename-block).

#### block.active_changed

Keys go to another block in a window, or to none.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `block_id`: The block's id; none if keys go to none.
  - `block_title`: Its title, as its header shows it; none without a block.
- Caused by: [`app.open`](#open-paths-or-urls), [`block.focus`](#go-to-block), [`block.close`](#close-file), [`block.close_all`](#close-all-files), [`block.focus.left`](#focus-block-to-the-left), [`block.focus.right`](#focus-block-to-the-right), [`block.focus.up`](#focus-block-above), [`block.focus.down`](#focus-block-below), [`block.focus.previous`](#focus-previous-block), [`block.focus.next`](#focus-next-block), [`block.back`](#go-back-one-block), [`block.recent`](#go-to-recent-block), [`block.move_to_window`](#move-block-to-window), [`block.move_to_new_window`](#move-block-to-new-window), or a click on a block or the mouse going to one.

## Windows

### Window commands

#### List Windows

`window.ls`: List the player windows, in the order ⌘1 to ⌘9 number them.

- Acts on the app.
- Arguments: none.
- Returns: [{id, label, title, number, front, blocks}]: each window's id, label (or null) and title, as its title bar shows it, its number from 1, whether it's in front, and how many blocks it has.
- Events: none.

#### Inspect Window

`window.inspect`: Say all about the window: where it is, how it shows and its blocks.

- Acts on a player window.
- Arguments: none.
- Returns: {id, label, title, number, front, blocks: [as block.ls], bounds: {x, y, width, height}, display, full_screen, on_top, visible}: as window.ls says, its blocks as block.ls lists them, where it is in points, the id of the display it's on, whether it's full screen and floats on top, and the panels over the video it shows in blocks opened in it, of controls, top_bar, track_switcher and notes.
- Events: none.

#### New Window

`window.new`: Open an empty player window, in front, numbered after the others.

- Acts on the app.
- Keys: ⌘N
- Arguments: none.
- Returns: {window}: the new window's id.
- Events: [`window.created`](#windowcreated), [`window.active_changed`](#windowactive_changed).

#### Open…

`window.open_files`: Choose files or folders to play in the window.

- Acts on a player window.
- Keys: ⌘O
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Open URL…

`window.open_url`: Type the URL of a stream to play in the window.

- Acts on a player window.
- Keys: ⇧⌘O
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Command Palette…

`window.command_palette`: List every command that can run in the window, to find one by name and run it, picking or typing what it takes.

- Acts on a player window.
- Keys: ⇧⌘P
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Go to…

`window.go_to`: List the windows, their blocks, the files in the front block's folder and those played lately, to find one by name and go there: bring it to the front, play the file in the block, or open it.

- Acts on a player window.
- Keys: ⌘P
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Close Window

`window.close`: Close the window and the files in it; with none named, the window in front, Settings and About too.

- Acts on a window, Settings and About too.
- Keys: ⌥⌘W, ⌘W
- Arguments: none.
- Returns: nothing.
- Events: [`block.closed`](#blockclosed), [`window.closed`](#windowclosed), [`window.active_changed`](#windowactive_changed).

#### Close Other Windows

`window.close_others`: Close every player window but this one, and the files in them.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`block.closed`](#blockclosed), [`window.closed`](#windowclosed).

#### Rename Window…

`window.rename`: Label the window: its title shows the label, and it can be named by it. A blank one takes the label away; with none given, the window asks for one.

- Acts on a player window.
- Arguments:
  - `label` (text, optional): What to call it; no two windows are called the same.
- Returns: nothing.
- Events: [`window.label_changed`](#windowlabel_changed).

#### Go to Window

`window.focus`: Bring the window to the front. ⌘1 to ⌘9 go to the first nine, by their numbers.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.active_changed`](#windowactive_changed).

#### Next Window

`window.next`: Bring the window after the one in front to the front: the first after the last.

- Acts on the app.
- Keys: ⌃⇥
- Arguments: none.
- Returns: nothing.
- Events: [`window.active_changed`](#windowactive_changed).

#### Previous Window

`window.previous`: Bring the window before the one in front to the front: the last before the first.

- Acts on the app.
- Keys: ⌃⇧⇥
- Arguments: none.
- Returns: nothing.
- Events: [`window.active_changed`](#windowactive_changed).

#### Go Back One Window

`window.back`: Bring back the player window in front before this one; again, the one before that, and so on, until something else is done.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`window.active_changed`](#windowactive_changed).

#### Go to Recent Window

`window.recent`: Bring back the player window in front last; again, the one in front first.

- Acts on the app.
- Keys: ⌘'
- Arguments: none.
- Returns: nothing.
- Events: [`window.active_changed`](#windowactive_changed).

#### Move Window To

`window.move`: Give the window another number, from 1, which ⌘1 to ⌘9 go to it by: the windows between move up or down one.

- Acts on a player window.
- Arguments:
  - `position` (a whole number, at least 1): Its number.
- Returns: nothing.
- Events: none.

#### Move Window Backward

`window.move.backward`: Give the window the number before its own, the window there taking its own.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Move Window Forward

`window.move.forward`: Give the window the number after its own, the window there taking its own.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Full Screen

`window.full_screen.toggle`: Show the window full screen, or as it was again.

- Acts on a player window.
- Keys: ⌃⌘F, F
- Arguments: none.
- Returns: nothing.
- Events: [`window.full_screen.on`](#windowfull_screenon), [`window.full_screen.off`](#windowfull_screenoff).

#### Enter Full Screen

`window.full_screen.on`: Show the window full screen, unless it is already.

- Acts on a player window.
- Arguments: none.
- Returns: nothing.
- Events: [`window.full_screen.on`](#windowfull_screenon).

#### Exit Full Screen

`window.full_screen.off`: Show the window as it was before it was full screen.

- Acts on a player window.
- Keys: ⎋
- Arguments: none.
- Returns: nothing.
- Events: [`window.full_screen.off`](#windowfull_screenoff).

#### Match Video Size

`window.match_video_size`: Size the window for its videos to show at their own size, as far as the screen allows.

- Acts on a player window.
- Keys: ⌥⌘0
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Float on Top

`window.float_on_top.toggle`: Keep player windows above other apps' windows, or stop.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`window.float_on_top.on`](#windowfloat_on_topon), [`window.float_on_top.off`](#windowfloat_on_topoff).

#### Start Floating on Top

`window.float_on_top.on`: Keep player windows above other apps' windows, unless they are already.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`window.float_on_top.on`](#windowfloat_on_topon).

#### Stop Floating on Top

`window.float_on_top.off`: Stop keeping player windows above other apps' windows, unless they float on top while playing.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`window.float_on_top.off`](#windowfloat_on_topoff).

#### Float on Top While Playing

`window.float_on_top_while_playing.toggle`: Keep player windows above other apps' windows while a file in them plays, or stop.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`window.float_on_top_while_playing.on`](#windowfloat_on_top_while_playingon), [`window.float_on_top_while_playing.off`](#windowfloat_on_top_while_playingoff).

#### Start Floating on Top While Playing

`window.float_on_top_while_playing.on`: Keep player windows above other apps' windows while a file in them plays, unless they are already.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`window.float_on_top_while_playing.on`](#windowfloat_on_top_while_playingon).

#### Stop Floating on Top While Playing

`window.float_on_top_while_playing.off`: Stop keeping player windows above other apps' windows while a file in them plays, unless they float on top always.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: [`app.settings.changed`](#appsettingschanged), [`window.float_on_top_while_playing.off`](#windowfloat_on_top_while_playingoff).

#### Show on Display

`window.show_on_display`: Show the window full screen on a display, coming back where it was once it leaves full screen there.

- Acts on a player window.
- Arguments:
  - `display` (a whole number, at least 0): The display's id.
- Returns: nothing.
- Events: [`window.full_screen.on`](#windowfull_screenon).

#### Show Panels in Window

`window.set_ui`: Show the panels named over the video in each block in the window, and those opened in it, and hide the rest.

- Acts on a player window.
- Arguments:
  - `visible` (a list of any of controls, top_bar, track_switcher, notes, none too): The panels to show; those not named hide.
- Returns: nothing.
- Events: [`window.ui.off`](#windowuioff), [`window.controls.off`](#windowcontrolsoff), [`window.top_bar.off`](#windowtop_baroff), [`window.track_switcher.off`](#windowtrack_switcheroff), [`window.notes.off`](#windownotesoff), [`block.ui.off`](#blockuioff), [`block.controls.off`](#blockcontrolsoff), [`block.top_bar.off`](#blocktop_baroff), [`block.track_switcher.off`](#blocktrack_switcheroff), [`block.notes.off`](#blocknotesoff).

### Window events

#### window.created

A player window opened.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `label`: Its label; none without one.
- Caused by: [`app.open`](#open-paths-or-urls), [`window.new`](#new-window), or [`block.move_to_new_window`](#move-block-to-new-window).

#### window.closed

A player window closed: told after each of its blocks is told closed.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.quit`](#appquit), [`window.close`](#close-window), [`window.close_others`](#close-other-windows), or Plays quitting.

#### window.label_changed

A window was labeled, or its label taken away.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `label`: Its label; none once it's taken away.
- Caused by: [`window.rename`](#rename-window).

#### window.active_changed

Another player window came to the front, or none is in front: its `window_id` and `window_title` are none then.

- Heard by those listening to the app's events, and to its window's.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`app.open`](#open-paths-or-urls), [`app.quit`](#appquit), [`window.new`](#new-window), [`window.close`](#close-window), [`window.focus`](#go-to-window), [`window.next`](#next-window), [`window.previous`](#previous-window), [`window.back`](#go-back-one-window), [`window.recent`](#go-to-recent-window), [`block.focus`](#go-to-block), or a click on another window.

#### window.full_screen.on

A window went full screen.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`window.full_screen.toggle`](#full-screen), [`window.full_screen.on`](#windowfull_screenon), [`window.show_on_display`](#show-on-display), [`block.zoom`](#maximize-block), or the window's own full screen button.

#### window.full_screen.off

A window came back from full screen.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
- Caused by: [`window.full_screen.toggle`](#full-screen), [`window.full_screen.off`](#windowfull_screenoff), or the window's own full screen button.

#### window.float_on_top.on

Player windows began to float above other apps' windows.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`window.float_on_top.toggle`](#float-on-top), [`window.float_on_top.on`](#windowfloat_on_topon), or Settings → Appearance floating player windows on top, importing a settings file or resetting every setting.

#### window.float_on_top.off

Player windows stopped floating above other apps' windows.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`window.float_on_top.toggle`](#float-on-top), [`window.float_on_top.off`](#windowfloat_on_topoff), or Settings → Appearance floating player windows on top, importing a settings file or resetting every setting.

#### window.float_on_top_while_playing.on

Player windows began to float above other apps' windows while a file in them plays.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`window.float_on_top_while_playing.toggle`](#float-on-top-while-playing), [`window.float_on_top_while_playing.on`](#windowfloat_on_top_while_playingon), or Settings → Appearance floating player windows on top, importing a settings file or resetting every setting.

#### window.float_on_top_while_playing.off

Player windows stopped floating above other apps' windows while a file in them plays.

- Heard by those listening to the app's events.
- Payload: none.
- Caused by: [`window.float_on_top_while_playing.toggle`](#float-on-top-while-playing), [`window.float_on_top_while_playing.off`](#windowfloat_on_top_while_playingoff), or Settings → Appearance floating player windows on top, importing a settings file or resetting every setting.

#### window.layout_changed

Blocks came into a window, went, or changed places, or one filled it or stopped filling it.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`: its window's.
  - `blocks`: The ids of its blocks, in the order its grid shows them.
  - `maximized`: The id of the block filling it; none if none does.
- Caused by: [`app.open`](#open-paths-or-urls), [`block.close`](#close-file), [`block.close_all`](#close-all-files), [`block.close_others`](#close-other-files), [`block.move.left`](#move-block-left), [`block.move.right`](#move-block-right), [`block.move.up`](#move-block-up), [`block.move.down`](#move-block-down), [`block.zoom`](#maximize-block), [`block.move`](#move-block-to), [`block.move_to_window`](#move-block-to-window), [`block.move_to_new_window`](#move-block-to-new-window), or files dropped on a window or picked when `window.open_files` or `window.open_url` asks.

## Casting

### Casting commands

#### List Devices

`cast.ls`: List the TVs and speakers found on the network to play on, by how they're reached.

- Acts on the app.
- Arguments: none.
- Returns: {Chromecast: [{id, name, shows}], AirPlay: […], DLNA: […]}: each protocol's devices found, by name, shows being video for a TV and sound for a speaker; a protocol with none found isn't there.
- Events: none.

#### Play On

`cast.to`: Play the block on a TV or speaker found on the network, from where it is.

- Acts on a block.
- Arguments:
  - `device` (text): The device's id.
  - `kind` (text, optional): How it's reached, such as Chromecast, AirPlay or DLNA, for an id two share.
- Returns: nothing.
- Events: [`cast.started`](#caststarted).

#### Play Here

`cast.stop`: Stop casting the block, and play it here from where the device got to.

- Acts on a block.
- Arguments: none.
- Returns: nothing.
- Events: [`cast.ended`](#castended).

#### Pair and Play On

`cast.pair`: Pair with a TV that shows a PIN to type, such as an Apple TV, then play the block on it.

- Acts on a block.
- Arguments:
  - `device` (text): The device's id.
  - `kind` (text, optional): How it's reached, such as Chromecast, AirPlay or DLNA, for an id two share.
- Returns: nothing.
- Events: none.

### Casting events

#### cast.devices_changed

TVs or speakers to cast to were found on the network, or went.

- Heard by those listening to the app's events.
- Payload:
  - `devices`: Those found now, as `cast.ls` lists them.
- Caused by: devices found on the network or gone from it.

#### cast.started

A block began playing on a TV or speaker.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `device`: The device: its id, kind and name.
- Caused by: [`cast.to`](#play-on), or a TV paired with `cast.pair` once its PIN is typed.

#### cast.ended

A block stopped playing on a TV or speaker.

- Heard by those listening to its window's events.
- Payload:
  - `window_id`, `window_title`, `block_id`, `block_title`: its window's and its block's.
  - `device`: The device it played on: its id, kind and name.
- Caused by: [`cast.stop`](#play-here), or the device stopping or leaving the network.

## Audio output

### Audio output commands

#### List Outputs

`output.ls`: List the outputs connected, the one picked, and where the sound goes now.

- Acts on the app.
- Arguments: none.
- Returns: {outputs: [{id, name, kind}], device: {id, name} or null, route}: the outputs connected (kind AirPlay for a speaker on the network, else null), the one picked (null for the system's default), and the route: {playing: {device, rate, channels, instead_of}} (instead_of naming the one picked, when the system's default stands in for it), {waiting: name}, {opening: name} or {unavailable: why}.
- Events: none.

#### Output Device

`output.set_device`: Play the app's sound through an output, or the system's default. A Bluetooth one paired and not connected is connected.

- Acts on the app.
- Arguments:
  - `id` (text, optional): The output's id; none for the system's default, or for a block, the app's output.
  - `name` (text, optional): What it's called, for an output not connected now, which is waited for.
- Returns: nothing.
- Events: [`output.route_changed`](#outputroute_changed), [`app.settings.changed`](#appsettingschanged).

#### Block Output Device

`output.set_block_device`: Play the block's sound through an output of its own, or through the app's again.

- Acts on a block.
- Arguments:
  - `id` (text, optional): The output's id; none for the system's default, or for a block, the app's output.
  - `name` (text, optional): What it's called, for an output not connected now, which is waited for.
- Returns: nothing.
- Events: none.

### Audio output events

#### output.devices_changed

Outputs were connected or disconnected.

- Heard by those listening to the app's events.
- Payload:
  - `outputs`: Those connected now, as `output.ls` lists them.
- Caused by: an output connected or disconnected.

#### output.route_changed

The app's sound goes elsewhere.

- Heard by those listening to the app's events.
- Payload:
  - `route`: Where it goes now, as `output.ls` says.
- Caused by: [`output.set_device`](#output-device), or the system's own output changing.

## Displays

### Display commands

#### List Displays

`display.ls`: List the displays connected, in the system's order.

- Acts on the app.
- Arguments: none.
- Returns: [{id, name, bounds: {x, y, width, height}}]: each display's id, as window.show_on_display and window.inspect have it, its name, and where it is, in points.
- Events: none.

## Settings

### Settings commands

#### Settings…

`app.settings.open`: Open the Settings window, or bring it to the front.

- Acts on the app.
- Keys: ⌘,
- Arguments: none.
- Returns: nothing.
- Events: none.

#### List Settings

`app.settings.ls`: List every setting: its key, what Settings calls it and the pane it's on, what it takes, and its value.

- Acts on the app.
- Arguments: none.
- Returns: [{key, title, pane, kind, value, default}], in the order Settings has them: kind is toggle, choice (with choices: [{value, label}]), number (with min and max), font, folder, output, keys or languages (a list, by name or code, kept by code).
- Events: none.

#### Get Setting

`app.settings.get`: Read a setting by its key, such as subtitles.size.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
- Returns: Its value, as the settings file keeps it.
- Events: none.

#### List Shortcuts

`app.settings.keymap`: List each command keys can run, with the keys that run it now, as Settings → Hotkeys has them.

- Acts on the app.
- Arguments: none.
- Returns: [{command, title, keys, shown}]: each command's id and title, its keys as the settings file keeps them, and as the menus show them; none for a command with no keys.
- Events: none.

#### Change Setting

`app.settings.set`: Change a setting by its key, to a value Settings offers, and save it.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
  - `value` (any value): What to set it to, as the settings file keeps it: true, 0.5, "dark".
- Returns: Its value now.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.settings.keymap_changed`](#appsettingskeymap_changed), [`app.notes.off`](#appnotesoff), [`window.notes.off`](#windownotesoff), [`block.notes.off`](#blocknotesoff).

#### Reset Setting

`app.settings.reset`: Put a setting back as Plays starts it out, and save it.

- Acts on the app.
- Arguments:
  - `key` (text): The setting's key, its path in the settings file: autoplay, subtitles.size.
- Returns: Its value now.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.settings.keymap_changed`](#appsettingskeymap_changed), [`app.notes.on`](#appnoteson), [`window.notes.on`](#windownoteson), [`block.notes.on`](#blocknoteson).

#### Show Settings File

`app.settings.reveal_file`: Show the settings file where the system shows files, saving it first if it isn't there yet.

- Acts on the app.
- Arguments: none.
- Returns: The file's path.
- Events: none.

#### Reload Settings

`app.settings.reload`: Read the settings file again, after it was changed by hand.

- Acts on the app.
- Arguments: none.
- Returns: [key]: the settings the file changed.
- Events: [`app.settings.changed`](#appsettingschanged), [`app.settings.keymap_changed`](#appsettingskeymap_changed), [`app.notes.off`](#appnotesoff), [`window.notes.off`](#windownotesoff), [`block.notes.off`](#blocknotesoff).

### Settings events

#### app.settings.changed

Settings changed.

- Heard by those listening to the app's events.
- Payload:
  - `keys`: The keys of those that changed, as `app.settings.get` takes them.
- Caused by: [`app.ui.toggle`](#showhide-all-in-every-window), [`app.ui.on`](#appuion), [`app.ui.off`](#appuioff), [`app.controls.toggle`](#showhide-controls-in-every-window), [`app.controls.on`](#appcontrolson), [`app.controls.off`](#appcontrolsoff), [`app.top_bar.toggle`](#showhide-top-bar-in-every-window), [`app.top_bar.on`](#apptop_baron), [`app.top_bar.off`](#apptop_baroff), [`app.track_switcher.toggle`](#showhide-track-switcher-in-every-window), [`app.track_switcher.on`](#apptrack_switcheron), [`app.track_switcher.off`](#apptrack_switcheroff), [`app.notes.toggle`](#showhide-notes-in-every-window), [`app.notes.on`](#appnoteson), [`app.notes.off`](#appnotesoff), [`window.float_on_top.toggle`](#float-on-top), [`window.float_on_top.on`](#windowfloat_on_topon), [`window.float_on_top.off`](#windowfloat_on_topoff), [`window.float_on_top_while_playing.toggle`](#float-on-top-while-playing), [`window.float_on_top_while_playing.on`](#windowfloat_on_top_while_playingon), [`window.float_on_top_while_playing.off`](#windowfloat_on_top_while_playingoff), [`block.subtitles.bigger`](#bigger-subtitles), [`block.subtitles.smaller`](#smaller-subtitles), [`block.subtitles.up`](#move-subtitles-up), [`block.subtitles.down`](#move-subtitles-down), [`output.set_device`](#output-device), [`app.settings.set`](#change-setting), [`app.settings.reset`](#reset-setting), [`app.settings.reload`](#reload-settings), or Settings moving a slider, importing a settings file or resetting every setting.

#### app.settings.keymap_changed

The keys that run commands changed.

- Heard by those listening to the app's events.
- Payload:
  - `commands`: The ids of the commands whose keys changed.
- Caused by: [`app.settings.set`](#change-setting), [`app.settings.reset`](#reset-setting), [`app.settings.reload`](#reload-settings), or Settings importing a settings file or resetting every setting.

## App

### App commands

#### Inspect Plays

`app.inspect`: Say what Plays is: its version, its player windows, and where the sound goes.

- Acts on the app.
- Arguments: none.
- Returns: {version, windows: [as window.ls], output: as output.ls}.
- Events: none.

#### List Formats

`app.formats`: List the formats Plays opens, video and audio.

- Acts on the app.
- Arguments: none.
- Returns: [{name, kind, extensions}]: what each is called, video or audio, and the extensions of its files, lowercase.
- Events: none.

#### Open Paths or URLs

`app.open`: Play files, the media files in folders, and streams, in the window named, or the one in front, or a new one if none is open: each in a block, or one after another in one, as Settings say.

- Acts on a player window.
- Arguments:
  - `urls` (a list of text): Each a file's or folder's full path, a file:// URL, or a stream's URL.
  - `autoplay` (true or false, optional): Whether each plays as it opens, or opens paused: as Settings say if not given.
  - `several_files` (one of together, playlist, optional): Several, or a folder's, each in a block of its own (together), or in one playing them one after another (playlist): as Settings say if not given.
  - `kind` (one of video, audio, optional): Only the video files in folders, or only the sound files: all their media files if not given.
- Returns: {window}: the id of the window they open in.
- Events: [`window.created`](#windowcreated), [`window.active_changed`](#windowactive_changed), [`block.created`](#blockcreated), [`window.layout_changed`](#windowlayout_changed), [`block.active_changed`](#blockactive_changed), [`block.playback.opened`](#blockplaybackopened).

#### Clear Recent Files

`app.clear_recent_files`: Forget the files played lately, which File → Open Recent and Go to… offer.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### About Plays

`app.about`: Show Plays' version, and what it's made with.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Make Plays the Default Player…

`app.default_player`: Open Settings at Default Player, to pick the formats Plays opens when their files are opened: from Finder or a file manager, say.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Check for Updates…

`app.check_for_updates`: Open Settings at About, and check for a newer Plays: one found downloads in the background, to install as Plays restarts or quits.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Restart to Update

`app.restart_to_update`: Install the update downloaded, and start Plays again to run it.

- Acts on the app.
- Arguments: none.
- Returns: nothing.
- Events: none.

#### Copy Version Info

`app.copy_version_info`: Copy Plays' version and the day it was built.

- Acts on the app.
- Arguments: none.
- Returns: What it copied, such as Plays 0.1.0 (18 November 2026).
- Events: none.

#### Copy Debug Info

`app.copy_debug_info`: Copy what a bug report needs: Plays' version, the system, FFmpeg's version, where the sound goes, the windows and blocks, and the settings changed.

- Acts on the app.
- Arguments: none.
- Returns: What it copied, a line for each thing.
- Events: none.

#### Quit Plays

`app.quit`: Quit, keeping where each file got to and stopping the devices cast to.

- Acts on the app.
- Keys: ⌘Q
- Arguments: none.
- Returns: nothing.
- Events: [`block.closed`](#blockclosed), [`window.closed`](#windowclosed), [`window.active_changed`](#windowactive_changed), [`app.quit`](#appquit).

### App events

#### app.command_run

A command is about to run: told before anything it changes.

- Heard by those listening to the app's events.
- Payload:
  - `command`: Its id.
  - `source`: Where it was run from: action (a key, a menu item, or a control that sends one), ui (a control that runs it itself), palette, or cli.
  - `args`, only sometimes: The arguments it was given, if any.
  - `target`, only sometimes: The window and the block it was aimed at, by name, if either was.
- Caused by: every command, as it runs.

#### app.client_connected

A client connected to the socket, as the command line does.

- Heard by those listening to the app's events.
- Payload:
  - `client_id`: A number for it, which its `app.client_left` has too.
- Caused by: a client connecting.

#### app.client_left

A client went from the socket.

- Heard by those listening to the app's events.
- Payload:
  - `client_id`: The number its `app.client_connected` had.
- Caused by: a client going or being cut off, as when control from the command line is turned off.

#### app.quit

Plays is quitting: told last, after each window still open is told closing.

- Heard by every listener.
- Payload: none.
- Caused by: [`app.quit`](#appquit), or Plays quitting from the Dock or with the system.

## Command line

### Command-line commands

#### Install Command Line Tool

`cli.install`: Link plays into /usr/local/bin, so a terminal can run it: to open files, run commands and hear what changes.

- Acts on the app.
- Arguments: none.
- Returns: The link's path.
- Events: none.
