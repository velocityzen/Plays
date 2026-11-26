//! ASS and SSA subtitles' own looks: a track's header (the size its
//! script is in, and its styles), and the override tags of its dialogue
//! lines (`{\c&H0000FF&}`, `{\an7\pos(20,30)}`, `{\k20}`), which set how
//! each part of a line looks and where the line goes. Without a header, as
//! for every other text format FFmpeg turns into ASS, only the words, line
//! breaks, italics, bold and placement are kept.

use std::sync::Arc;

use super::subtitles::{CharStyle, Karaoke, Look, Placement, Styled, Text, build_text};

/// What a script is drawn on when its header doesn't say (libass's).
const CANVAS: (f32, f32) = (384.0, 288.0);
/// The sizes a script can say it's drawn on.
const SIZES: std::ops::RangeInclusive<f32> = 1.0..=65_536.0;
/// The fields of a style line when the header doesn't list them.
const FORMAT: &str = "Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
                      BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, \
                      Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, \
                      Encoding";
/// The override tags known, longest first, so each is told from those it
/// starts with (`\bord` from `\b`). Those not read here are skipped.
const TAGS: &[&str] = &[
    "iclip", "alpha", "xbord", "ybord", "xshad", "yshad", "fscx", "fscy", "bord", "shad", "blur",
    "clip", "fade", "move", "pos", "org", "fsp", "frx", "fry", "frz", "fad", "pbo", "an", "fn",
    "fs", "fr", "fe", "fax", "fay", "be", "kf", "ko", "1c", "2c", "3c", "4c", "1a", "2a", "3a",
    "4a", "b", "i", "u", "s", "c", "a", "k", "K", "q", "r", "t", "p",
];

/// A track's header: the size its positions and sizes are in, and its
/// styles.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub canvas: (f32, f32),
    styles: Vec<Style>,
}

/// A style a script names, as its lines start out.
#[derive(Clone, Debug, PartialEq)]
struct Style {
    name: String,
    font: Arc<str>,
    size: f32,
    /// Colors, 0xRRGGBBAA: the fill, the fill until sung, the outline (or
    /// box), and the shadow.
    fill: u32,
    secondary: u32,
    outline_color: u32,
    shadow_color: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
    /// A box behind each row rather than an outline.
    boxed: bool,
    outline: f32,
    shadow: f32,
    /// As a numeric keypad lays it out.
    alignment: u8,
    margins: [f32; 3],
}

impl Default for Style {
    /// libass's, for a line whose style the script doesn't have.
    fn default() -> Self {
        Self {
            name: "Default".to_owned(),
            font: "Arial".into(),
            size: 18.0,
            fill: 0xffff_ffff,
            secondary: 0x00ff_ffff,
            outline_color: 0x0000_00ff,
            shadow_color: 0x0000_007f,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            boxed: false,
            outline: 2.0,
            shadow: 2.0,
            alignment: 2,
            margins: [20.0; 3],
        }
    }
}

impl Script {
    /// The `[Script Info]` and `[V4+ Styles]` (or SSA's `[V4 Styles]`) of
    /// `header`; what they leave out, as libass takes it.
    pub fn parse(header: &str) -> Self {
        let (mut width, mut height) = (None, None);
        let mut styles = Vec::new();
        let mut section = String::new();
        let mut format = fields(FORMAT);
        for line in header.lines().map(str::trim) {
            if line.starts_with('[') {
                section = line.to_ascii_lowercase();
                format = fields(FORMAT);
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            match (section.as_str(), key.as_str()) {
                ("[script info]", "playresx") => {
                    width = number(value).filter(|w| SIZES.contains(w))
                }
                ("[script info]", "playresy") => {
                    height = number(value).filter(|h| SIZES.contains(h))
                }
                ("[v4+ styles]" | "[v4 styles]", "format") => format = fields(value),
                ("[v4+ styles]" | "[v4 styles]", "style") => {
                    styles.push(style(&format, value, section == "[v4 styles]"));
                }
                _ => {}
            }
        }
        let canvas = match (width, height) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) if width == 1280.0 => (width, 1024.0),
            (Some(width), None) => (width, width * 3.0 / 4.0),
            (None, Some(height)) if height == 1024.0 => (1280.0, height),
            (None, Some(height)) => (height * 4.0 / 3.0, height),
            (None, None) => CANVAS,
        };
        Self { canvas, styles }
    }

    /// The style `name`d, as libass finds it: a leading `*` aside, in any
    /// case.
    fn named(&self, name: &str) -> Option<Style> {
        let name = name.trim().trim_start_matches('*');
        self.styles
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case(name))
            .cloned()
    }

    /// The style `name`d, else the script's Default, else libass's.
    fn style(&self, name: &str) -> Style {
        self.named(name)
            .or_else(|| self.named("Default"))
            .unwrap_or_default()
    }
}

