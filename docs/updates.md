# Updates

Plays updates itself from a feed signed with minisign. `updater`, in
`crates/updater`, makes the key pair, the update files and the feed:

```bash
cargo run -p updater --features release -- --help
```

Below, `updater` stands for `cargo run -p updater --features release --`.

## Once: the key pair

```bash
updater keygen ~/plays-release-keys
```

- `updates.key`: the secret key, encrypted with a password, asked for on
  the terminal or taken from `UPDATER_PASSWORD`. Keep it, and its
  password, out of the repo.
- `updates.pub`: its public key, which goes in `resources/updates.json`:

```json
{
  "feed": "https://example.com/plays/feed.jsonl",
  "keys": ["RW…"]
}
```

- `keys`: a `.pub` file's text, or its key line alone (`RW…`).
- `"feed": null`: Plays doesn't update itself.
- Built in: a change takes effect from the next build.

## Each release

For 0.2.0, with 0.1.0 before it, on Apple silicon:

```bash
updater pack --version 0.2.0 target/release/Plays.app Plays-0.2.0-macos-aarch64.update
```

```bash
updater delta Plays-0.1.0-macos-aarch64.update Plays-0.2.0-macos-aarch64.update Plays-0.1.0-to-0.2.0-macos-aarch64.update
```

```bash
updater feed feed.jsonl --notes notes.md --key ~/plays-release-keys/updates.key \
  --update macos-aarch64=Plays-0.2.0-macos-aarch64.update \
  --delta macos-aarch64=Plays-0.1.0-to-0.2.0-macos-aarch64.update
```

```bash
updater verify feed.jsonl --key ~/plays-release-keys/updates.pub
```

- `pack`: the install as it ships, at the version in `Cargo.toml`:
  `Plays.app` on macOS, the `plays` binary or the AppImage on Linux,
  `plays.exe` on Windows.
- macOS: Plays takes an update only if it passes `codesign --verify
  --strict --deep`, and is signed by the running app's team, if that has
  one. Pack the app once it's signed, notarized and stapled.
- Platforms are `OS-ARCH`: `macos-aarch64`, `macos-x86_64`,
  `linux-x86_64`, `linux-aarch64`, `windows-x86_64`. Give one `--update`
  per platform. Give one `--delta` per platform and earlier version.
- `delta`: from an earlier release's whole update to this one's, so keep
  each release's whole updates. A Plays with no delta from its version
  downloads the whole update.
- `feed`: makes the feed if there's none (`--app plays`), then puts the
  release first and signs it and the header. The other releases' lines
  stay as they are. It refuses a version that isn't newer than the
  latest.
- `--notes`: a Markdown file. About shows the notes of every release
  since the running one.
- The feed names each update file as it's named, beside the feed or under
  `--url`. Names need the version and platform in them.
- Upload the update files first and the feed last.
- A release needn't be for every platform. Each platform updates to its
  own newest release, and is shown the notes of its own releases alone.

## On GitHub

The update files go in a GitHub release, built by Actions. The feed goes
on the `gh-pages` branch, served by Pages. The secret key stays on your
machine.

Once:

- Settings → Pages: deploy from the `gh-pages` branch, its root.
- In `resources/updates.json`, set `"feed"` to
  `https://OWNER.github.io/REPO/feed.jsonl`.

Each release:

1. Set `version` in `Cargo.toml`, build once so `Cargo.lock` has it
   too, write the release's notes in `release.md`, and commit and push
   all three.
2. Push the tag, the version as it is, such as `0.2.1`, for every OS. Or
   Actions → Release → Run workflow, on that commit, with the tag and the
   OS: `all`, `macos` or `linux`. The workflow:
   - fails if the tag isn't `Cargo.toml`'s and `Cargo.lock`'s version,
     is released already, or `release.md` is missing or empty
   - builds each of the OS's platforms: `macos-aarch64`, `linux-x86_64`,
     `linux-aarch64`
   - makes their update files with `script/release-files`, with deltas
     from each platform's last 3 releases
   - publishes the release with `release.md` as its notes, tagging the
     commit if it's run by hand
3. Once the release is published:

```bash
script/feed add VERSION
```

`script/feed add` downloads the release's update files and notes, and
puts the release first in the feed, pointing at the release. It signs the
feed, checks it with the keys in `resources/updates.json`, and pushes it
to `gh-pages` once you say so.

