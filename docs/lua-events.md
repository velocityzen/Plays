# Events in Lua scripts

A script's handlers run as things happen in Plays, each for an event of
its type, once its first run is over ([lua.md](lua.md)).

```lua
-- solo.lua: as a block plays, the others in its window pause
plays.on("playback.playing", function(ctx, event)
  for _, block in ipairs(plays.block.ls()) do
    if block.id ~= ctx.block_id then
      plays.playback.pause{block_id = block.id}
    end
  end
end)
```

```bash
plays do solo.lua   # listening: 1 handler; Ctrl-C to stop
```

## Handlers

- `plays.on(event, function(ctx, event))`: registered as the script first
  runs; `plays.on` in a handler is an error.
- Run once the first run is over, one at a time, for each event heard
  from the script's first call or wait on, or else from then, in order.
- `ctx.window_id`, `ctx.block_id`: the event's; calls in a handler act on
  them. `event`: its payload.
- A wait or a sleep in a handler holds the others; a timer doesn't (see
  [Timers in handlers](#timers-in-handlers)).

## Events

- Every event in [commands.md](commands.md#events), by its type, as
  `playback.ended` or `window.closed`, and the fields of its payload.
- Types aren't checked: a handler for one misspelled never runs.
- `playback.position`, where a file is as it plays, is heard once a wait
  or a handler is for it, at most once a second.
- Cues, a block reaching a time or a frame, by their names (see
  [Cues](#cues)).
- Everything's events; with `-w`, that window's.

## Cues

```lua
-- stop-at.lua: the block plays on to the frame `frame` names, and pauses
local cue = ("block.%s.position@%df"):format(plays.block_id, plays.args.frame)
plays.on(cue, function(ctx, event)
  plays.playback.pause()
  plays.log(("paused at frame %d, %.3fs"):format(event.frame, event.time))
end)
```

```bash
plays -b 2 do stop-at.lua frame=345
```

- `block.<block>.position@120.5s`, or `…@345f`: a block reaching a time,
  in seconds, or a frame, from 0, as `frame` counts them. The block by
  id, label or number, as `-b` names it. Wherever an event's type goes:
  `plays.wait`, `plays.on`, `plays wait`, `plays events --cue`.
- Heard once, in whatever window, as the block plays on to it or past it,
  or lands on it, within a frame: a seek, a step, the next loop. Not a
  seek past it. Set past it, once the block is back before it, playing on.
- Its payload: `playback.position`'s, told as the block ticks there, not
  once a second.
- A handler for one runs once, then is gone; as its block closes, it's
  gone unrun, said on stderr. A wait for one fails as its block closes.
- Refused: a block that isn't there, and a frame in sound alone.

## Running on

- A script with handlers or timers runs on once its first run is over,
  until `plays.stop`, `--for` is up (`90`, `90s`, `10m`, `1.5h`), Ctrl-C
  (exit status 130), or, with no handlers, no timer is left. With
  handlers, until Plays quits or the window `-w` names closes too.
- `plays.on_stop(fn)` runs as it stops ([lua.md](lua.md#stopping)).
- Says what it runs on stderr: `listening: 2 handlers, running: 1 timer;
  Ctrl-C to stop`.

```lua
-- failed.lua: at the first file that fails, why, and exit status 1
plays.on("playback.failed", function(ctx, event)
  plays.log(event.block_title .. ": " .. event.error)
  plays.stop(1)
end)
```

## Failures

- A handler, a timer or a program's function failing is said on stderr,
  and the script goes on; with `--fail-fast` it ends, exit status 1.
- A call that fails isn't one: it hands back nil and why
  ([lua.md](lua.md#calling-plays)).

## Handlers or waits

- `plays.wait`: one event, as the script goes on from it: open a file,
  then wait for its end.
- `plays.on`: every event of a type, for as long as the script runs on.

## Timers in handlers

```lua
-- settled.lua: a window's blocks, once they've stopped moving for half
-- a second
local settling = {}
plays.on("window.layout_changed", function(ctx, event)
  if settling[ctx.window_id] then settling[ctx.window_id]:cancel() end
  settling[ctx.window_id] = plays.after(0.5, function()
    settling[ctx.window_id] = nil
    print(plays.json.encode(plays.block.ls{window_id = ctx.window_id}))
  end)
end)
```

- `plays.after` in a handler: once events settle, not at each; the
  handler goes on at once.
- Calls in a timer act on the script's window and block, though a
  handler started it: name the event's, as `{window_id = ctx.window_id}`.

## Examples

```lua
-- opened.lua: each file opened, a line in the file `to` names
local log = assert(io.open(plays.args.to, "a"))
plays.on("playback.opened", function(ctx, event)
  log:write(os.date("%Y-%m-%d %H:%M:%S "), event.file or event.url, "\n")
  log:flush()
end)
```

```lua
-- ended.lua: on macOS, a notification as each file ends
plays.on("playback.ended", function(ctx, event)
  plays.exec{"osascript", "-e", "on run argv",
    "-e", 'display notification (item 1 of argv) with title "Plays"',
    "-e", "end run", event.block_title}
end)
```

```bash
plays do opened.lua to=$HOME/plays.log --for 8h
plays do ended.lua
```
