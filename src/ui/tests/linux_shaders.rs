//! gpui's shaders for Linux, which Plays changes too: nothing building on
//! macOS reads them, gpui's build script only parses them on Linux, and
//! the GPU's driver takes only valid ones.

use std::fs;
use std::path::Path;

use naga::valid::{Capabilities, ValidationFlags, Validator};

/// Valid as Blade, gpui's renderer there, takes them: their bindings are
/// set as its pipelines are made, and it asks for no more of the GPU.
#[test]
fn gpuis_shaders_for_linux_are_valid() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/gpui/src/platform/blade/shaders.wgsl");
    let source = fs::read_to_string(&path).unwrap();
    let module = naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|err| panic!("{}", err.emit_to_string(&source)));
    Validator::new(
        ValidationFlags::all() ^ ValidationFlags::BINDINGS,
        Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|err| panic!("{}", err.emit_to_string(&source)));
}
