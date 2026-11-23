# Events in Lua scripts

A script's handlers run as things happen in Plays, each for an event of
its type, once its first run is over ([lua.md](lua.md)).

```lua
-- solo.lua: as a block plays, the others in its window pause
plays.on("playback.playing", function(ctx, event)
  for _, block in ipairs(plays.block.ls()) do
    if block.id ~= ctx.block_id and block.playing then
      plays.playback.play_pause{block_id = block.id}
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
- Everything's events; with `-w`, that window's.

## Running on

- A script with handlers or timers runs on once its first run is over,
  until `plays.stop`, `--for` is up (`90`, `90s`, `10m`, `1.5h`), Ctrl-C
  (exit status 130), or, with no handlers, no timer is left. With
  handlers, until Plays quits or the window `-w` names closes too.
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
