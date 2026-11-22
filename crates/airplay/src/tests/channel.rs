use super::*;

/// Both ends of a connection, in memory.
struct Pipe {
    incoming: io::Cursor<Vec<u8>>,
    outgoing: Vec<u8>,
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.incoming.read(buf)
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.outgoing.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn pipe(incoming: Vec<u8>) -> Pipe {
    Pipe {
        incoming: io::Cursor::new(incoming),
        outgoing: Vec::new(),
    }
}

#[test]
fn what_one_side_writes_the_other_reads_in_frames_of_1024_bytes() {
    let keys = Keys::control(b"a secret both sides have");
    let message: Vec<u8> = (0..2500).map(|i| i as u8).collect();
    let mut ours = Sealed::new(pipe(Vec::new()), keys);
    ours.write_all(&message).unwrap();
    ours.write_all(b"and more").unwrap();
    let wire = ours.stream.outgoing.clone();
    assert_eq!(wire.len(), 2500 + 8 + 4 * (2 + TAG));
    assert_eq!(&wire[..2], 1024u16.to_le_bytes());

    let mut same = Sealed::new(pipe(wire), keys);
    assert!(
        same.read(&mut [0; 16]).is_err(),
        "a side can't read with the keys it writes with"
    );
}

#[test]
fn the_other_side_reads_with_the_keys_swapped() {
    let keys = Keys::control(b"a secret both sides have");
    let mut ours = Sealed::new(pipe(Vec::new()), keys);
    ours.write_all(&[7; 3000]).unwrap();
    ours.write_all(b"end").unwrap();
    let mut theirs = Sealed::new(pipe(ours.stream.outgoing), keys.swapped());
    let mut read = Vec::new();
    theirs.read_to_end(&mut read).unwrap();
    assert_eq!(read.len(), 3003);
    assert_eq!(&read[3000..], b"end");
}

#[test]
fn a_frame_out_of_order_doesnt_open() {
    let keys = Keys::control(b"secret");
    let mut ours = Sealed::new(pipe(Vec::new()), keys);
    ours.write_all(b"first").unwrap();
    let first = ours.stream.outgoing.clone();
    ours.write_all(b"second").unwrap();
    let second = ours.stream.outgoing[first.len()..].to_vec();
    let mut theirs = Sealed::new(pipe(second), keys.swapped());
    assert!(theirs.read(&mut [0; 16]).is_err());
}
