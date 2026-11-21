//! A single-line text field, adapted from gpui's input example: selection with
//! mouse and keyboard, clipboard, IME composition, and horizontal scrolling
//! so the cursor stays visible in long URLs.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle,
    Window, actions, div, fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

use super::components::{ComponentSize, ControlFrame, Sizable};
use super::theme::Theme;
use super::theme::controls::{CARET_WIDTH, COMPOSING_UNDERLINE, SELECTION, TEXT_SCROLL_MARGIN};

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
    ]
);

const CONTEXT: &str = "TextInput";

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("secondary-left", Home, context),
        KeyBinding::new("secondary-right", End, context),
        KeyBinding::new("secondary-a", SelectAll, context),
        KeyBinding::new("secondary-v", Paste, context),
        KeyBinding::new("secondary-c", Copy, context),
        KeyBinding::new("secondary-x", Cut, context),
    ]);
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    /// How far the text is scrolled to the left to keep the cursor in view.
    scroll_x: Pixels,
    is_selecting: bool,
    colors: Colors,
    size: ComponentSize,
}

#[derive(Clone, Copy)]
struct Colors {
    placeholder: Hsla,
    selection: Hsla,
    cursor: Hsla,
}

impl TextInput {
    pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: SharedString::default(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_x: px(0.),
            is_selecting: false,
            colors: Colors {
                placeholder: gpui::transparent_black(),
                selection: gpui::transparent_black(),
                cursor: gpui::transparent_black(),
            },
            size: ComponentSize::Medium,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    /// Replace the text and select all of it, so typing replaces it.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.content = text.into();
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            // One line only: a URL copied with a trailing newline should still work.
            let text = text.trim().replace(['\n', '\r'], "");
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text().to_owned()));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text().to_owned()));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left() + self.scroll_x)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        utf16_offset(&self.content, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        utf8_range(&self.content, range_utf16)
    }

    /// The range IME or editing is to replace: the one given, else what's
    /// being composed, else the selection, made safe to slice the text with.
    fn range_to_replace(&self, range_utf16: Option<&Range<usize>>) -> Range<usize> {
        let range = range_utf16
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        within(&self.content, range)
    }

    fn selected_text(&self) -> &str {
        &self.content[within(&self.content, self.selected_range.clone())]
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.range_to_replace(range_utf16.as_ref());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.range_to_replace(range_utf16.as_ref());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        // IME counts the selection within what it's composing.
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|selected| utf8_range(new_text, selected))
            .map(|selected| range.start + selected.start..range.start + selected.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let left = bounds.left() - self.scroll_x;
        Some(Bounds::from_corners(
            point(left + last_layout.x_for_index(range.start), bounds.top()),
            point(left + last_layout.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(point.x - bounds.left() + self.scroll_x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

/// Where in `text` UTF-16 `offset` is, as a byte offset, and no further
/// than its end.
fn utf8_offset(text: &str, offset: usize) -> usize {
    let mut utf8_offset = 0;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }
    utf8_offset
}

fn utf16_offset(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    let mut utf8_count = 0;
    for ch in text.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

/// A UTF-16 range of `text` as bytes, in order and within it.
fn utf8_range(text: &str, range_utf16: &Range<usize>) -> Range<usize> {
    within(
        text,
        utf8_offset(text, range_utf16.start)..utf8_offset(text, range_utf16.end),
    )
}

/// `range` made safe to slice `text` with: in order, no further than its
/// end, and on character boundaries.
fn within(text: &str, range: Range<usize>) -> Range<usize> {
    let start = text.floor_char_boundary(range.start);
    let end = text.floor_char_boundary(range.end);
    start.min(end)..start.max(end)
}

/// Lays out and paints the text, selection and cursor of a [`TextInput`].
struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    scroll_x: Pixels,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let colors = input.colors;
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), colors.placeholder)
        } else {
            (content.clone(), style.color)
        };
        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match input.marked_range.as_ref().filter(|_| !content.is_empty()) {
            Some(marked) => [
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: COMPOSING_UNDERLINE,
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        // Scroll just enough to keep the cursor inside the field.
        let cursor_x = if content.is_empty() {
            px(0.)
        } else {
            line.x_for_index(cursor)
        };
        let width = bounds.size.width;
        let max_scroll = (line.width - width + TEXT_SCROLL_MARGIN).max(px(0.));
        let mut scroll_x = input.scroll_x;
        if cursor_x < scroll_x {
            scroll_x = cursor_x;
        } else if cursor_x > scroll_x + width - TEXT_SCROLL_MARGIN {
            scroll_x = cursor_x - width + TEXT_SCROLL_MARGIN;
        }
        let scroll_x = scroll_x.clamp(px(0.), max_scroll);
        let left = bounds.left() - scroll_x;

        let (selection, cursor) = if selected_range.is_empty() {
            let cursor = fill(
                Bounds::new(
                    point(left + cursor_x, bounds.top()),
                    size(CARET_WIDTH, bounds.size.height),
                ),
                colors.cursor,
            );
            (None, Some(cursor))
        } else {
            let selection = fill(
                Bounds::from_corners(
                    point(left + line.x_for_index(selected_range.start), bounds.top()),
                    point(left + line.x_for_index(selected_range.end), bounds.bottom()),
                ),
                colors.selection,
            );
            (Some(selection), None)
        };
        PrepaintState {
            line: Some(line),
            scroll_x,
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line = prepaint.line.take().expect("shaped in prepaint");
        let scroll_x = prepaint.scroll_x;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection)
            }
            let origin = point(bounds.left() - scroll_x, bounds.top());
            if let Err(err) = line.paint(origin, window.line_height(), window, cx) {
                log::warn!("painting text failed: {err:#}");
            }
            if focus_handle.is_focused(window)
                && let Some(cursor) = prepaint.cursor.take()
            {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
            input.scroll_x = scroll_x;
        });
    }
}

