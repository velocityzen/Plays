use super::*;

#[test]
fn ffmpeg_is_built_in_a_folder_named_for_its_version() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = fs::read_to_string(root.join("script/build-ffmpeg")).unwrap();
    let version = script
        .lines()
        .find_map(|line| line.strip_prefix("FFMPEG_VERSION="))
        .unwrap();
    let config = fs::read_to_string(root.join(".cargo/config.toml")).unwrap();
    let folder = config
        .lines()
        .find_map(|line| line.strip_prefix("FFMPEG_DIR = { value = \""))
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    assert_eq!(folder, format!("build/ffmpeg-{version}"));
}

#[test]
fn finds_the_runtime_of_a_compiler_named_with_a_wrapper() {
    assert_eq!(runtime_library("env cc"), runtime_library("cc"));
}
