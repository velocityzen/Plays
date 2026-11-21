//! What `window.ls`, `window.inspect`, `tile.ls` and `tile.inspect` say
//! of a window and its tiles, as JSON.

use gpui::{App, Entity, Window};
use serde_json::{Map, Value, json};

use super::Workspace;
use crate::ui::displays;
use crate::ui::player::PlayerView;

impl Workspace {
    /// What `window.ls` says of it: its id and label, its `number` (from
    /// 1, as ⌘1 to ⌘9 go to it), whether it's in `front`, and how many
    /// tiles it has.
    pub fn summary(&self, number: usize, front: bool) -> Map<String, Value> {
        let mut summary = Map::new();
        summary.insert("id".into(), json!(self.id().to_string()));
        summary.insert("label".into(), json!(self.label));
        summary.insert("number".into(), json!(number));
        summary.insert("front".into(), json!(front));
        summary.insert("tiles".into(), json!(self.tiles.len()));
        summary
    }

    /// All `window.inspect` says of it: its summary; where it is, in
    /// points, and on which display; whether it's full screen and floats
    /// on top; its own output, if it has one; and its tiles, as `tile.ls`
    /// says.
    pub fn inspect(
        &self,
        number: usize,
        front: bool,
        window: &Window,
        cx: &App,
    ) -> Map<String, Value> {
        let bounds = window.bounds();
        let mut inspected = self.summary(number, front);
        inspected.extend([
            (
                "bounds".into(),
                json!({
                    "x": f32::from(bounds.origin.x),
                    "y": f32::from(bounds.origin.y),
                    "width": f32::from(bounds.size.width),
                    "height": f32::from(bounds.size.height),
                }),
            ),
            ("display".into(), json!(displays::of(window, cx))),
            ("full_screen".into(), json!(window.is_fullscreen())),
            ("on_top".into(), json!(self.on_top)),
            ("output".into(), json!(self.sound_device())),
            ("tiles".into(), Value::from(self.tiles_listed(cx))),
        ]);
        inspected
    }

    /// What `tile.ls` says of each tile, in the grid's order: its summary,
    /// its number there (from 1), and whether keys go to it.
    pub fn tiles_listed(&self, cx: &App) -> Vec<Value> {
        let front = self.active_view().map(|view| view.entity_id());
        self.tiles
            .iter()
            .enumerate()
            .map(|(at, tile)| {
                let view = tile.view.read(cx);
                let front = Some(tile.view.entity_id()) == front;
                Value::Object(placed(view.summary(), at, front))
            })
            .collect()
    }

    /// All `tile.inspect` says of `tile`, as `tile.ls` does and more, with
    /// its window's id; none if it isn't here.
    pub fn inspect_tile(&self, tile: &Entity<PlayerView>, cx: &App) -> Option<Value> {
        let at = self.tiles.iter().position(|t| t.view == *tile)?;
        let front = self.active_view().as_ref() == Some(tile);
        let mut inspected = placed(tile.read(cx).inspect(), at, front);
        inspected.insert("window".into(), json!(self.id().to_string()));
        Some(Value::Object(inspected))
    }
}

/// `about` a tile, with its number in the grid (it's at `at`, from 0) and
/// whether it's in `front`.
fn placed(mut about: Map<String, Value>, at: usize, front: bool) -> Map<String, Value> {
    about.insert("number".into(), json!(at + 1));
    about.insert("front".into(), json!(front));
    about
}