A fix for one OS is a release for that OS alone:

- The feed has downloads for its platforms alone.
- The GitHub release isn't marked the latest, so the latest still has an
  install for every platform.
- Each platform's deltas come from its own earlier releases, so Linux's
  next delta is from its last release, not from a macOS fix.

Every week, the Feed workflow runs `script/feed check 14`, which fails
when the published feed has fewer than 14 days left. Then:

```bash
script/feed sign
```

- `script/feed` signs for 90 days (`FEED_DAYS`), with
  `~/plays-release-keys/updates.key` (`UPDATER_KEY`).
- In a public repo, GitHub turns scheduled workflows off after 60 days
  with no activity, emailing first.

## Before the feed expires

```bash
updater sign feed.jsonl --key ~/plays-release-keys/updates.key
```

- Plays refuses a feed past its expiry, so a stale mirror can't hold it on
  an old release.
- `sign` signs the header again; the releases' lines stay as they are.
- `feed` and `sign` take `--days`; the default is 30.
- Sign it again and upload it before then.
- `updater verify --at-least 14` fails when the feed has fewer than 14
  days left.

## A new key

1. `keygen` into another folder.
2. Add the new public key to `keys`, beside the old, and release that
   build with the feed still signed with the old key.
3. Once that build is out, `sign` with the new key, which signs the
   lines the old key signed again, and take the old key out of `keys`.

## What Plays does

- It checks a minute after starting, then once a day, while Settings →
  About → Check for updates automatically is on. Plays → Check for
  Updates… checks now.
- It reads the feed as it comes, and stops at the running version.
- It takes the feed only if:
  - each line is signed with one of `keys`, for `plays`
  - it names `plays`
  - it hasn't expired
  - it came over HTTPS (HTTP only from this machine)
- It downloads the delta from the running version if there is one, and
  the whole update otherwise. Each file is checked against its SHA-256,
  and a delta that doesn't apply falls back to the whole update.
- It puts the update together beside the install. Restart to Update swaps
  it in, or Plays swaps it in as it quits.
- Off when:
  - it's run from Cargo's `target`
  - it's installed under `/usr`, `/nix` or `/gnu`
  - it's in a Flatpak or Snap, or from the Microsoft Store
  - macOS runs it from a translocated copy
- Once an update is ready, a note over the block keys go to, or at the
  top of an empty window, says so until it's dismissed, with Dismiss and
  Restart Now: none while notes are hidden.
- About says why it doesn't update, or why an update failed.

## Trying it here

```bash
script/try-updates tests/fixtures/clip.mp4
```

- The debug build updates a stand-in for its install, from a feed served
  on `127.0.0.1`, with a key of its own: Settings → About, the notes, the
  download's progress, the note that says it's ready, and Restart to
  Update.
- In `target/try-updates`: the stand-in at `Cargo.toml`'s version, two
  newer versions with deltas, their feed, and `try.json`, which
  `PLAYS_TRY_UPDATES` names. Release builds don't read it.
- The newest's notes are `release.md`'s, if there's one.
- `RATE` (bytes a second), `FIRST_CHECK` (seconds), `PORT`.
- The files open in it.
- It runs beside another Plays.

## Trying a release

1. In `resources/updates.json`, set `"feed": "http://localhost:8000/feed.jsonl"`
   and the public key.
2. Build the earlier version, and copy it out of `target`.
3. Pack the newer version and make its feed in a folder.
4. Serve that folder: `python3 -m http.server 8000`.
5. Open the copy, then Plays → Check for Updates….

## Formats

- The feed is JSON Lines; [feed.rs](../crates/updater/src/feed.rs) has it
  line by line.

| Line | What it is |
| --- | --- |
| 1 | `{"signature": …, "header": …}`: `app`, `latest`, `published`, `expires` (seconds since 1970) |
| 2… | `{"signature": …, "release": …}`, newest first: `version`, `published`, `notes`, `downloads` by platform |

- Each `signature` is minisign's, of the record's JSON as it is in the
  line, its trusted comment the app.
- The first release is the header's `latest`, and each after is older. A
  release left out hides only its notes.

- An update file is `UPDATER1`, its header's length, the header as JSON,
  then zstd frames. Each file is kept, patched from the installed one, or
  whole; [package.rs](../crates/updater/src/package.rs) has the details.
