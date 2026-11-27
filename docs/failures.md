# Failures

What fails, or plays with less, because of a platform, a device or a
protocol rather than Plays. What Plays could do and doesn't yet is in the
README's [Limitations](../README.md#limitations).

## Wayland

- Windows can't float on top or be placed: Wayland leaves both to the
  compositor.
- Full screen on another display is up to the compositor.

## AirPlay

- AirPlay 1 speakers with FairPlay or MFi can't be played through: both
  need Apple's own keys or chip.
- Speakers and TVs that let only their Home play to them can't be played
  through: Plays isn't in the Home.
- TVs with AirPlay built in, other than Apple TVs, get H.264 up to 1080p
  and stereo AAC, their HDR fitted to SDR: their AirPlay player may show
  no HEVC, as a VisionMaster projector's shows none.

## DLNA

- DLNA has no way to name HDR: it reaches a TV only where the TV names no
  codecs and AirPlay at its address lists its screen's HDR.
- DLNA has no authentication: a device on the network can take another's
  name. Devices of one protocol sharing a name show their addresses.

## Any cast device

- Speed and frame steps don't apply while a block casts: the device plays
  on its own clock.
- A device that takes only files gets them as they are: without delays,
  picture adjustments or picture subtitles.
- A device that decodes nothing this computer encodes gets video as it is:
  without picture adjustments or picture subtitles, and in MPEG-TS a
  phone's video sideways, as MPEG-TS can't say it's turned.

## Prompts

- The firewall may ask about Plays as it casts: the stream server listens
  on every interface.
- On macOS 15 and later, looking for devices asks to use the local
  network.
