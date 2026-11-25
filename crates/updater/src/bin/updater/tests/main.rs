use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A folder of a test's own, gone once it's dropped.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "updater-tool-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    /// `path` in it, as an argument says it.
    pub(crate) fn arg(&self, path: &str) -> String {
        self.join(path).to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Bytes that hardly compress, the same for the same `seed`.
pub(crate) fn binary(seed: u8, len: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64 ^ u64::from(seed);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// In `scratch`: `old` and `new`, single-file installs of 0.1.0 and
/// 0.2.0, and their whole updates, `0.1.0.update` and `0.2.0.update`.
pub(crate) fn installs(scratch: &Scratch) -> (Vec<u8>, Vec<u8>) {
    let old = binary(1, 50_000);
    let mut new = old.clone();
    new[20_000..20_050].fill(0);
    fs::write(scratch.join("old"), &old).unwrap();
    fs::write(scratch.join("new"), &new).unwrap();
    pack(
        &scratch.join("old"),
        &Version::new(0, 1, 0),
        &scratch.join("0.1.0.update"),
    )
    .unwrap();
    pack(
        &scratch.join("new"),
        &Version::new(0, 2, 0),
        &scratch.join("0.2.0.update"),
    )
    .unwrap();
    (old, new)
}

#[test]
fn a_delta_is_made_from_two_whole_updates() {
    let scratch = Scratch::new();
    let (_, new) = installs(&scratch);
    let delta = scratch.join("delta.update");
    delta_in(
        &scratch.join("0.1.0.update"),
        &scratch.join("0.2.0.update"),
        &delta,
        &scratch.join("work"),
    )
    .unwrap();
    let versions = package::versions(&delta).unwrap();
    assert_eq!(
        (versions.from.as_deref(), versions.to.as_str()),
        (Some("0.1.0"), "0.2.0")
    );
    assert!(fs::metadata(&delta).unwrap().len() < 5_000);
    package::apply(
        &delta,
        Some(&scratch.join("old")),
        "0.2.0",
        &scratch.join("applied"),
    )
    .unwrap();
    assert_eq!(fs::read(scratch.join("applied")).unwrap(), new);
}

#[test]
fn a_delta_is_made_only_from_whole_updates_to_a_newer_version() {
    let scratch = Scratch::new();
    installs(&scratch);
    let work = scratch.join("work");
    let (old, new) = (scratch.join("0.1.0.update"), scratch.join("0.2.0.update"));
    let backwards = delta_in(&new, &old, &scratch.join("back.update"), &work);
    assert_eq!(
        backwards.unwrap_err(),
        format!("{} is 0.1.0, not newer than 0.2.0", old.display())
    );
    delta_in(&old, &new, &scratch.join("delta.update"), &work).unwrap();
    let _ = fs::remove_dir_all(&work);
    let of_delta = delta_in(
        &scratch.join("delta.update"),
        &new,
        &scratch.join("again.update"),
        &work,
    );
    assert!(
        of_delta
            .unwrap_err()
            .ends_with("is a delta from 0.1.0, not a whole update")
    );
}
