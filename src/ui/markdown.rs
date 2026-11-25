//! Markdown, as release notes are written, drawn: headings, paragraphs,
//! lists, block quotes, code blocks and rules; in them strong, emphasis,
//! struck-through and code text, and links, which open. HTML is left out,
//! and an image shows as its text.

use std::ops::Range;

use gpui::prelude::*;
use gpui::{
    AnyElement, FontStyle, FontWeight, HighlightStyle, InteractiveText, SharedString,
    StrikethroughStyle, StyledText, UnderlineStyle, div, px,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::ui::theme::Theme;
use crate::ui::theme::text::TextStyle;

/// The font code is in.
const CODE_FONT: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else {
    "monospace"
};
/// How far each list in a list goes in.
const INDENT: f32 = 16.0;

/// How a run of text looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub strong: bool,
    pub emphasis: bool,
    pub struck: bool,
    pub code: bool,
    pub link: bool,
}

/// A block's text, and how each run of it looks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    /// The runs that aren't plain, in order, none overlapping.
    pub runs: Vec<(Range<usize>, Look)>,
    /// Each link's run, in order, and where it goes.
    pub links: Vec<(Range<usize>, String)>,
}

impl Text {
    fn push(&mut self, text: &str, look: Look) {
        let start = self.text.len();
        self.text.push_str(text);
        if look != Look::default() && !text.is_empty() {
            self.runs.push((start..self.text.len(), look));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    /// Its level, 1 to 6.
    Heading(u8, Text),
    Paragraph(Text),
    /// A list's item: how many lists it's in past the first, and its
    /// number, in a numbered list.
    Item {
        depth: usize,
        number: Option<u64>,
        text: Text,
    },
    Quote(Text),
    Code(String),
    Rule,
}

/// The blocks `markdown` has.
pub fn parse(markdown: &str) -> Vec<Block> {
    let mut parsing = Parsing::default();
    let options = Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(markdown, options) {
        parsing.event(event);
    }
    parsing.blocks
}

#[derive(Default)]
struct Parsing {
    blocks: Vec<Block>,
    /// The text of the block under way.
    text: Option<Text>,
    /// Each list it's in, and its next item's number, if it's numbered.
    lists: Vec<Option<u64>>,
    /// The item under way: its depth and number.
    item: Option<(usize, Option<u64>)>,
    quotes: usize,
    code: Option<String>,
    look: Look,
    /// How many of each look it's in.
    strong: usize,
    emphasis: usize,
    struck: usize,
    /// Each link it's in: where its run starts, and where it goes.
    links: Vec<(usize, String)>,
}

impl Parsing {
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some(code) => code.push_str(&text),
                None => self.push(&text, self.look),
            },
            Event::Code(code) => self.push(
                &code,
                Look {
                    code: true,
                    ..self.look
                },
            ),
            Event::SoftBreak => self.push(" ", self.look),
            Event::HardBreak => self.push("\n", self.look),
            Event::Rule => {
                self.finish();
                self.blocks.push(Block::Rule);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { .. } | Tag::Paragraph => {
                // A paragraph in an item goes on in it.
                if self.item.is_none() {
                    self.finish();
                } else if self.text.as_ref().is_some_and(|text| !text.text.is_empty()) {
                    self.push("\n", Look::default());
                }
            }
            Tag::List(first) => {
                // An item's own text comes before the list in it.
                self.finish();
                self.lists.push(first);
            }
            Tag::Item => {
                self.finish();
                let depth = self.lists.len().saturating_sub(1);
                let number = self.lists.last_mut().and_then(|next| {
                    let number = next.as_mut()?;
                    *number += 1;
                    Some(*number - 1)
                });
                self.item = Some((depth, number));
            }
            Tag::BlockQuote(_) => {
                self.finish();
                self.quotes += 1;
            }
            Tag::CodeBlock(_) => {
                self.finish();
                self.code = Some(String::new());
            }
            Tag::Strong => self.strong += 1,
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strikethrough => self.struck += 1,
            Tag::Link { dest_url, .. } => {
                let at = self.text.as_ref().map_or(0, |text| text.text.len());
                self.links.push((at, dest_url.to_string()));
            }
            _ => {}
        }
        self.restyle();
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(level) => {
                let text = self.text.take().unwrap_or_default();
                if self.item.is_none() {
                    self.blocks.push(Block::Heading(level as u8, text));
                } else {
                    self.text = Some(text);
                }
            }
            TagEnd::Paragraph if self.item.is_none() => self.finish(),
            TagEnd::Item => {
                self.finish();
                self.item = None;
            }
            TagEnd::List(_) => {
                self.finish();
                self.lists.pop();
                // Back in the item the list was in, if it was in one; its
                // text after the list is its own again.
                self.item = (!self.lists.is_empty()).then(|| (self.lists.len() - 1, None));
            }
            TagEnd::BlockQuote(_) => {
                self.finish();
                self.quotes = self.quotes.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.blocks
                        .push(Block::Code(code.trim_end_matches('\n').to_owned()));
                }
            }
            TagEnd::Strong => self.strong = self.strong.saturating_sub(1),
            TagEnd::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            TagEnd::Strikethrough => self.struck = self.struck.saturating_sub(1),
            TagEnd::Link => {
                if let (Some((start, url)), Some(text)) = (self.links.pop(), &mut self.text)
                    && start < text.text.len()
                {
                    text.links.push((start..text.text.len(), url));
                }
            }
            _ => {}
        }
        self.restyle();
    }

    fn restyle(&mut self) {
        self.look = Look {
            strong: self.strong > 0,
            emphasis: self.emphasis > 0,
            struck: self.struck > 0,
            code: false,
            link: !self.links.is_empty(),
        };
    }

    fn push(&mut self, text: &str, look: Look) {
        self.text.get_or_insert_with(Text::default).push(text, look);
    }

    /// The block under way, if it has text, put with the others.
    fn finish(&mut self) {
        let Some(mut text) = self.text.take() else {
            return;
        };
        let trimmed = text.text.trim_end().len();
        text.text.truncate(trimmed);
        text.runs.retain_mut(|(range, _)| trim(range, trimmed));
        text.links.retain_mut(|(range, _)| trim(range, trimmed));
        if text.text.is_empty() {
            return;
        }
        let block = match self.item {
            Some((depth, number)) => {
                // Its number goes with its first block alone.
                self.item = Some((depth, None));
                Block::Item {
                    depth,
                    number,
                    text,
                }
            }
            None if self.quotes > 0 => Block::Quote(text),
            None => Block::Paragraph(text),
        };
        self.blocks.push(block);
    }
}

