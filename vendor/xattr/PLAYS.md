# xattr 0.2.3, patched for Plays

This is xattr 0.2.3 from crates.io, without its tests, which gpui reaches
through `gpui_http_client` and `zed-async-tar`. Plays uses it through
`[patch.crates-io]` in its Cargo.toml.

## What changed

`src/sys/mod.rs`: on Linux and Android, `ENOATTR` is `libc::ENODATA`.
libc no longer defines `ENOATTR` on Linux (0.2.191 doesn't), so xattr 0.2.3
doesn't build there with it, and tokio's mio needs a libc that new. xattr 1.x
fixes it, but `zed-async-tar` asks for 0.2.

`src/sys/linux_macos/macos.rs`: `u32` instead of libc's deprecated
`uint32_t`, which is the same type, as warnings show for a patched crate.

Drop this copy once gpui's dependencies move to xattr 1.x.