impl Sizable for TextInput {
    fn with_size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let focused = self.focus_handle.is_focused(window);
        self.colors = Colors {
            placeholder: theme.text_muted,
            selection: theme.accent.opacity(SELECTION),
            cursor: theme.accent,
        };
        div()
            .id("text-input")
            .debug_selector(|| "text-input".into())
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .control_frame(self.size)
            .w_full()
            .border_color(if focused { theme.accent } else { theme.border })
            .bg(theme.background)
            .text_color(theme.text)
            .overflow_hidden()
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Entity, TestAppContext, VisualTestContext};
    use proptest::prelude::*;

    use super::*;
    use crate::settings::Settings;
    use crate::ui::theme::UiFont;

    /// Characters of one to four bytes, and of one or two UTF-16 units:
    /// a, é, 中 and 😀.
    const WIDE: &str = "aé中😀b";

    /// Do something to the input, as its window would.
    fn edit<R>(
        cx: &mut VisualTestContext,
        input: &Entity<TextInput>,
        what: impl FnOnce(&mut TextInput, &mut Window, &mut Context<TextInput>) -> R,
    ) -> R {
        cx.update(|window, cx| input.update(cx, |input, cx| what(input, window, cx)))
    }

    fn new_input(cx: &mut TestAppContext) -> (Entity<TextInput>, &mut VisualTestContext) {
        let cx = cx.add_empty_window();
        let input = cx.update(|_, cx| cx.new(|cx| TextInput::new("", cx)));
        (input, cx)
    }

    /// What IME's `setMarkedText:selectedRange:` does.
    fn compose(
        cx: &mut VisualTestContext,
        input: &Entity<TextInput>,
        text: &str,
        selected: Range<usize>,
    ) {
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, text, Some(selected), window, cx);
            });
        });
    }

    fn type_text(cx: &mut VisualTestContext, input: &Entity<TextInput>, text: &str) {
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.replace_text_in_range(None, text, window, cx);
            });
        });
    }

    fn state(cx: &mut VisualTestContext, input: &Entity<TextInput>) -> (String, Range<usize>) {
        cx.update(|_, cx| {
            let input = input.read(cx);
            (input.content.to_string(), input.selected_range.clone())
        })
    }

    #[gpui::test]
    fn composing_keeps_the_cursor_in_what_is_composed(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        compose(cx, &input, "n", 1..1);
        assert_eq!(state(cx, &input), ("n".to_owned(), 1..1));
        compose(cx, &input, "ni", 2..2);
        assert_eq!(state(cx, &input), ("ni".to_owned(), 2..2));
        compose(cx, &input, "你", 1..1);
        assert_eq!(state(cx, &input), ("你".to_owned(), 3..3));
        type_text(cx, &input, "你");
        assert_eq!(state(cx, &input), ("你".to_owned(), 3..3));
    }

    #[gpui::test]
    fn deleting_what_is_composed_leaves_the_text_to_type_into(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        compose(cx, &input, "n", 1..1);
        compose(cx, &input, "ni", 2..2);
        compose(cx, &input, "", 0..0);
        assert_eq!(state(cx, &input), (String::new(), 0..0));
        type_text(cx, &input, "x");
        assert_eq!(state(cx, &input), ("x".to_owned(), 1..1));
    }

    #[gpui::test]
    fn the_selection_counts_within_what_is_composed_after_other_text(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        type_text(cx, &input, "abc");
        compose(cx, &input, "中", 1..1);
        assert_eq!(state(cx, &input), ("abc中".to_owned(), 6..6));
        compose(cx, &input, "中文", 1..1);
        assert_eq!(state(cx, &input), ("abc中文".to_owned(), 6..6));
        type_text(cx, &input, "中文");
        assert_eq!(state(cx, &input), ("abc中文".to_owned(), 9..9));
    }

    #[gpui::test]
    fn ranges_out_of_the_text_are_kept_in_it(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        type_text(cx, &input, "aé");
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                let mut actual = None;
                // IME may ask for a range backwards.
                let backwards = Range { start: 2, end: 0 };
                let text = input.text_for_range(backwards, &mut actual, window, cx);
                assert_eq!(text.as_deref(), Some("aé"));
                assert_eq!(actual, Some(0..2));
                input.selected_range = 2..9;
                input.replace_text_in_range(None, "!", window, cx);
            });
        });
        assert_eq!(state(cx, &input), ("a!".to_owned(), 2..2));
    }

    #[test]
    fn offsets_go_between_utf16_and_bytes_through_wide_characters() {
        // Where each character starts, in bytes and in UTF-16, and the end.
        let bytes = [0, 1, 3, 6, 10, 11];
        let units = [0, 1, 2, 3, 5, 6];
        for (byte, unit) in bytes.into_iter().zip(units) {
            assert_eq!(utf16_offset(WIDE, byte), unit, "byte {byte}");
            assert_eq!(utf8_offset(WIDE, unit), byte, "unit {unit}");
        }
        // Inside a character, on to its end.
        assert_eq!(utf16_offset(WIDE, 2), 2, "inside é");
        assert_eq!(utf16_offset(WIDE, 7), 5, "inside 😀");
        assert_eq!(utf8_offset(WIDE, 4), 10, "between 😀's surrogates");
        // Past the end, the end.
        assert_eq!(utf16_offset(WIDE, 99), 6);
        assert_eq!(utf8_offset(WIDE, 99), 11);
        assert_eq!(utf16_offset("", 0), 0);
        assert_eq!(utf8_offset("", 5), 0);
    }

    #[test]
    fn ranges_are_kept_in_order_on_characters_and_in_the_text() {
        assert_eq!(within(WIDE, 0..11), 0..11);
        assert_eq!(within(WIDE, Range { start: 6, end: 3 }), 3..6, "backwards");
        assert_eq!(
            within(WIDE, 2..8),
            1..6,
            "inside é and 😀, back to their starts"
        );
        assert_eq!(within(WIDE, 4..99), 3..11);
        assert_eq!(within("", Range { start: 3, end: 1 }), 0..0);
        assert_eq!(utf8_range(WIDE, &(3..5)), 6..10, "😀, in UTF-16");
        assert_eq!(utf8_range(WIDE, &Range { start: 5, end: 3 }), 6..10);
        assert_eq!(utf8_range(WIDE, &(4..4)), 10..10);
        assert_eq!(utf8_range("", &(0..2)), 0..0);
    }

    proptest! {
        #[test]
        fn any_utf16_offset_lands_on_a_character_and_each_goes_there_and_back(
            text in any::<String>(),
            offset in 0usize..64,
        ) {
            let byte = utf8_offset(&text, offset);
            prop_assert!(text.is_char_boundary(byte));
            prop_assert!(utf16_offset(&text, byte) >= offset.min(text.encode_utf16().count()));
            for (start, _) in text.char_indices() {
                prop_assert_eq!(utf8_offset(&text, utf16_offset(&text, start)), start);
            }
        }

        #[test]
        fn any_range_slices_the_text(text in any::<String>(), start in 0usize..64, end in 0usize..64) {
            let range = within(&text, start..end);
            prop_assert!(range.start <= range.end);
            let _ = &text[range];
            let _ = &text[utf8_range(&text, &(start..end))];
        }
    }

    #[gpui::test]
    fn backspace_and_delete_take_whole_graphemes(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        // An accent written as a character of its own, a family of three
        // people joined into one emoji, and a CJK character.
        let text = "ae\u{301}👨‍👩‍👧中";
        type_text(cx, &input, text);
        for left in ["ae\u{301}👨‍👩‍👧", "ae\u{301}", "a", "", ""] {
            edit(cx, &input, |input, window, cx| {
                input.backspace(&Backspace, window, cx)
            });
            assert_eq!(state(cx, &input), (left.to_owned(), left.len()..left.len()));
        }
        type_text(cx, &input, text);
        edit(cx, &input, |input, window, cx| {
            input.home(&Home, window, cx)
        });
        for left in ["e\u{301}👨‍👩‍👧中", "👨‍👩‍👧中", "中", "", ""] {
            edit(cx, &input, |input, window, cx| {
                input.delete(&Delete, window, cx)
            });
            assert_eq!(state(cx, &input), (left.to_owned(), 0..0));
        }
    }

    #[gpui::test]
    fn a_selection_turned_round_keeps_its_anchor(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        // 中 is bytes 2 to 5, 😀 5 to 9; in UTF-16, 2 and 3 to 5.
        type_text(cx, &input, "ab中😀");
        edit(cx, &input, |input, window, cx| {
            input.home(&Home, window, cx);
            input.right(&Right, window, cx);
            input.right(&Right, window, cx);
        });
        let left = |cx: &mut VisualTestContext| {
            edit(cx, &input, |input, window, cx| {
                input.select_left(&SelectLeft, window, cx);
                (input.selected_range.clone(), input.selection_reversed)
            })
        };
        let right = |cx: &mut VisualTestContext| {
            edit(cx, &input, |input, window, cx| {
                input.select_right(&SelectRight, window, cx);
                (input.selected_range.clone(), input.selection_reversed)
            })
        };
        assert_eq!(right(cx), (2..5, false));
        assert_eq!(left(cx), (2..2, false));
        assert_eq!(left(cx), (1..2, true), "past the anchor it turns round");
        let told = edit(cx, &input, |input, window, cx| {
            input.selected_text_range(false, window, cx)
        });
        let told = told.unwrap();
        assert_eq!((told.range, told.reversed), (1..2, true));
        assert_eq!(left(cx), (0..2, true));
        assert_eq!(right(cx), (1..2, true), "the far end moves, not the anchor");
        assert_eq!(right(cx), (2..2, true));
        assert_eq!(right(cx), (2..5, false), "and back again");
        assert_eq!(right(cx), (2..9, false));
        let told = edit(cx, &input, |input, window, cx| {
            input.selected_text_range(false, window, cx)
        });
        assert_eq!(told.unwrap().range, 2..5, "in UTF-16");
        type_text(cx, &input, "x");
        assert_eq!(state(cx, &input), ("abx".to_owned(), 3..3));
    }

    #[gpui::test]
    fn ime_edits_wide_text_by_its_utf16_ranges(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        // In UTF-16, 中 is 0, 文 1, and 😀 2 to 4.
        type_text(cx, &input, "中文😀");
        edit(cx, &input, |input, window, cx| {
            input.replace_and_mark_text_in_range(Some(2..4), "にほん", Some(1..2), window, cx);
        });
        assert_eq!(state(cx, &input), ("中文にほん".to_owned(), 9..12));
        let (marked, read) = edit(cx, &input, |input, window, cx| {
            let mut actual = None;
            let read = input.text_for_range(2..5, &mut actual, window, cx);
            assert_eq!(actual, Some(2..5));
            (input.marked_text_range(window, cx), read)
        });
        assert_eq!(marked, Some(2..5));
        assert_eq!(read.as_deref(), Some("にほん"));
        // Committing replaces what's composed, wherever the cursor is.
        type_text(cx, &input, "日本");
        assert_eq!(state(cx, &input), ("中文日本".to_owned(), 12..12));
        let marked = edit(cx, &input, |input, window, cx| {
            input.marked_text_range(window, cx)
        });
        assert_eq!(marked, None);

        // A range that starts and ends inside surrogate pairs takes the
        // characters it ends in.
        edit(cx, &input, |input, window, cx| {
            input.set_text("😀😀", cx);
            input.replace_text_in_range(Some(1..3), "a", window, cx);
        });
        assert_eq!(state(cx, &input), ("😀a".to_owned(), 5..5));
    }

    #[gpui::test]
    fn ime_into_empty_text_and_with_empty_text(cx: &mut TestAppContext) {
        let (input, cx) = new_input(cx);
        edit(cx, &input, |input, window, cx| {
            let mut actual = None;
            assert_eq!(
                input
                    .text_for_range(0..4, &mut actual, window, cx)
                    .as_deref(),
                Some("")
            );
            assert_eq!(actual, Some(0..0));
            input.replace_and_mark_text_in_range(Some(0..0), "", Some(0..0), window, cx);
            assert_eq!(
                input.marked_text_range(window, cx),
                None,
                "nothing is marked"
            );
            input.replace_and_mark_text_in_range(Some(3..7), "😀", None, window, cx);
            assert_eq!(input.marked_text_range(window, cx), Some(0..2));
        });
        assert_eq!(state(cx, &input), ("😀".to_owned(), 4..4));
        edit(cx, &input, |input, window, cx| {
            input.replace_text_in_range(None, "", window, cx);
            input.replace_text_in_range(Some(5..9), "a", window, cx);
        });
        assert_eq!(state(cx, &input), ("a".to_owned(), 1..1));
    }

    #[gpui::test]
    fn ime_is_told_where_wide_characters_are_drawn(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(Settings::default());
            cx.set_global(UiFont("Plays Test".into()));
        });
        let (input, cx) = cx.add_window_view(|_, cx| TextInput::new("Search", cx));
        let draw = |cx: &mut VisualTestContext| {
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        };
        let field = Bounds::new(point(px(10.), px(20.)), size(px(400.), px(30.)));
        let at = |cx: &mut VisualTestContext, range: Range<usize>| {
            edit(cx, &input, |input, window, cx| {
                input.bounds_for_range(range, field, window, cx)
            })
            .expect("drawn")
        };
        // Empty, its placeholder drawn: the cursor at the start.
        draw(cx);
        let start = at(cx, 0..0);
        assert_eq!(start.size.width, px(0.));
        assert_eq!((start.top(), start.bottom()), (field.top(), field.bottom()));

        // In UTF-16, a is 0, 中 1, and 😀 2 to 4.
        type_text(cx, &input, "a中😀");
        draw(cx);
        let (a, cjk, emoji) = (at(cx, 0..1), at(cx, 1..2), at(cx, 2..4));
        assert_eq!(a.left(), start.left());
        assert!(a.size.width > px(0.) && cjk.size.width > px(0.) && emoji.size.width > px(0.));
        assert_eq!(a.right(), cjk.left());
        assert_eq!(cjk.right(), emoji.left());
        let all = at(cx, 0..4);
        assert_eq!((all.left(), all.right()), (a.left(), emoji.right()));
        assert_eq!(at(cx, Range { start: 4, end: 2 }), emoji, "backwards");
        // Between 😀's surrogates, and past the end: after it.
        for range in [3..3, 9..12] {
            let after = at(cx, range.clone());
            assert_eq!(
                (after.left(), after.size.width),
                (emoji.right(), px(0.)),
                "{range:?}"
            );
        }
    }
}