/// The names of the fields a `Format:` line lists, lower case.
fn fields(format: &str) -> Vec<String> {
    format
        .split(',')
        .map(|field| field.trim().to_ascii_lowercase())
        .collect()
}

/// A `Style:` line's values, in `format`'s fields. SSA's (`legacy`)
/// numbers its alignments its own way and calls the outline's color the
/// tertiary one.
fn style(format: &[String], values: &str, legacy: bool) -> Style {
    let mut style = Style::default();
    for (field, value) in format.iter().zip(values.splitn(format.len(), ',')) {
        let value = value.trim();
        let number = number(value);
        match (field.as_str(), number) {
            ("name", _) => value.clone_into(&mut style.name),
            ("fontname", _) => style.font = value.into(),
            ("fontsize", Some(size)) if size > 0.0 => style.size = size,
            ("primarycolour", _) => set_color(&mut style.fill, value),
            ("secondarycolour", _) => set_color(&mut style.secondary, value),
            ("outlinecolour" | "tertiarycolour", _) => set_color(&mut style.outline_color, value),
            ("backcolour", _) => set_color(&mut style.shadow_color, value),
            ("bold", Some(bold)) => style.bold = is_bold(bold),
            ("italic", Some(on)) => style.italic = on != 0.0,
            ("underline", Some(on)) => style.underline = on != 0.0,
            ("strikeout", Some(on)) => style.strikeout = on != 0.0,
            ("borderstyle", Some(border)) => style.boxed = border == 3.0 || border == 4.0,
            ("outline", Some(width)) if width >= 0.0 => style.outline = width,
            ("shadow", Some(offset)) if offset >= 0.0 => style.shadow = offset,
            ("alignment", Some(alignment)) => {
                let alignment = alignment as u8;
                style.alignment = match (legacy, alignment) {
                    // As VSFilter takes those SSA has no place for.
                    (true, 8) => from_legacy(3),
                    (true, 4) => from_legacy(11),
                    (true, _) => from_legacy(alignment),
                    (false, 1..=9) => alignment,
                    (false, _) => 2,
                };
            }
            ("marginl", Some(margin)) => style.margins[0] = margin.max(0.0),
            ("marginr", Some(margin)) => style.margins[1] = margin.max(0.0),
            ("marginv", Some(margin)) => style.margins[2] = margin.max(0.0),
            _ => {}
        }
    }
    style
}

fn number(value: &str) -> Option<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|number| number.is_finite())
}

/// Bold, as `-1` and `1` say, or a weight of 600 or more.
fn is_bold(value: f32) -> bool {
    value == -1.0 || value == 1.0 || value >= 600.0
}

/// SSA's alignments, 1 to 3 along the bottom, 5 to 7 the top and 9 to 11
/// the middle, as a numeric keypad lays them out.
fn from_legacy(alignment: u8) -> u8 {
    let across = match alignment & 3 {
        0 => 2,
        across => across,
    };
    let up = if alignment & 4 != 0 {
        6
    } else if alignment & 8 != 0 {
        3
    } else {
        0
    };
    across + up
}

/// A color as ASS writes it, `&HAABBGGRR&` (its alpha 0 for opaque) or a
/// decimal number, as 0xRRGGBBAA.
fn parse_color(value: &str) -> Option<u32> {
    let value = value.trim().trim_end_matches('&');
    let code = match value
        .strip_prefix("&H")
        .or_else(|| value.strip_prefix("&h"))
    {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => value.parse::<i64>().ok()? as u32,
    };
    let [red, green, blue, alpha] = code.to_le_bytes();
    Some(u32::from_be_bytes([red, green, blue, 255 - alpha]))
}

/// Set `color` to `value`, its alpha too.
fn set_color(color: &mut u32, value: &str) {
    if let Some(parsed) = parse_color(value) {
        *color = parsed;
    }
}

/// Set `color`'s red, green and blue to `value`'s, keeping its alpha; to
/// `default`'s if there's no value.
fn set_rgb(color: &mut u32, value: &str, default: u32) {
    let rgb = if value.trim().is_empty() {
        default
    } else {
        match parse_color(value) {
            Some(parsed) => parsed,
            None => return,
        }
    };
    *color = (rgb & 0xffff_ff00) | (*color & 0xff);
}

/// Set `color`'s alpha to `value`'s, `&HAA&`, 0 for opaque.
fn set_alpha(color: &mut u32, value: &str) {
    let value = value.trim().trim_end_matches('&');
    let hex = value
        .strip_prefix("&H")
        .or_else(|| value.strip_prefix("&h"))
        .unwrap_or(value);
    if let Ok(alpha) = u32::from_str_radix(hex, 16) {
        *color = (*color & 0xffff_ff00) | (255 - (alpha & 0xff));
    }
}

