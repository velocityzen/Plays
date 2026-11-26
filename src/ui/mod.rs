pub mod about;
mod ass;
pub mod blackout;
pub mod components;
#[cfg(test)]
#[path = "tests/desktop_file.rs"]
mod desktop_file;
pub mod displays;
pub mod framing;
#[cfg(debug_assertions)]
pub mod gallery;
#[cfg(test)]
#[path = "tests/linux_shaders.rs"]
mod linux_shaders;
pub mod markdown;
pub mod name_sheet;
pub mod open_url;
mod overlay;
pub mod pairing;
pub mod palette;
pub mod pip;
pub mod player;
pub mod screenshot;
pub mod sentence;
pub mod settings;
mod subtitles;
mod system_colors;
#[cfg(test)]
pub mod testing;
pub mod text_input;
pub mod theme;
mod traffic_lights;
pub mod workspace;

/// The windows' app_id on Wayland and WM_CLASS on X11, by which Linux
/// finds their desktop file, `resources/linux/plays.desktop`, and its icon.
pub const APP_ID: &str = "plays";
