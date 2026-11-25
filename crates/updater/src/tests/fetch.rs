use super::*;
use crate::support::{Answer, Scratch, Server, binary};

fn client() -> Client {
    Client::new("plays/0.1.0")
}

/// As the feed would say `bytes` are.
fn file_of(bytes: &[u8]) -> File {
    File {
        url: "a.update".into(),
        size: bytes.len() as u64,
        sha256: hash::of(bytes),
    }
}

/// `file` downloaded from `server`'s `a.update` into `scratch`: what was
/// told of how far it had come, and how it went.
fn download(
    server: &Server,
    scratch: &Scratch,
    file: &File,
    cancel: bool,
) -> (Vec<u64>, Result<()>) {
    let mut told = Vec::new();
    let downloaded = client().download(
        &server.url("a.update"),
        file,
        &scratch.join("a.update"),
        &mut |done| told.push(done),
        &AtomicBool::new(cancel),
    );
    (told, downloaded)
}

#[test]
fn a_download_comes_as_the_feed_says_it_is() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let bytes = binary(1, 300_000);
    server.set("a.update", Answer::Ok(bytes.clone()));
    let (told, downloaded) = download(&server, &scratch, &file_of(&bytes), false);
    downloaded.unwrap();
    assert_eq!(fs::read(scratch.join("a.update")).unwrap(), bytes);
    assert_eq!(told.last(), Some(&300_000));
    assert!(told.windows(2).all(|pair| pair[0] < pair[1]), "{told:?}");
    assert_eq!(scratch.names(""), ["a.update"]);
}

#[test]
fn a_download_sent_on_elsewhere_comes_from_there() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let bytes = binary(7, 50_000);
    server.set("a.update", Answer::Redirect("assets/a.update".into()));
    server.set("assets/a.update", Answer::Ok(bytes.clone()));
    let (_, downloaded) = download(&server, &scratch, &file_of(&bytes), false);
    downloaded.unwrap();
    assert_eq!(fs::read(scratch.join("a.update")).unwrap(), bytes);
    assert_eq!(server.asked(), ["a.update", "assets/a.update"]);
}

#[test]
fn a_download_there_already_isnt_downloaded_again() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let bytes = binary(2, 1_000);
    fs::write(scratch.join("a.update"), &bytes).unwrap();
    let (told, downloaded) = download(&server, &scratch, &file_of(&bytes), false);
    downloaded.unwrap();
    assert_eq!(told, [1_000]);
    assert!(server.asked().is_empty());
}

#[test]
fn a_download_there_already_that_isnt_the_one_is_downloaded_again() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let bytes = binary(2, 1_000);
    fs::write(scratch.join("a.update"), "an earlier one").unwrap();
    server.set("a.update", Answer::Ok(bytes.clone()));
    download(&server, &scratch, &file_of(&bytes), false)
        .1
        .unwrap();
    assert_eq!(fs::read(scratch.join("a.update")).unwrap(), bytes);
}

#[test]
fn a_download_bigger_than_the_feed_says_stops_there() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let bytes = binary(3, 200_000);
    server.set("a.update", Answer::Ok(bytes.clone()));
    let file = file_of(&bytes[..100_000]);
    let (told, downloaded) = download(&server, &scratch, &file, false);
    assert!(
        matches!(downloaded, Err(Error::TooBig { limit: 100_000, .. })),
        "{downloaded:?}"
    );
    assert!(told.iter().all(|done| *done <= 100_000));
    assert!(scratch.names("").is_empty());
}

#[test]
fn a_download_that_isnt_what_the_feed_says_is_thrown_away() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let bytes = binary(4, 5_000);
    let mut changed = bytes.clone();
    changed[100] ^= 1;
    server.set("a.update", Answer::Ok(changed));
    let downloaded = download(&server, &scratch, &file_of(&bytes), false).1;
    assert!(
        matches!(downloaded, Err(Error::Mismatch(_))),
        "{downloaded:?}"
    );
    // Nor one cut short.
    server.set("a.update", Answer::Ok(bytes[..4_000].to_vec()));
    let downloaded = download(&server, &scratch, &file_of(&bytes), false).1;
    assert!(
        matches!(downloaded, Err(Error::Mismatch(_))),
        "{downloaded:?}"
    );
    assert!(scratch.names("").is_empty());
}

#[test]
fn a_download_stopped_leaves_nothing() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let bytes = binary(5, 5_000);
    server.set("a.update", Answer::Ok(bytes.clone()));
    let downloaded = download(&server, &scratch, &file_of(&bytes), true).1;
    assert!(
        matches!(downloaded, Err(Error::Cancelled)),
        "{downloaded:?}"
    );
    assert!(scratch.names("").is_empty());
}

#[test]
fn an_http_error_says_its_status_and_whether_to_try_again() {
    let (server, scratch) = (Server::new(), Scratch::new());
    let file = file_of(b"plays");
    let downloaded = download(&server, &scratch, &file, false).1;
    assert!(
        matches!(&downloaded, Err(Error::Http { status: 404, .. })),
        "{downloaded:?}"
    );
    assert!(!downloaded.unwrap_err().is_transient());
    server.set("a.update", Answer::Status(503));
    let downloaded = download(&server, &scratch, &file, false).1;
    assert!(
        matches!(&downloaded, Err(Error::Http { status: 503, .. })),
        "{downloaded:?}"
    );
    assert!(downloaded.unwrap_err().is_transient());
}

#[test]
fn a_server_not_there_is_the_network() {
    let url = {
        let server = Server::new();
        server.url("feed.jsonl")
    };
    let opened = client().open(&url);
    assert!(matches!(&opened, Err(err @ Error::Network { .. }) if err.is_transient()));
}

#[test]
fn the_feed_is_read_as_it_comes() {
    let server = Server::new();
    server.set("feed.jsonl", Answer::Ok(b"one\ntwo\n".to_vec()));
    let lines: io::Result<Vec<String>> = client()
        .open(&server.url("feed.jsonl"))
        .unwrap()
        .lines()
        .collect();
    let lines = lines.unwrap();
    assert_eq!(lines, ["one", "two"]);
}

#[test]
fn only_https_is_fetched_or_http_to_this_machine() {
    for url in [
        "http://example.com/feed.jsonl",
        "http://10.0.0.1/feed.jsonl",
        "ftp://localhost/feed.jsonl",
        "file:///etc/hosts",
    ] {
        let opened = client().open(&Url::parse(url).unwrap());
        assert!(matches!(&opened, Err(Error::Insecure(_))), "{url}");
    }
    for url in [
        "http://localhost:1/",
        "http://127.0.0.1:1/",
        "http://[::1]:1/",
        "http://LOCALHOST:1/",
    ] {
        assert!(is_local(&Url::parse(url).unwrap()), "{url}");
    }
    for url in [
        "http://localhost.example.com/",
        "http://192.168.1.2/",
        "https://example.com/",
    ] {
        assert!(!is_local(&Url::parse(url).unwrap()), "{url}");
    }
}
