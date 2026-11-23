# Lua scripts

`plays do` runs a Lua 5.4 script, a client of the Plays running, as the
rest of the command line is.

```bash
plays do seek.lua time=90                  # a file: ends in .lua, or has a slash
plays do -e 'return plays.window.ls()'     # code
echo 'print(plays.window_id)' | plays do - # stdin
plays do seek.lua --args '{"times": [10, 20]}'
plays do seek.lua --args @args.json
plays do loop.lua --for 10m --fail-fast    # run on 10m, end at a failure
```

## What a script has

- `plays.args`: its arguments. `KEY=VALUE` typed as written: `n=2` a
  number, `on=true` true, `name=x` text. `--args` for JSON.
- `plays.window_id`, `plays.block_id`: those `-w` and `-b` name, else the
  window in front and the block keys go to; nil where there's none.
- `plays.playback.seek{time = 90}`: a command, by its id, as in
  [commands.md](commands.md).
- `plays.call("playback.seek", {time = 90})`: the same.
- `plays.null`: null, to send one.
- `print(...)`: a line on stdout. `plays.log(...)`: a line on stderr.
- `plays.stop(status)`: ends it now, with that exit status, 0 if none.
- `plays.sleep(seconds)`: waits that long.
- `plays.wait(event, filter, timeout)`: waits for an event, as in
  [commands.md](commands.md#events); see [Waiting](#waiting).
- `plays.on(event, function(ctx, event))`: a handler; see
  [Handlers](#handlers).
- `plays.after(seconds, fn)`, `plays.every(seconds, fn)`: timers; see
  [Timers](#timers).
- `require`: modules beside the script first.
- Lua's standard library, without `debug`, `io.popen`, `os.execute` and C
  modules.

## Calls

- On the script's window, or its block, as the command acts on one.
  `window_id` and `block_id` among its arguments name another.
- What the command hands back; a failed one gives nil and why.

## Waiting

- `plays.wait("playback.ended", {block_id = id}, 600)`: the next event of
  that type whose fields equal the filter's, numbers by their values. A
  function filter matches when it returns true.
- What it returns: the event's payload; nil and "timeout" once `timeout`
  seconds are up; nil and why if Plays goes. No timeout waits on.
- What it hears: what happens from the start of the last call before it,
  in a handler the last in it; with none, from the wait's start, or in a
  handler from its event. Each event once, in order, those it passes over
  gone, for handlers too.
- Everything's events; with `-w`, that window's.

## Handlers

```lua
plays.on("playback.ended", function(ctx, event)
  plays.playback.seek{time = 0} -- on the block that ended
end)
```

- Registered as the script first runs; `plays.on` in a handler is an
  error. Event names aren't checked.
- Run once the first run is over, one at a time, for each event heard
  from the script's first call or wait on, or else from then, in order.
- `ctx.window_id`, `ctx.block_id`: the event's; calls in a handler act on
  them.
- A wait or a sleep in a handler holds the others.

## Timers

```lua
local ticking = plays.every(5, function()
  plays.log(plays.block.inspect().position) -- where it is, every 5 seconds
end)
plays.after(60, function() ticking:cancel() end)
```

- Each returns a timer, stopped by `:cancel()`; dropped, it runs on.
- Run once the first run is over, never at once with other Lua: one due
  as a handler runs waits for it, and a tick missed meanwhile runs once.
- Calls in one act on the script's window and block, though a handler
  started it.
- At most 1,024 going; more stops the script, exit status 1.

## Running on

- A script with handlers or timers runs on once its first run is over,
  until `plays.stop`, `--for` is up (`90`, `90s`, `10m`, `1.5h`), Ctrl-C
  (exit status 130), or, with no handlers, no timer is left. With
  handlers, until Plays quits or the window `-w` names closes too.
- Says what it runs on stderr: `listening: 2 handlers, running: 1 timer;
  Ctrl-C to stop`.
- A handler or a timer failing is said on stderr, and the script goes on;
  with `--fail-fast` it ends, exit status 1.

## What it prints

- What it returns, as JSON (`--json` indented); nothing for nil.
- An error: its message, exit status 1.
- Exit statuses as for other commands: 3 no such script, 4 no Plays to
  ask, 5 control from the command line off.

## JSON and Lua

- null is nil.
- An object or a list is a table. A list from Plays stays one, empty too;
  another table is a list if it has items from 1, else an object: `{}` is
  `{}`.
- Integers stay integers; other numbers are floats.

## Example

```lua
-- Every block to the same time, then where each is.
for _, block in ipairs(plays.block.ls()) do
  plays.playback.seek{block_id = block.id, time = plays.args.time}
end
return plays.block.ls()
```
