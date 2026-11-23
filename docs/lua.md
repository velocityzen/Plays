# Lua scripts

`plays do` runs a Lua 5.4 script, a client of the Plays running, as the
rest of the command line is.

```bash
plays do seek.lua time=90                  # a file: ends in .lua, or has a slash
plays do -e 'return plays.window.ls()'     # code
echo 'print(plays.window_id)' | plays do - # stdin
plays do seek.lua --args '{"times": [10, 20]}'
plays do seek.lua --args @args.json
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
- `require`: modules beside the script first.
- Lua's standard library, without `debug`, `io.popen`, `os.execute` and C
  modules.

## Calls

- On the script's window, or its block, as the command acts on one.
  `window_id` and `block_id` among its arguments name another.
- What the command hands back; a failed one gives nil and why.

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
