use std::thread;

use ffmpeg_next::Error;

use super::init;

#[test]
fn set_up_from_many_threads_at_once() {
    let threads: Vec<_> = (0..8).map(|_| thread::spawn(init)).collect();
    for thread in threads {
        assert!(thread.join().unwrap().is_ok());
    }
    assert!(init().is_ok(), "and again after");
    // FFmpeg's own errors, named.
    assert_eq!(
        Error::InvalidData.to_string(),
        "Invalid data found when processing input"
    );
}
