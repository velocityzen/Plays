use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;

use ffmpeg::media::Type;

use super::*;

thread_local! {
    static ALLOCATED: Cell<usize> = const { Cell::new(0) };
}

/// Counts the fresh allocations each thread makes, to see what is
/// copied: growing what was allocated isn't counted.
struct Counted;

// SAFETY: allocating is left to the system's allocator.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATED.try_with(|allocated| allocated.set(allocated.get() + layout.size()));
        // SAFETY: as the caller asked.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller asked.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: as the caller asked.
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static COUNTED: Counted = Counted;

fn allocated() -> usize {
    ALLOCATED.with(Cell::get)
}

#[test]
fn the_sink_goes_back_to_put_things_right_but_not_into_what_it_handed_over() {
    let mut sink = Sink::default();
    assert!(sink.write(b"size....box"));
    assert_eq!(sink.seek(0, SEEK_SET), 0);
    assert!(sink.write(b"SIZE"));
    assert_eq!(sink.seek(0, SEEK_END), 11);
    assert!(sink.write(b"!"));
    assert_eq!(sink.seek(0, ffi::AVSEEK_SIZE), 12);
    assert_eq!(sink.take(), b"SIZE....box!");
    assert!(sink.write(b"next"));
    assert_eq!(sink.seek(-2, SEEK_CUR), 14);
    assert!(sink.write(b"XT"));
    // Before what was handed over.
    assert!(sink.seek(4, SEEK_SET) < 0);
    assert_eq!(sink.seek(18, SEEK_SET | ffi::AVSEEK_FORCE), 18);
    assert!(sink.write(b"."));
    assert_eq!(sink.take(), b"neXT\0\0.");
    assert_eq!(sink.seek(0, ffi::AVSEEK_SIZE), 19);
}

#[test]
fn hands_over_what_it_wrote_without_copying_it() {
    crate::init();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/clip-ip.mkv");
    let input = ffmpeg::format::input(&path).expect("the fixture");
    let stream = input.streams().best(Type::Video).expect("video");
    let time_base = Rational::new(1, 25);
    let mut muxer = Muxer::new(
        Format::Fmp4,
        &[OutStream {
            parameters: stream.parameters(),
            time_base,
        }],
    )
    .expect("a muxer");
    muxer.write_header().expect("the header");
    // Eight megabytes of picture, as copied video's segments can be.
    let picture = vec![0_u8; 512 * 1024];
    for frame in 0..16 {
        let mut packet = Packet::copy(&picture);
        packet.set_pts(Some(frame));
        packet.set_dts(Some(frame));
        packet.set_duration(1);
        if frame == 0 {
            packet.set_flags(ffmpeg::packet::Flags::KEY);
        }
        muxer.write(packet, 0).expect("a packet");
    }
    let before = allocated();
    let segment = muxer.cut().expect("the segment");
    let copied = allocated() - before;
    assert!(segment.len() >= 16 * picture.len(), "{}", segment.len());
    assert!(
        copied < segment.len() / 8,
        "{copied} bytes allocated to hand over {}",
        segment.len()
    );
}
