use crate::doc::BlockKind;
use iced::advanced::text::highlighter::{Format, Highlighter};
use iced::{Color, Font};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
pub struct MarkStyle {
    pub color: Color,
    pub font: Font,
}

#[derive(Clone, PartialEq)]
pub struct Settings {
    pub lines: Arc<Vec<Vec<(Range<usize>, MarkStyle)>>>,
}

pub struct MarkHighlighter {
    settings: Settings,
    line: usize,
}

impl Highlighter for MarkHighlighter {
    type Settings = Settings;
    type Highlight = MarkStyle;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, MarkStyle)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            settings: settings.clone(),
            line: 0,
        }
    }

    fn update(&mut self, settings: &Self::Settings) {
        self.settings = settings.clone();
    }

    fn change_line(&mut self, line: usize) {
        self.line = line;
    }

    fn highlight_line(&mut self, _line: &str) -> Self::Iterator<'_> {
        let index = self.line;
        self.line += 1;
        self.settings
            .lines
            .get(index)
            .cloned()
            .unwrap_or_default()
            .into_iter()
    }

    fn current_line(&self) -> usize {
        self.line
    }
}

pub fn format(style: &MarkStyle, _theme: &iced::Theme) -> Format<Font> {
    Format {
        color: Some(style.color),
        font: Some(style.font),
    }
}

pub struct StyleSet {
    pub body: Font,
    pub heading: Font,
    pub mono: Font,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
}

pub struct Display {
    pub text: String,
    pub map: Vec<usize>,
    pub styles: Vec<Vec<(Range<usize>, MarkStyle)>>,
}

impl Display {
    pub fn settings(&self) -> Settings {
        Settings {
            lines: Arc::new(self.styles.clone()),
        }
    }
}

struct Frame {
    start: usize,
    end: usize,
    content_start: Option<usize>,
    content_end: usize,
    style: FrameStyle,
}

#[derive(Clone, Copy)]
enum FrameStyle {
    Bold,
    Italic,
    Strike,
    Link,
}

