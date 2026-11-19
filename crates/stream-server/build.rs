//! Links the static FFmpeg into this crate's tests, as Plays' build script
//! does for the app.

fn main() {
    ffmpeg_link::link();
}
