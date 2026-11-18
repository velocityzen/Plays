//! Opening files, the media files in folders, and streams, from the open
//! panel, a drop or the "Open URL" form.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, PathPromptOptions, Window};

use super::Workspace;
use crate::app::AudioEngine;
use crate::media::Source;
use crate::media::source::media_in;
use crate::settings::Settings;
use crate::ui::open_url::{OpenUrlDialog, OpenUrlEvent};
use crate::ui::player::PlayerView;

impl Workspace {
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.open_sources(paths.into_iter().map(Source::File).collect(), cx);
    }

    /// Open files, the media files in folders, and streams.
    pub fn open_sources(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        let settings = cx.global::<Settings>().clone();
        let mixer = AudioEngine::mixer(cx);
        let sources = sources.into_iter().flat_map(|source| match source {
            Source::File(path) => expand_folders(vec![path])
                .into_iter()
                .map(Source::File)
                .collect(),
            stream => vec![stream],
        });
        for source in sources {
            log::info!("opening {source}");
            let graveyard = self.graveyard.clone();
            let mixer = mixer.clone();
            let view = cx.new(|_| PlayerView::new(source, mixer, &settings, graveyard));
            self.add_tile(view, cx);
        }
        self.tiles_changed(cx);
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
        if self.tiles.is_empty() {
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
                this.open_sources(vec![source.clone()], cx);
            }
            this.open_url = None;
            window.focus(&this.focus_handle);
            cx.notify();
        });
        self.open_url = Some((dialog, subscription));
        cx.notify();
    }
}

/// Replace folders with the media files directly inside them.
fn expand_folders(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            files.extend(media_in(&path));
        } else {
            files.push(path);
        }
    }
    files
}
