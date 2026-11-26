# Streams

What the tests play, made with the `ffmpeg` tool unless they're fixtures
(`tests/fixtures`) or built in code.

## Containers

- MP4, M4A, MOV, 3GP
- Matroska, MKA, WebM
- MPEG-TS, M2TS, MPEG-PS (VOB, MPG)
- AVI, FLV, ASF (WMV, WMA), MXF
- Ogg, WAV, AIFF, FLAC, WavPack, MP3, MP2, ADTS AAC
- A file written through a pipe, without a duration

## Video

| Codec | Kinds | In |
| --- | --- | --- |
| H.264 | Constrained Baseline, High, High 10, High 4:2:2, High 4:4:4 | MP4, MOV, Matroska, MPEG-TS, HLS |
| HEVC | Main, Main 10, Main 12, 4:2:2 10-bit | MP4, Matroska, MPEG-TS |
| AV1 | Main, 8- and 10-bit | MP4, Matroska, WebM |
| VP9 | Profiles 0, 1 (4:4:4) and 2 (10-bit) | WebM |
| VP8 | | WebM |
| MPEG-4 Part 2 | Simple, as Xvid | AVI |
| MPEG-2 | Main | VOB, M2TS |
| MPEG-1 | | MPG |
| MJPEG | Full range 4:2:2 | MOV |
| ProRes | 422, 4444 with alpha | MOV |
| DNxHR | LB, BT.709 | MXF |
| H.263 | | 3GP |
| Sorenson Spark | | FLV |
| WMV 8 | | WMV |
| FFV1 | 10-bit, colours without names (fixture) | Matroska |

- Pixel formats: yuv420p, yuvj420p, yuv422p, yuvj422p, yuv444p,
  yuv420p10le, yuv422p10le, yuv420p12le, yuva444p12le; NV12, P010 and BGRA
  in code
- Colours: untagged (SD as BT.601, HD as BT.709), BT.709, BT.601, full
  range
- Sizes: 160×120 to 1280×720, 3840×2160, odd (159×119)
- Pixel shapes: 4:3 in the stream, 2:1 in a Matroska file over it,
  H.263's 12:11 in a square container
- Rotation and flip, as phones' display matrix
- Frame rates: 10, 24000/1001, 25, 30, 50, 60, 120, variable
- Interlaced H.264, top field first
- Cover art: attached PNG, PNG, JPG and TIFF beside the file

## HDR

- HDR10: PQ, limited and full range, mastering display, MaxCLL
- HLG
- Dolby Vision profile 7: base and enhancement layers in one track or two,
  or the layer missing, composed in software and on the GPU
- Dolby Vision profile 5's reshaping, in code
- Dolby Vision profile 20: IPT tagged IPT-C2 and PQ, reshaped, decoded on
  the GPU and in software
- Tone mapping and PQ/HLG palettes, in code

## Audio

| Codec | Kinds | In |
| --- | --- | --- |
| AAC LC | Mono, stereo, 5.1, 7.1; channels changing mid-stream | MP4, M4A, Matroska, MPEG-TS, ADTS, HLS |
| AC-3, E-AC-3 | 5.1 (fixtures) | Matroska |
| DTS | 5.1 | MKA |
| TrueHD | 5.1 | MKA |
| Opus | Mono, stereo, 5.1; a gap (fixture) | Ogg, Matroska, MKA, WebM |
| Vorbis | Stereo | Ogg |
| MP3 | 22.05 and 44.1 kHz | MP3, AVI, FLV |
| MP2 | 32 and 48 kHz | MP2, MPG, M2TS |
| FLAC | 44.1, 88.2 and 96 kHz, 24-bit; the rate changing mid-stream | FLAC |
| ALAC | Stereo | M4A |
| WMA 2 | Stereo | WMA, WMV |
| WavPack | | WV |
| PCM | s16 LE and BE, s24, s32, f32, u8; 8 to 192 kHz; channels out of order | WAV, AIFF, MOV, Matroska |

- Surround mixed down to stereo
- Tags in UTF-8 and Windows-1252

## Subtitles

- SubRip: in Matroska and beside the video, UTF-8 and Windows-1251, its
  markup
- WebVTT: beside the video, placed by its line; in Matroska and WebM
- MP4 timed text
- ASS and SSA: beside the video and in Matroska, styles, placement,
  karaoke, drawings skipped
- VobSub, in Matroska and VOB
- DVB, in MPEG-TS

## Network

- HTTP, with and without ranges, stalling, failing
- HLS: VOD of MPEG-TS and of fragmented MP4 segments, live with a sliding
  window, master playlists with variants, redirects and missing variants

## Casting

- Files as they are: MP4, M4A, Matroska
- HLS of MPEG-TS and fragmented MP4: H.264 copied or re-encoded, FFV1 to
  HEVC 10-bit, AC-3 and E-AC-3 copied, sound to AAC or AC-3, delayed,
  live; WebVTT renditions; picture subtitles drawn in; turned, cropped and
  gamma
- MPEG-TS from a time (DLNA), with SubRip
- Sound alone: ADTS AAC, FLAC, HLS
- AirPlay sound: 16-bit stereo at 44.1 kHz

## Checked by hand

Samples too big to commit, in `ignore/`:

- HEVC Main 10 HLG, 4K60, Dolby Vision 8.4, AAC (MOV)
- HEVC 4K Dolby Vision 7 FEL: Matroska; MP4 with AC-3 5.1; DTS 5.1
- HEVC Dolby Vision 8.1, 3840×2014
- HEVC Dolby Vision 20 (MV-HEVC), 24 fps: its first view, decoded in
  software, as VideoToolbox won't take it
- HEVC Dolby Vision 5, 4K 59.94, E-AC-3 stereo
- AV1 4K PQ, Dolby Vision 10, DTS-HD MA 7.1
- AV1 Dolby Vision 10, 3840×1920, E-AC-3 Atmos 5.1, MJPEG cover
- AV1 8K60 PQ, Opus stereo
- AAC LC 44.1 kHz stereo (M4A)

## Not tested

- PGS, CEA-608 and 708 captions: no encoder makes them
- HDR10+
- Theora, VC-1
- DASH, encrypted HLS, HTTPS, RTSP, RTMP
- Hardware decoding off macOS, NVENC and VA-API encoding