pub fn build(source: &str, kind: &BlockKind, set: &StyleSet) -> Display {
    if matches!(kind, BlockKind::Code) {
        return plain(source);
    }
    if matches!(kind, BlockKind::TableRow { .. }) {
        return table_row(source);
    }
    let mut frames = Vec::new();
    let mut hidden = Vec::new();
    let mut source_styles = Vec::new();
    if matches!(kind, BlockKind::Quote) {
        hide_quote_prefixes(source, &mut hidden);
    }
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => frames.push(frame(range, FrameStyle::Bold)),
                Tag::Emphasis => frames.push(frame(range, FrameStyle::Italic)),
                Tag::Strikethrough => frames.push(frame(range, FrameStyle::Strike)),
                Tag::Link { .. } | Tag::Image { .. } => frames.push(frame(range, FrameStyle::Link)),
                _ => {}
            },
            Event::Text(_) => note_text(&mut frames, &range),
            Event::Code(_) => {
                let bytes = source.as_bytes();
                let mut start = range.start;
                let mut end = range.end;
                while start < end && bytes[start] == b'`' {
                    start += 1;
                }
                while end > start && bytes[end - 1] == b'`' {
                    end -= 1;
                }
                let mut marker_start = range.start;
                while marker_start > 0 && bytes[marker_start - 1] == b'`' {
                    marker_start -= 1;
                }
                let mut marker_end = range.end;
                while marker_end < bytes.len() && bytes[marker_end] == b'`' {
                    marker_end += 1;
                }
                if marker_start < start {
                    hidden.push(marker_start..start);
                }
                if end < marker_end {
                    hidden.push(end..marker_end);
                }
                source_styles.push((
                    start..end,
                    MarkStyle {
                        color: set.accent,
                        font: set.mono,
                    },
                ));
            }
            Event::SoftBreak | Event::HardBreak => note_text(&mut frames, &range),
            Event::End(tag) => {
                if matches!(
                    tag,
                    TagEnd::Strong
                        | TagEnd::Emphasis
                        | TagEnd::Strikethrough
                        | TagEnd::Link
                        | TagEnd::Image
                ) {
                    if let Some(frame) = frames.pop() {
                        if let Some(content_start) = frame.content_start {
                            if frame.start < content_start {
                                hidden.push(frame.start..content_start);
                            }
                            if frame.content_end < frame.end {
                                hidden.push(frame.content_end..frame.end);
                            }
                            source_styles.push((
                                content_start..frame.content_end,
                                mark_style(frame.style, set),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if matches!(kind, BlockKind::Heading(_)) {
        source_styles.push((
            0..source.len(),
            MarkStyle {
                color: set.text,
                font: set.heading,
            },
        ));
    }
    let (text, display_to_source, source_to_display) = conceal(source, hidden);
    let mut styles = Vec::new();
    for (range, style) in source_styles {
        let start = source_to_display[range.start.min(source.len())];
        let end = source_to_display[range.end.min(source.len())];
        if start < end {
            push_style(&mut styles, &text, start..end, style);
        }
    }
    Display {
        map: display_to_source,
        text,
        styles,
    }
}

fn hide_quote_prefixes(source: &str, hidden: &mut Vec<Range<usize>>) {
    let mut offset = 0;
    for line in source.split('\n') {
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let mut end = offset + indent;
        while source[end..].starts_with('>') {
            end += 1;
            if source[end..].starts_with(' ') {
                end += 1;
            }
        }
        if end > offset + indent {
            hidden.push(offset + indent..end);
        }
        offset += line.len() + 1;
    }
}

fn frame(range: Range<usize>, style: FrameStyle) -> Frame {
    Frame {
        start: range.start,
        end: range.end,
        content_start: None,
        content_end: range.start,
        style,
    }
}

fn note_text(frames: &mut [Frame], range: &Range<usize>) {
    for frame in frames {
        frame.content_start = Some(
            frame
                .content_start
                .map_or(range.start, |start| start.min(range.start)),
        );
        frame.content_end = frame.content_end.max(range.end);
    }
}

fn mark_style(style: FrameStyle, set: &StyleSet) -> MarkStyle {
    match style {
        FrameStyle::Bold => MarkStyle {
            color: set.text,
            font: Font {
                weight: iced::font::Weight::Bold,
                ..set.body
            },
        },
        FrameStyle::Italic => MarkStyle {
            color: set.text,
            font: Font {
                style: iced::font::Style::Italic,
                ..set.body
            },
        },
        FrameStyle::Strike => MarkStyle {
            color: set.muted,
            font: set.body,
        },
        FrameStyle::Link => MarkStyle {
            color: set.accent,
            font: set.body,
        },
    }
}

fn plain(source: &str) -> Display {
    Display {
        text: source.to_string(),
        map: (0..=source.len()).collect(),
        styles: Vec::new(),
    }
}

fn table_row(source: &str) -> Display {
    let first = source.find('|');
    let last = source.rfind('|');
    let mut hidden = Vec::new();
    if let Some(first) = first {
        if first == 0 && source[first + 1..].contains('|') {
            hidden.push(first..first + 1);
        }
    }
    if let Some(last) = last {
        if last + 1 == source.len() && first != Some(last) {
            hidden.push(last..last + 1);
        }
    }
    let (text, map, _) = conceal(source, hidden);
    Display {
        text,
        map,
        styles: Vec::new(),
    }
}

fn conceal(source: &str, mut hidden: Vec<Range<usize>>) -> (String, Vec<usize>, Vec<usize>) {
    hidden.retain(|range| range.start < range.end);
    hidden.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in hidden {
        if let Some(last) = merged.last_mut() {
            if range.start <= last.end {
                last.end = last.end.max(range.end);
                continue;
            }
        }
        merged.push(range);
    }
    let mut text = String::new();
    let mut display_to_source = Vec::with_capacity(source.len() + 1);
    let mut source_to_display = vec![0; source.len() + 1];
    let mut range_index = 0;
    let mut position = 0;
    let mut last_visible_end = 0;
    while position < source.len() {
        while range_index < merged.len() && merged[range_index].end <= position {
            range_index += 1;
        }
        if range_index < merged.len() && merged[range_index].start <= position {
            let end = merged[range_index].end.min(source.len());
            for index in position..end {
                source_to_display[index] = text.len();
            }
            position = end;
            continue;
        }
        let Some(character) = source[position..].chars().next() else {
            break;
        };
        for index in position..position + character.len_utf8() {
            source_to_display[index] = text.len();
            display_to_source.push(position);
        }
        text.push(character);
        position += character.len_utf8();
        last_visible_end = position;
        source_to_display[position] = text.len();
    }
    display_to_source.push(last_visible_end);
    source_to_display[source.len()] = text.len();
    (text, display_to_source, source_to_display)
}

fn push_style(
    lines: &mut Vec<Vec<(Range<usize>, MarkStyle)>>,
    text: &str,
    range: Range<usize>,
    style: MarkStyle,
) {
    let mut line_start = 0;
    let mut line_index = 0;
    for line in text.split('\n') {
        let line_end = line_start + line.len();
        let start = range.start.max(line_start);
        let end = range.end.min(line_end);
        if start < end {
            while lines.len() <= line_index {
                lines.push(Vec::new());
            }
            lines[line_index].push((start - line_start..end - line_start, style));
        }
        line_start = line_end + 1;
        line_index += 1;
    }
}

pub fn apply(source: &mut String, map: &[usize], before: &str, after: &str) -> bool {
    if before == after || map.len() != before.len() + 1 {
        return false;
    }
    let mut prefix = 0;
    while prefix < before.len()
        && prefix < after.len()
        && before.as_bytes()[prefix] == after.as_bytes()[prefix]
    {
        prefix += 1;
    }
    while prefix > 0 && !before.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = 0;
    while suffix < before.len() - prefix
        && suffix < after.len() - prefix
        && before.as_bytes()[before.len() - 1 - suffix]
            == after.as_bytes()[after.len() - 1 - suffix]
    {
        suffix += 1;
    }
    while suffix > 0 && !after.is_char_boundary(after.len() - suffix) {
        suffix -= 1;
    }
    let start = map[prefix];
    let end = map[before.len() - suffix];
    let inserted = &after[prefix..after.len() - suffix];
    if start > end || end > source.len() {
        return false;
    }
    source.replace_range(start..end, inserted);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::font::Weight;

    fn set() -> StyleSet {
        StyleSet {
            body: Font::DEFAULT,
            heading: Font {
                weight: Weight::Bold,
                ..Font::DEFAULT
            },
            mono: Font::MONOSPACE,
            text: Color::BLACK,
            muted: Color::from_rgb(0.5, 0.5, 0.5),
            accent: Color::from_rgb(0.1, 0.4, 0.8),
        }
    }

    #[test]
    fn hides_inline_markers_and_styles_text() {
        let display = build("标题 **粗** 与 *斜* 和 `码`", &BlockKind::Paragraph, &set());
        assert_eq!(display.text, "标题 粗 与 斜 和 码");
        assert!(display
            .styles
            .iter()
            .flatten()
            .any(|(_, style)| style.font.weight == Weight::Bold));
        assert!(display
            .styles
            .iter()
            .flatten()
            .any(|(_, style)| style.font.style == iced::font::Style::Italic));
        assert!(display
            .styles
            .iter()
            .flatten()
            .any(|(_, style)| style.font.family == Font::MONOSPACE.family));
    }

    #[test]
    fn hides_link_destination() {
        let display = build(
            "[官网](https://example.test)",
            &BlockKind::Paragraph,
            &set(),
        );
        assert_eq!(display.text, "官网");
        assert!(display
            .styles
            .iter()
            .flatten()
            .any(|(_, style)| style.color == set().accent));
    }

    #[test]
    fn edits_source_through_display_map() {
        let original = "**粗**";
        let mut source = original.to_string();
        let display = build(original, &BlockKind::Paragraph, &set());
        assert_eq!(display.text, "粗");
        assert!(apply(&mut source, &display.map, "粗", "粗体"));
        assert_eq!(source, "**粗体**");
    }

    #[test]
    fn hides_quote_prefixes_and_table_edges() {
        let quote = build("引用一\n> 引用二", &BlockKind::Quote, &set());
        assert_eq!(quote.text, "引用一\n引用二");
        let table = build(
            "| 甲 | 乙 |",
            &BlockKind::TableRow { header: false },
            &set(),
        );
        assert_eq!(table.text, " 甲 | 乙 ");
    }
}
