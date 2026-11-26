//! Opening files, the media files in folders, what playlist files list,
//! and streams, from the open panel, a drop or the "Open URL" form: several
//! at once in a block each, or in one playing them in turn, as Settings →
//! Playback say; what a playlist file lists, in one playing them in turn.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, PathPromptOptions, Window};
use serde_json::json;

use super::Workspace;
use crate::media::Source;
use crate::media::playlist_file;
use crate::media::source::{kind_of, media_in};
use crate::recents;
use crate::settings::{Settings, SeveralFiles};
use crate::ui::open_url::{OpenUrlDialog, OpenUrlEvent};
use crate::ui::player::PlayerView;

/// How what's opened plays, where not as Settings say.
#[derive(Clone, Copy, Debug, Default)]
pub struct Opening {
    /// Whether each plays from the moment it opens: as Settings →
    /// Playback say if `None`.
    pub autoplay: Option<bool>,
    /// Several files in a block each, or in one playing them in turn: as
    /// Settings → Playback say if `None`.
    pub several_files: Option<SeveralFiles>,
    /// Only a folder's video files, or only its sound files: all its
    /// media files if `None`.
    pub kind: Option<file_types::Kind>,
}

impl Workspace {
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.open_sources(paths.into_iter().map(Source::File).collect(), cx);
    }

    /// Open files, the media files in folders, what playlist files list,
    /// and streams. What is a folder, and what is in it or in a playlist, is
    /// found in the background (it may be on a network drive), then the
    /// blocks open in the order given.
    pub fn open_sources(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        self.open_as(sources, Opening::default(), cx);
    }

    /// Open `sources` as `open_sources` does, but as `opening` says.
    pub fn open_as(&mut self, sources: Vec<Source>, opening: Opening, cx: &mut Context<Self>) {
        let listing = cx.background_spawn(async move { expand(sources, opening.kind) });
        cx.spawn(async move |this, cx| {
            let listed = listing.await;
            this.update(cx, |this, cx| this.add_players(listed, opening, cx))
                .ok();
        })
        .detach();
    }

    /// A block for each of `listed`, or one playing them all one after
    /// another, as Settings → Playback say, unless `opening` says else; a
    /// playlist file's in one block either way.
    fn add_players(&mut self, listed: Vec<Listed>, opening: Opening, cx: &mut Context<Self>) {
        let settings = cx.global::<Settings>();
        let in_turn =
            opening.several_files.unwrap_or(settings.several_files) == SeveralFiles::Playlist;
        let autoplay = opening.autoplay.unwrap_or(settings.autoplay);
        let blocks = blocks(listed, in_turn);
        if blocks.is_empty() {
            return;
        }
        let mixer = self.mixer(cx);
        let shown = self.over_video;
        let mut opened = Vec::new();
        for Block {
            source,
            playlist,
            from,
        } in blocks
        {
            log::info!("opening {source}");
            if let Some(path) = from.as_ref().or(match &source {
                Source::File(path) => Some(path),
                Source::Url(_) => None,
            }) {
                recents::note_file(path, cx);
            }
            let graveyard = self.graveyard.clone();
            let mixer = mixer.clone();
            let view = cx.new(|cx| {
                let mut view = PlayerView::new(source, autoplay, mixer, graveyard, cx);
                view.set_over_video(shown, cx);
                view.set_playlist(playlist, cx);
                view.resume(cx);
                view.find_neighbors(cx);
                view
            });
            opened.push(view.entity_id());
            self.add_block(view, cx);
        }
        // What's opened shows, with the blocks there already.
        self.maximized = None;
        self.blocks_changed(cx);
        self.fit_when_known(opened, cx);
    }

    pub fn prompt_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: cx.can_select_mixed_files_and_dirs(),
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                this.update(cx, |this, cx| this.open_paths(paths, cx)).ok();
            }
        })
        .detach();
    }

    /// Show the "Open URL" form: in place of the empty window's buttons, or
    /// in a sheet over a window that is playing. Focuses it if already up.
    pub fn show_open_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocks.is_empty() {
            self.url_form_open = true;
            self.url_form.update(cx, |form, cx| form.focus(window, cx));
            cx.notify();
            return;
        }
        if let Some((dialog, _)) = &self.open_url {
            dialog.update(cx, |dialog, cx| dialog.focus(window, cx));
            return;
        }
        let dialog = cx.new(|cx| OpenUrlDialog::new(window, cx));
        let subscription = cx.subscribe_in(&dialog, window, |this, _, event, window, cx| {
            if let OpenUrlEvent::Open(source) = event {
                this.send("app.open", json!({"urls": [source.address()]}), cx);
            }
            this.open_url = None;
            window.focus(&this.focus_handle);
            cx.notify();
        });
        self.open_url = Some((dialog, subscription));
        cx.notify();
    }
}