/// A dialogue line as FFmpeg gives it:
/// `ReadOrder,Layer,Style,Name,MarginL,MarginR,MarginV,Effect,Text` (older
/// versions gave the whole `Dialogue:` line, with its times).
#[derive(Debug, PartialEq)]
pub struct Dialogue<'a> {
    pub style: &'a str,
    /// Margins the line sets in place of its style's: none where it says 0.
    pub margins: [Option<f32>; 3],
    pub text: &'a str,
}

impl<'a> Dialogue<'a> {
    pub fn parse(line: &'a str) -> Self {
        let (count, style) = if line.starts_with("Dialogue:") {
            (10, 3)
        } else {
            (9, 2)
        };
        let parts: Vec<&str> = line.splitn(count, ',').collect();
        if parts.len() < count {
            return Self {
                style: "",
                margins: [None; 3],
                text: line,
            };
        }
        let margin = |at: usize| number(parts[style + 2 + at]).filter(|margin| *margin > 0.0);
        Self {
            style: parts[style].trim(),
            margins: [margin(0), margin(1), margin(2)],
            text: parts[count - 1],
        }
    }
}

/// How the line reads so far: what the next character looks like.
#[derive(Clone)]
struct State {
    italic: bool,
    bold: bool,
    look: Look,
    font: Arc<str>,
    size: f32,
    outline: f32,
    shadow: f32,
}

impl State {
    fn of(style: &Style) -> Self {
        Self {
            italic: style.italic,
            bold: style.bold,
            look: Look {
                fill: style.fill,
                secondary: style.secondary,
                outline: style.outline_color,
                shadow: style.shadow_color,
                underline: style.underline,
                strikeout: style.strikeout,
                karaoke: None,
            },
            font: style.font.clone(),
            size: style.size,
            outline: style.outline,
            shadow: style.shadow,
        }
    }
}

/// Where a line goes: the first of its `\an` or `\a`, and of its `\pos`
/// or `\move`, wins.
#[derive(Default)]
struct Place {
    alignment: Option<u8>,
    position: Option<(f32, f32)>,
}

/// Karaoke so far: the syllable read now, and when the next starts, in
/// hundredths of a second into the line.
#[derive(Default)]
struct Singing {
    syllable: Option<Karaoke>,
    next: f32,
}

/// ASS dialogue text: override blocks (`{\i1}`) set how what follows looks
/// and where the line goes, `\N` breaks lines, `\h` is a hard space, and
/// drawings are skipped. With the `script` it's a line of, and the line's
/// style and margins, it keeps how the script draws it too.
pub fn parse(source: &str, script: Option<(&Script, &Dialogue)>) -> Option<Text> {
    let style = script.map_or_else(Style::default, |(script, line)| script.style(line.style));
    let mut state = State::of(&style);
    let mut first: Option<State> = None;
    let mut singing = Singing::default();
    let mut place = Place::default();
    let mut chars = Vec::new();
    let mut drawing = false;
    let mut rest = source;
    while let Some(c) = rest.chars().next() {
        if c == '{'
            && let Some(close) = rest.find('}')
        {
            for (name, value) in tags(&rest[1..close]) {
                match name {
                    "p" => drawing = number(value).is_some_and(|n| n != 0.0),
                    "r" => {
                        let named = script.and_then(|(script, _)| script.named(value));
                        state = State::of(named.as_ref().unwrap_or(&style));
                    }
                    "an" | "a" | "pos" | "move" => placed(&mut place, &style, name, value),
                    _ => override_tag(&mut state, &mut singing, &style, name, value),
                }
            }
            rest = &rest[close + 1..];
            continue;
        }
        let (text, len) = match rest.as_bytes() {
            [b'\\', b'N' | b'n', ..] => ('\n', 2),
            [b'\\', b'h', ..] => ('\u{a0}', 2),
            _ => (c, c.len_utf8()),
        };
        if !drawing {
            if !text.is_whitespace() && first.is_none() {
                first = Some(state.clone());
            }
            let look = script.map(|_| Look {
                karaoke: singing.syllable,
                ..state.look
            });
            let style = CharStyle {
                italic: state.italic,
                bold: state.bold,
                look,
            };
            chars.push((text, style));
        }
        rest = &rest[len..];
    }
    let alignment = place.alignment.unwrap_or(style.alignment);
    let placement = match alignment {
        7..=9 => Placement::Top,
        4..=6 => Placement::Middle,
        _ => Placement::Bottom,
    };
    let styled = script.map(|(script, line)| {
        let first = first.unwrap_or(state);
        Styled {
            canvas: script.canvas,
            alignment,
            margins: std::array::from_fn(|at| line.margins[at].unwrap_or(style.margins[at])),
            position: place.position,
            font: first.font,
            size: first.size,
            outline: first.outline,
            shadow: first.shadow,
            boxed: style.boxed,
        }
    });
    build_text(chars, placement, styled)
}

