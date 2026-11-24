# block 0.1.6, patched for Plays

This is block 0.1.6 from crates.io, without its tests, which gpui, cocoa,
metal and other macOS crates use for Objective-C blocks. Plays uses it
through `[patch.crates-io]` in its Cargo.toml.

## What changed

`src/lib.rs`: `Class`, the type of `_NSConcreteStackBlock`, is an opaque
struct, not an empty enum. Rust is phasing out statics of an uninhabited
type, and warned of it on every build; only the static's address is taken.
`extern` says its ABI, "C", which it was without saying, as Rust warns for a
patched crate.

Drop this copy once gpui and the crates it uses move to objc2's `block2`.