/// What's opened, a folder's files and a playlist's found.
#[derive(Debug, PartialEq, Eq)]
enum Listed {
    /// A file or stream, given or in a folder given.
    One(Source),
    /// What a playlist file lists, which plays in one block.
    List {
        /// The playlist file, noted among the recent files for it; none for
        /// one from the network.
        from: Option<PathBuf>,
        sources: Vec<Source>,
    },
}

/// Replace folders with the media files directly inside them, only those
/// of `kind` if there is one, and playlist files with what they list. A
/// playlist that can't be read, or is a stream (HLS), opens as it is.
/// Looks at the disk and the network, so not for the UI thread.
fn expand(sources: Vec<Source>, kind: Option<file_types::Kind>) -> Vec<Listed> {
    let mut listed = Vec::new();
    for source in sources {
        match source {
            Source::File(path) if path.is_dir() => listed.extend(
                media_in(&path)
                    .into_iter()
                    .filter(|file| kind.is_none_or(|kind| kind_of(file) == Some(kind)))
                    .map(|file| Listed::One(Source::File(file))),
            ),
            Source::File(path) if playlist_file::is_playlist_file(&path) => {
                listed.push(match playlist_file::read(&path) {
                    Some(sources) if !sources.is_empty() => Listed::List {
                        from: Some(path),
                        sources,
                    },
                    _ => Listed::One(Source::File(path)),
                });
            }
            Source::Url(url) if playlist_file::is_playlist_url(&url) => {
                listed.push(match playlist_file::fetch(&url) {
                    Some(sources) if !sources.is_empty() => Listed::List {
                        from: None,
                        sources,
                    },
                    _ => Listed::One(Source::Url(url)),
                });
            }
            source => listed.push(Listed::One(source)),
        }
    }
    listed
}

/// A block to open: what it plays first, the playlist it plays in turn (if
/// more than that), and the playlist file that listed them.
#[derive(Debug, PartialEq, Eq)]
struct Block {
    source: Source,
    playlist: Vec<Source>,
    from: Option<PathBuf>,
}

/// The blocks `listed` opens in: all in one, playing them in turn, if
/// `in_turn`, else a block each, but for each playlist file's, in one.
fn blocks(listed: Vec<Listed>, in_turn: bool) -> Vec<Block> {
    if in_turn {
        let from = match listed.as_slice() {
            [Listed::List { from, .. }] => from.clone(),
            _ => None,
        };
        let playlist: Vec<Source> = listed
            .into_iter()
            .flat_map(|listed| match listed {
                Listed::One(source) => vec![source],
                Listed::List { sources, .. } => sources,
            })
            .collect();
        return playlist
            .first()
            .cloned()
            .map(|source| Block {
                source,
                playlist,
                from,
            })
            .into_iter()
            .collect();
    }
    listed
        .into_iter()
        .filter_map(|listed| match listed {
            Listed::One(source) => Some(Block {
                source,
                playlist: Vec::new(),
                from: None,
            }),
            Listed::List { from, sources } => sources.first().cloned().map(|source| Block {
                source,
                playlist: sources,
                from,
            }),
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/opening.rs"]
mod tests;
