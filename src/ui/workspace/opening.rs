//! Opening files, the media files in folders, and streams, from the open
//! panel, a drop or the "Open URL" form: several at once in a block each,
//! or in one playing them in turn, as Settings → Playback say.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, PathPromptOptions, Window};
use serde_json::json;

use super::Workspace;
use crate::media::Source;
use crate::media::source::media_in;
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
}

impl Workspace {
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.open_sources(paths.into_iter().map(Source::File).collect(), cx);
    }

    /// Open files, the media files in folders, and streams. What is a
    /// folder, and what is in it, is found in the background (it may be on
    /// a network drive), then the blocks open in the order given.
    pub fn open_sources(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        self.open_as(sources, Opening::default(), cx);
    }

    /// Open `sources` as `open_sources` does, playing as `opening` says.
    pub fn open_as(&mut self, sources: Vec<Source>, opening: Opening, cx: &mut Context<Self>) {
        let listing = cx.background_spawn(async move { expand_folders(sources) });
        cx.spawn(async move |this, cx| {
            let sources = listing.await;
            this.update(cx, |this, cx| this.add_players(sources, opening, cx))
                .ok();
        })
        .detach();
    }

    /// A block for each of `sources`, or one playing them one after
    /// another, as Settings → Playback say, unless `opening` says else.
    fn add_players(&mut self, sources: Vec<Source>, opening: Opening, cx: &mut Context<Self>) {
        let Some(first) = sources.first() else {
            return;
        };
        let settings = cx.global::<Settings>();
        let in_turn = settings.several_files == SeveralFiles::Playlist;
        let autoplay = opening.autoplay.unwrap_or(settings.autoplay);
        let (blocks, playlist) = if in_turn {
            (vec![first.clone()], sources)
        } else {
            (sources, Vec::new())
        };
        let mixer = self.mixer(cx);
        let shown = self.over_video;
        let mut opened = Vec::new();
        for source in blocks {
            log::info!("opening {source}");
            if let Source::File(path) = &source {
                recents::note_file(path, cx);
            }
            let graveyard = self.graveyard.clone();
            let mixer = mixer.clone();
            let playlist = playlist.clone();
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

/// Replace folders with the media files directly inside them. Looks at
/// the disk, so not for the UI thread.
fn expand_folders(sources: Vec<Source>) -> Vec<Source> {
    sources
        .into_iter()
        .flat_map(|source| match source {
            Source::File(path) if path.is_dir() => {
                media_in(&path).into_iter().map(Source::File).collect()
            }
            source => vec![source],
        })
        .collect()
}
