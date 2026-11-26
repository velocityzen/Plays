-- slideshow.lua: the videos in a folder, in a new window with nothing over
-- them, playing one at a time for `seconds` each, in turn, round and
-- round, a cue telling when each turn is up. Each loops, so one that
-- ends in its turn starts over.
--
--   plays do slideshow.lua folder=~/Movies/loops
--   plays do slideshow.lua folder=. seconds=5 full_screen=true --for 8h
--
-- Ctrl-C stops it, as closing its window does: its videos paused, and
-- out of full screen.

local folder = plays.args.folder
local seconds = plays.args.seconds or 10
assert(type(folder) == "string", "give the folder: folder=/path/to/videos")
assert(type(seconds) == "number" and seconds > 0, "seconds: a number above 0")

-- A call that fails hands back nil and why; one with nothing to hand
-- back, nil alone. `must` ends the script at a failure, saying why.
local function must(value, why)
  if why ~= nil then error(why, 0) end
  return value
end

-- The folder's full path, as app.open takes them.
if folder:sub(1, 1) == "~" then folder = os.getenv("HOME") .. folder:sub(2) end
local pwd, why = plays.exec({"pwd", "-P"}, {cwd = folder})
if not pwd then error(folder .. ": " .. why, 0) end
folder = pwd.stdout:match("[^\n]+")

-- A window of its own, and nothing over the video in it: blocks opened
-- in it start out so.
local window = must(plays.window.new()).window
must(plays.window.set_ui{window_id = window, visible = {}})

-- However it stops: Ctrl-C, --for up, an error, no videos left. The
-- window may be gone, so neither call needs to work.
plays.on_stop(function()
  plays.window.playback.pause{window_id = window}
  plays.window.full_screen.off{window_id = window}
  plays.log("slideshow: stopped")
end)

-- The folder's videos, by name, each in a block of its own, whatever
-- Settings say of several files and Autoplay: paused, looping, and from
-- its start, not where it was left last time. All are in the window by
-- the first one's block.created. The mark: to hear the first open later.
local mark = must(plays.mark())
must(plays.app.open{
  window_id = window, urls = {folder},
  kind = "video", several_files = "together", autoplay = false,
})
if not plays.wait("block.created", {window_id = window}, 10) then
  error(folder .. ": no videos in it", 0)
end
local blocks = {}
for _, listed in ipairs(must(plays.block.ls{window_id = window})) do
  local id = listed.id
  local block = must(plays.block.inspect{block_id = id})
  must(plays.block.playback.loop.on{block_id = id})
  must(plays.block.playback.seek{block_id = id, time = 0})
  blocks[#blocks + 1] = id
end
assert(#blocks > 0, "no video opened")

if plays.args.full_screen then
  must(plays.window.full_screen.on{window_id = window})
end

-- A cue's name: block `id` reaching `time` seconds into its file.
local function cue(id, time)
  return ("block.%s.position@%.3fs"):format(id, time)
end

-- The block's turn: from where it is, `seconds` of its file, then paused.
-- Cues say when: the block reaching the time the turn ends, or, if the
-- file ends first, its start, where looping lands it, each time round. It
-- ends early if the block closes, or is 10 seconds late to a cue: paused
-- by hand, or stalled.
local function play_turn(id, block)
  local at, left = block.position, seconds
  local length = block.duration or 0 -- 0: a stream, which doesn't end
  local frame = 1 / (block.frame_rate or 25)
  local _, gone = plays.block.playback.play{block_id = id}
  if not gone then
    while left >= frame do
      -- Round to the start first, if the turn ends past the last frame.
      local to, span = 0, length - at
      if length == 0 or at + left <= length - frame then to, span = at + left, left end
      if not plays.wait(cue(id, to), nil, span + 10) then break end
      at, left = to, left - span
    end
  end
  plays.block.playback.pause{block_id = id}
end

-- The turns, in order, round and round: a block still opening, or with
-- no video, passes its turn, and one closed, or that can't play, is out.
local turn = 0 -- the place in blocks of the one whose turn it was

local function take_turn()
  for _ = 1, #blocks do
    turn = turn % #blocks + 1
    local id = blocks[turn]
    local block = plays.block.inspect{block_id = id}
    if not block or block.error then
      if block then plays.log(block.title .. ": " .. block.error) end
      table.remove(blocks, turn)
      turn = turn - 1
      if #blocks == 0 then break end
    elseif block.opened and #block.tracks.video > 0 then
      play_turn(id, block)
      return plays.after(0, take_turn)
    end
  end
  if #blocks == 0 then
    plays.log("slideshow: no videos left")
    return plays.stop()
  end
  plays.after(1, take_turn) -- none ready: again in a second
end

-- The first video first: once it's open, or can't be, or in 30 seconds.
plays.wait({"block.playback.opened", "block.playback.open_failed"}, {block_id = blocks[1]}, 30, mark)
plays.log(("slideshow: %d videos from %s, %gs each"):format(#blocks, folder, seconds))
plays.after(0, take_turn)