/// The override tags in a block, each its name (one of `TAGS`) and value;
/// a value in parentheses, `\t(\c&HFF&)` say, is kept whole.
fn tags(block: &str) -> Vec<(&str, &str)> {
    let mut tags = Vec::new();
    let mut rest = block;
    while let Some(start) = rest.find('\\') {
        rest = &rest[start + 1..];
        let Some(name) = TAGS.iter().find(|tag| rest.starts_with(**tag)) else {
            continue;
        };
        let after = &rest[name.len()..];
        let end = if after.trim_start().starts_with('(') {
            let mut depth = 0;
            after
                .char_indices()
                .find_map(|(at, c)| {
                    match c {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => return None,
                    }
                    (depth == 0).then_some(at + 1)
                })
                .unwrap_or(after.len())
        } else {
            after.find('\\').unwrap_or(after.len())
        };
        tags.push((*name, &after[..end]));
        rest = &after[end..];
    }
    tags
}

/// Where `\an`, `\a`, `\pos` or `\move` puts the line, if nothing before
/// it in the line has. A `\move` stays where it starts.
fn placed(place: &mut Place, style: &Style, name: &str, value: &str) {
    match name {
        "pos" | "move" if place.position.is_none() => place.position = point(value),
        "an" | "a" if place.alignment.is_none() => {
            let Some(n) = number(value).map(|n| n as u8) else {
                return;
            };
            place.alignment = Some(match (name, n) {
                ("an", 1..=9) => n,
                ("an", _) => style.alignment,
                // As VSFilter takes those SSA has no place for.
                (_, n) if n & 3 == 0 || n == 8 => from_legacy(5),
                (_, n) => from_legacy(n),
            });
        }
        _ => {}
    }
}

/// `(x,y)`, or the first two of more numbers, as `\move` starts.
fn point(value: &str) -> Option<(f32, f32)> {
    let inner = value.trim().strip_prefix('(')?.strip_suffix(')')?;
    let mut numbers = inner.split(',').map(number);
    Some((numbers.next()??, numbers.next()??))
}

/// Apply a tag that changes how what follows looks; with no value, it's
/// back as `style` has it.
fn override_tag(state: &mut State, singing: &mut Singing, style: &Style, name: &str, value: &str) {
    let number = number(value);
    let on = |default: bool| number.map_or(default, |n| n != 0.0);
    let look = &mut state.look;
    match name {
        "i" => state.italic = on(style.italic),
        "b" => state.bold = number.map_or(style.bold, is_bold),
        "u" => look.underline = on(style.underline),
        "s" => look.strikeout = on(style.strikeout),
        "fn" => {
            let font = value.trim();
            state.font = if font.is_empty() {
                style.font.clone()
            } else {
                font.into()
            };
        }
        // Sizes relative to the one before (`\fs+2`) aren't read.
        "fs" if !value.trim_start().starts_with(['+', '-']) => {
            state.size = number.filter(|n| *n > 0.0).unwrap_or(style.size);
        }
        "bord" => state.outline = number.filter(|n| *n >= 0.0).unwrap_or(style.outline),
        "shad" => state.shadow = number.filter(|n| *n >= 0.0).unwrap_or(style.shadow),
        "c" | "1c" => set_rgb(&mut look.fill, value, style.fill),
        "2c" => set_rgb(&mut look.secondary, value, style.secondary),
        "3c" => set_rgb(&mut look.outline, value, style.outline_color),
        "4c" => set_rgb(&mut look.shadow, value, style.shadow_color),
        "alpha" => {
            let colors = [
                &mut look.fill,
                &mut look.secondary,
                &mut look.outline,
                &mut look.shadow,
            ];
            for color in colors {
                set_alpha(color, value);
            }
        }
        "1a" => set_alpha(&mut look.fill, value),
        "2a" => set_alpha(&mut look.secondary, value),
        "3a" => set_alpha(&mut look.outline, value),
        "4a" => set_alpha(&mut look.shadow, value),
        "k" | "K" | "kf" | "ko" => {
            let length = number.unwrap_or(0.0).max(0.0);
            // A sweep is taken to be sung halfway through.
            let sung = match name {
                "K" | "kf" => singing.next + length / 2.0,
                _ => singing.next,
            };
            singing.syllable = Some(Karaoke {
                at: (sung * 10.0).round() as u32,
                outlined: name != "ko",
            });
            singing.next += length;
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "tests/ass.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/ass_properties.rs"]
mod properties;
