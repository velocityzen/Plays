# Lua scripts

`plays do` runs a Lua 5.4 script, a client of the Plays running, as the
rest of the command line is: once, top to bottom, and it ends, unless it
has handlers, `plays.on`, or timers to run on with
([lua-events.md](lua-events.md)).

```lua
-- louder.lua: every block in the window louder by 10
for _, block in ipairs(plays.block.ls()) do
  local volume = plays.block.inspect{block_id = block.id}.volume
  plays.audio.set_volume{block_id = block.id, volume = math.min(volume + 10, 100)}
end
```

```bash
plays do louder.lua
plays -w Kitchen do louder.lua   # the window labeled Kitchen
```

## Running a script

```bash
plays do louder.lua                         # a file: ends in .lua, or has a slash
plays do -e 'return plays.window.ls()'      # code
echo 'print(plays.window_id)' | plays do -  # stdin
plays do solo.lua --for 10m --fail-fast     # run on 10m, end at a failure
```

- `plays.window_id`, `plays.block_id`: those `-w` and `-b` name, else the
  window in front and the block keys go to; nil where there's none.
- `require`: modules beside the script first.
- Lua's standard library, without `debug`, `io.popen`, `os.execute` and C
  modules; [Programs](#programs) run programs.
- `--for` and `--fail-fast`: see [lua-events.md](lua-events.md#running-on).

## Arguments

```lua
-- seek.lua: every block in the window to a time
for _, block in ipairs(plays.block.ls()) do
  plays.playback.seek{block_id = block.id, time = plays.args.time or 0}
end
```

```bash
plays do seek.lua time=90
plays do seek.lua --args '{"time": 90}'
plays do seek.lua --args @args.json
```

- `plays.args`: `KEY=VALUE` typed as written: `n=2` a number, `on=true`
  true, `name=x` text.
- `--args`: JSON, or a file of it after `@`; for text that looks like a
  number, as `{"name": "2"}`, and for lists and objects.

## What it prints

```bash
plays do -e 'return plays.block.inspect().volume'  # 100.0
plays do -e 'return plays.block.inspect().title'   # "talk.mkv", JSON
plays do -e 'print(plays.block.inspect().title)'   # talk.mkv
```

- What it returns, as JSON (`--json` indented); nothing for nil.
- `print(...)`: a line on stdout. `plays.log(...)`: a line on stderr.
- An error: its message, exit status 1. `plays.stop(status)`: ends it at
  once, with that exit status, 0 if none.
- Exit statuses as for other commands: 3 no such script, 4 no Plays to
  ask, 5 control from the command line off, 6 no answer in time.

## Calling Plays

```lua
plays.playback.seek{time = 90}            -- a command, by its id
plays.call("playback.seek", {time = 90})  -- the same
local block = plays.block.inspect()
print(block.title, block.position, block.duration)
```

- Every command in [commands.md](commands.md), its arguments a table.
- On the script's window, or its block, as the command acts on one.
  `window_id` and `block_id` among its arguments name another, by id,
  label or number.
- What the command hands back, alone. `plays.null`: null, to send one.
- One that fails hands back nil and why, and the script goes on, or
  `assert` ends it there. `plays.wait`, `plays.exec` and `plays.json`
  hand back the same way.

```lua
local window, why = plays.window.inspect{window_id = "Kitchen"}
if not window then plays.log(why) end  -- no window Kitchen
window = assert(plays.window.inspect{window_id = "Kitchen"})
```

## Sleeping and waiting

```lua
-- next.lua: once the block ends, the next file in its folder
plays.wait("playback.ended", {block_id = plays.block_id})
plays.playback.next_file()
```

- `plays.sleep(seconds)`: waits that long.
- `plays.wait(event, filter, timeout)`: the payload of the next event of
  that type whose fields equal the filter's, numbers by their values. A
  function filter matches when it returns true. Events and their fields
  are in [commands.md](commands.md#events).
- A cue's, `block.<block>.position@120.5s` or `…@345f`: the block
  reaching that time or frame ([lua-events.md](lua-events.md#cues)).
- nil and "timeout" once `timeout` seconds are up; nil and why if Plays
  goes. No timeout waits on.
- What it hears: what happens from the start of the last call before it,
  what that call does too; in a handler, the last in it. With none, from
  the wait's start, or in a handler from its event. Each event once, in
  order, those it passes over gone, for handlers too.
- Everything's events; with `-w`, that window's.

## Timers

```lua
local ticking = plays.every(5, function()
  plays.log(plays.block.inspect().position) -- where it is, every 5 seconds
end)
plays.after(60, function() ticking:cancel() end)
```

- `plays.after(seconds, fn)`: runs it once the seconds are up.
  `plays.every(seconds, fn)`: every so many.
- Each returns a timer, stopped by `:cancel()`; dropped, it runs on.
- Run once the first run is over, never at once with other Lua: one due
  as a handler runs waits for it, and a tick missed meanwhile runs once.
- Calls in one act on the script's window and block, though a handler
  started it.
- At most 1,024 going; more stops the script, exit status 1.

## Programs

```lua
-- newest.lua: the newest file in the folder `folder` names, opened
local listed = plays.exec({"ls", "-t"}, {cwd = plays.args.folder})
local newest = listed and listed.ok and listed.stdout:match("[^\n]+")
if newest then plays.app.open{urls = {plays.args.folder .. "/" .. newest}} end
```

```lua
local counted = plays.exec{"sh", "-c", "ls *.mkv | wc -l"} -- a shell, to have one
plays.exec.start({"make"}, {stdout = io.write}):wait()     -- its output as it comes
```

- `plays.exec{program, arg...}`: runs it, with no shell, and waits: `ok`,
  `code` (nil if a signal ended it), `stdout`, `stderr`. nil and why only
  if it can't run; one that fails is `ok = false`.
- Options, a second table: `cwd`; `env`, as `{CI = "1"}`, `false`
  removing one; `timeout`, in seconds, then it's killed.
- `plays.exec.start`: starts it; `:wait()` waits as `plays.exec` does.
  Options too: `stdout`, `stderr`, a function each piece of output goes
  to as it comes, not kept; `detach`.
- Those functions run as the script waits for a program, or runs on;
  never at once with other Lua.
- Stopped as the part of the script that started it ends: its top level,
  a handler or a timer. Detached, as the script ends.

## JSON and Lua

- As calls and what they hand back convert them, so do
  `plays.json.encode(value, {indent = true})`, indented by two spaces, and
  `plays.json.decode(text)`; nil and why if they can't.
- null is nil.
- An object or a list is a table. A list from Plays stays one, empty too;
  another table is a list if it has items from 1, else an object: `{}` is
  `{}`.
- Integers stay integers; other numbers are floats.

## Examples

```lua
-- same-time.lua: every block in the window to the same time, then where
-- each is
for _, block in ipairs(plays.block.ls()) do
  plays.playback.seek{block_id = block.id, time = plays.args.time}
end
local where = {}
for _, block in ipairs(plays.block.ls()) do
  where[#where + 1] = plays.block.inspect{block_id = block.id}.position
end
return where
```

```lua
-- where.lua: where each block is, as JSON in the file `to` names, now
-- and every 5 seconds
local function save()
  local where = {}
  for _, block in ipairs(plays.block.ls()) do
    local position = plays.block.inspect{block_id = block.id}.position
    where[#where + 1] = {title = block.title, position = position}
  end
  local file = assert(io.open(plays.args.to, "w"))
  file:write(plays.json.encode(where, {indent = true}))
  file:close()
end
save()
plays.every(5, save)
```

```bash
plays do same-time.lua time=90   # [90.0, 90.0], to the frame
plays do where.lua to=where.json
plays do newest.lua folder=$HOME/Downloads
```