/// `range` cut at `end`: whether any of it is left.
fn trim(range: &mut Range<usize>, end: usize) -> bool {
    range.end = range.end.min(end);
    range.start < range.end
}

/// `blocks`, drawn, each element's id from `id`.
pub fn render(blocks: &[Block], id: &str, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .children(blocks.iter().enumerate().map(|(index, block)| {
            let id = SharedString::from(format!("{id}-{index}"));
            render_block(block, id, theme)
        }))
        .into_any_element()
}

fn render_block(block: &Block, id: SharedString, theme: &Theme) -> AnyElement {
    match block {
        Block::Heading(level, text) => div()
            .pt_1()
            .when(*level <= 2, |this| this.text_label())
            .when(*level > 2, |this| this.text_sm())
            .font_weight(FontWeight::SEMIBOLD)
            .child(inline(text, id, theme))
            .into_any_element(),
        Block::Paragraph(text) => div()
            .text_sm()
            .child(inline(text, id, theme))
            .into_any_element(),
        Block::Item {
            depth,
            number,
            text,
        } => {
            let marker = number.map_or_else(|| "•".to_owned(), |number| format!("{number}."));
            div()
                .flex()
                .gap_1p5()
                .pl(px(INDENT * *depth as f32))
                .text_sm()
                .child(div().flex_none().text_color(theme.text_muted).child(marker))
                .child(div().flex_1().min_w_0().child(inline(text, id, theme)))
                .into_any_element()
        }
        Block::Quote(text) => div()
            .pl_3()
            .border_l_2()
            .border_color(theme.border)
            .text_sm()
            .text_color(theme.text_muted)
            .child(inline(text, id, theme))
            .into_any_element(),
        Block::Code(code) => div()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .font_family(CODE_FONT)
            .text_xs()
            .child(code.clone())
            .into_any_element(),
        Block::Rule => div().h(px(1.0)).bg(theme.border).into_any_element(),
    }
}

/// A block's text, each run as it looks, its links opening once clicked.
fn inline(text: &Text, id: SharedString, theme: &Theme) -> AnyElement {
    let highlights = text
        .runs
        .iter()
        .map(|(range, look)| (range.clone(), highlight(*look, theme)));
    let styled = StyledText::new(SharedString::from(text.text.clone())).with_highlights(highlights);
    if text.links.is_empty() {
        return styled.into_any_element();
    }
    let ranges = text.links.iter().map(|(range, _)| range.clone()).collect();
    let urls: Vec<String> = text.links.iter().map(|(_, url)| url.clone()).collect();
    InteractiveText::new(id, styled)
        .on_click(ranges, move |index, _, cx| cx.open_url(&urls[index]))
        .into_any_element()
}

fn highlight(look: Look, theme: &Theme) -> HighlightStyle {
    HighlightStyle {
        color: look.link.then_some(theme.accent),
        font_weight: look.strong.then_some(FontWeight::SEMIBOLD),
        font_style: look.emphasis.then_some(FontStyle::Italic),
        background_color: look.code.then_some(theme.border),
        underline: look.link.then(|| UnderlineStyle {
            thickness: px(1.0),
            color: Some(theme.accent),
            wavy: false,
        }),
        strikethrough: look.struck.then(|| StrikethroughStyle {
            thickness: px(1.0),
            color: None,
        }),
        fade_out: None,
    }
}

#[cfg(test)]
#[path = "tests/markdown.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/markdown_properties.rs"]
mod properties;
