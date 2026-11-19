use iced::widget::Id as WidgetId;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Doc {
    pub blocks: Vec<Block>,
    pub headings: Vec<Heading>,
    pub path: Option<PathBuf>,
    pub words: usize,
    pub lines: usize,
}

#[derive(Clone)]
pub struct Block {
    pub id: WidgetId,
    pub kind: BlockKind,
    pub runs: Vec<Run>,
    pub plain: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    Code,
    Quote,
    Rule,
    ListItem { indent: u8, marker: String },
    TableRow { header: bool },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: Option<Link>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    External(String),
    Internal {
        path: PathBuf,
        anchor: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub block: usize,
    pub slug: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMatch {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

pub const QUICKSTART: &str = include_str!("../assets/quickstart.md");

pub fn quickstart() -> Doc {
    let mut doc = parse(QUICKSTART, None);
    doc.path = None;
    doc
}

pub fn load(path: &Path) -> Result<Doc, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("读取失败：{error}"))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(parse(&text, Some(path)))
}

pub fn parse(text: &str, path: Option<&Path>) -> Doc {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut options = Options::empty();
    options.insert(
        Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS,
    );
    let mut builder = Builder::new(path);
    for event in Parser::new_ext(text, options) {
        builder.event(event);
    }
    builder.finish_all(text)
}

struct ListState {
    ordered: bool,
    next: u64,
}

struct Builder<'a> {
    path: Option<&'a Path>,
    blocks: Vec<Block>,
    headings: Vec<Heading>,
    runs: Vec<Run>,
    kind: BlockKind,
    started: bool,
    metadata: usize,
    quote: usize,
    lists: Vec<ListState>,
    table_header: bool,
    bold: usize,
    italic: usize,
    strike: usize,
    link: Option<Link>,
    code_text: String,
    image_alt: Option<String>,
}

impl<'a> Builder<'a> {
    fn new(path: Option<&'a Path>) -> Self {
        Self {
            path,
            blocks: Vec::new(),
            headings: Vec::new(),
            runs: Vec::new(),
            kind: BlockKind::Paragraph,
            started: false,
            metadata: 0,
            quote: 0,
            lists: Vec::new(),
            table_header: false,
            bold: 0,
            italic: 0,
            strike: 0,
            link: None,
            code_text: String::new(),
            image_alt: None,
        }
    }

    fn event(&mut self, event: Event<'_>) {
        if self.metadata > 0 {
            match event {
                Event::Start(Tag::MetadataBlock(_)) | Event::Start(Tag::HtmlBlock) => {
                    self.metadata += 1;
                }
                Event::End(TagEnd::MetadataBlock(_)) | Event::End(TagEnd::HtmlBlock) => {
                    self.metadata = self.metadata.saturating_sub(1);
                }
                _ => {}
            }
            return;
        }

        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => {
                if let Some(alt) = self.image_alt.as_mut() {
                    alt.push_str(&text);
                } else {
                    self.push_text(&text);
                }
            }
            Event::Code(text) => self.push_run(&text, true),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.push_text("\n"),
            Event::Rule => {
                self.finish();
                self.push_block(BlockKind::Rule, Vec::new());
            }
            Event::TaskListMarker(checked) => {
                self.push_text(if checked { "[x] " } else { "[ ] " });
            }
            Event::Html(_) | Event::InlineHtml(_) | Event::FootnoteReference(_) => {}
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if !self.started {
                    let kind = if self.quote > 0 {
                        BlockKind::Quote
                    } else {
                        BlockKind::Paragraph
                    };
                    self.start(kind);
                }
            }
            Tag::Heading { level, .. } => {
                self.finish();
                self.start(BlockKind::Heading(heading_level(level)));
            }
            Tag::CodeBlock(_) => {
                self.finish();
                self.start(BlockKind::Code);
                self.code_text.clear();
            }
            Tag::BlockQuote(_) => {
                self.finish();
                self.quote += 1;
            }
            Tag::List(start) => {
                self.lists.push(ListState {
                    ordered: start.is_some(),
                    next: start.unwrap_or(1),
                });
            }
            Tag::Item => {
                self.finish();
                let indent = self.lists.len().saturating_sub(1) as u8;
                let marker = match self.lists.last_mut() {
                    Some(list) if list.ordered => {
                        let marker = format!("{}. ", list.next);
                        list.next += 1;
                        marker
                    }
                    _ => String::from("• "),
                };
                self.start(BlockKind::ListItem { indent, marker });
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                self.link = resolve_link(&dest_url, self.path);
            }
            Tag::Image { dest_url, .. } => {
                let _ = dest_url;
                self.image_alt = Some(String::new());
            }
            Tag::Table(_) => self.finish(),
            Tag::TableHead => {
                self.finish();
                self.table_header = true;
                self.start(BlockKind::TableRow { header: true });
            }
            Tag::TableRow => {
                self.finish();
                self.start(BlockKind::TableRow {
                    header: self.table_header,
                });
            }
            Tag::TableCell => {
                if self.started && !self.runs.is_empty() {
                    self.push_run("  │  ", false);
                }
            }
            Tag::MetadataBlock(_) => self.metadata += 1,
            Tag::HtmlBlock => self.metadata += 1,
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                if self.started {
                    self.finish();
                }
            }
            TagEnd::Heading(_) => self.finish(),
            TagEnd::CodeBlock => self.finish(),
            TagEnd::BlockQuote(_) => {
                self.finish();
                self.quote = self.quote.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.finish();
                self.lists.pop();
            }
            TagEnd::Item => self.finish(),
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = None,
            TagEnd::Image => {
                if let Some(alt) = self.image_alt.take() {
                    let label = alt.trim();
                    if label.is_empty() {
                        self.push_text("[图片]");
                    } else {
                        self.push_text(&format!("[图片：{label}]"));
                    }
                }
            }
            TagEnd::TableHead => {
                self.table_header = false;
                self.finish();
            }
            TagEnd::TableRow => self.finish(),
            TagEnd::TableCell => {}
            TagEnd::Table => self.finish(),
            TagEnd::MetadataBlock(_) => self.metadata = self.metadata.saturating_sub(1),
            TagEnd::HtmlBlock => self.metadata = self.metadata.saturating_sub(1),
            _ => {}
        }
    }

    fn start(&mut self, kind: BlockKind) {
        if self.started {
            self.finish();
        }
        self.kind = kind;
        self.started = true;
        self.runs.clear();
        self.code_text.clear();
    }

    fn push_text(&mut self, text: &str) {
        if self.metadata > 0 || text.is_empty() {
            return;
        }
        if !self.started {
            let kind = if self.quote > 0 {
                BlockKind::Quote
            } else {
                BlockKind::Paragraph
            };
            self.start(kind);
        }
        if matches!(self.kind, BlockKind::Code) {
            self.code_text.push_str(text);
        } else {
            self.push_run(text, false);
        }
    }

    fn push_run(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        if matches!(self.kind, BlockKind::Code) {
            self.code_text.push_str(text);
            return;
        }
        let run = Run {
            text: text.to_string(),
            bold: self.bold > 0,
            italic: self.italic > 0,
            code,
            strike: self.strike > 0,
            link: self.link.clone(),
        };
        if let Some(last) = self.runs.last_mut() {
            if last.bold == run.bold
                && last.italic == run.italic
                && last.code == run.code
                && last.strike == run.strike
                && last.link == run.link
            {
                last.text.push_str(&run.text);
                return;
            }
        }
        self.runs.push(run);
    }

    fn finish(&mut self) {
        if !self.started {
            return;
        }
        self.started = false;
        let kind = std::mem::replace(&mut self.kind, BlockKind::Paragraph);
        let mut runs = std::mem::take(&mut self.runs);
        if matches!(kind, BlockKind::Code) {
            let text = std::mem::take(&mut self.code_text);
            if !text.is_empty() {
                runs = vec![Run {
                    text,
                    code: true,
                    ..Run::default()
                }];
            }
        }
        let plain = runs.iter().map(|run| run.text.as_str()).collect::<String>();
        if plain.trim().is_empty() && !matches!(kind, BlockKind::Rule) {
            return;
        }
        let index = self.blocks.len();
        let heading = match kind {
            BlockKind::Heading(level) => Some((level, plain.trim().to_string())),
            _ => None,
        };
        self.blocks.push(Block {
            id: WidgetId::from(format!("block-{index}")),
            kind,
            runs,
            plain,
        });
        if let Some((level, text)) = heading {
            let base = slugify(&text);
            let mut slug = base.clone();
            let mut suffix = 1;
            while self.headings.iter().any(|heading| heading.slug == slug) {
                slug = format!("{base}-{suffix}");
                suffix += 1;
            }
            self.headings.push(Heading {
                level,
                text,
                block: index,
                slug,
            });
        }
    }

    fn push_block(&mut self, kind: BlockKind, runs: Vec<Run>) {
        let index = self.blocks.len();
        let plain = runs.iter().map(|run| run.text.as_str()).collect::<String>();
        self.blocks.push(Block {
            id: WidgetId::from(format!("block-{index}")),
            kind,
            runs,
            plain,
        });
    }

    fn finish_all(mut self, source: &str) -> Doc {
        self.finish();
        let words = count_words(&self.blocks);
        Doc {
            blocks: self.blocks,
            headings: self.headings,
            path: self.path.map(Path::to_path_buf),
            words,
            lines: source.lines().count(),
        }
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn find_matches(doc: &Doc, query: &str) -> Vec<TextMatch> {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (block, item) in doc.blocks.iter().enumerate() {
        let haystack = item.plain.to_lowercase();
        for (start, found) in haystack.match_indices(&needle) {
            out.push(TextMatch {
                block,
                start,
                end: start + found.len(),
            });
            if out.len() >= 5000 {
                return out;
            }
        }
    }
    out
}

pub fn count_words(blocks: &[Block]) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for block in blocks {
        for ch in block.plain.chars() {
            if is_cjk(ch) {
                count += 1;
                in_word = false;
            } else if ch.is_alphanumeric() {
                if !in_word {
                    count += 1;
                    in_word = true;
                }
            } else {
                in_word = false;
            }
        }
    }
    count
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
    )
}

pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            dash = false;
        } else if ch.is_whitespace() || ch == '-' {
            if !out.is_empty() && !dash {
                out.push('-');
                dash = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        String::from("section")
    } else {
        out
    }
}

pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn resolve_link(dest: &str, current: Option<&Path>) -> Option<Link> {
    if dest.is_empty() {
        return None;
    }
    let decoded = percent_decode(dest);
    let lower = decoded.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
    {
        return Some(Link::External(decoded));
    }
    let (file, anchor) = match decoded.split_once('#') {
        Some((file, anchor)) => (file, Some(anchor.to_string())),
        None => (decoded.as_str(), None),
    };
    let file = file.strip_prefix("file:///").unwrap_or(file);
    let base = current
        .and_then(|path| path.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let path = if file.is_empty() {
        current
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else if Path::new(file).is_absolute() {
        PathBuf::from(file)
    } else {
        base.join(file)
    };
    Some(Link::Internal { path, anchor })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn headings_and_slugs() {
        let doc = parse(
            "# 第一章 概述\n\n正文\n\n## API 说明\n\n## API 说明\n",
            None,
        );
        assert_eq!(doc.headings.len(), 3);
        assert_eq!(doc.headings[0].text, "第一章 概述");
        assert_eq!(doc.headings[0].slug, "第一章-概述");
        assert_eq!(doc.headings[1].slug, "api-说明");
        assert_eq!(doc.headings[2].slug, "api-说明-1");
    }

    #[test]
    fn percent_and_links() {
        assert_eq!(percent_decode("a%20b%E4%B8%AD"), "a b中");
        let link = resolve_link("./x.md#%E4%B8%80", Some(Path::new("D:/a/b.md")));
        assert!(matches!(
            link,
            Some(Link::Internal { path, anchor })
                if path.ends_with("x.md") && anchor.as_deref() == Some("一")
        ));
    }

    #[test]
    fn search_is_case_insensitive() {
        let doc = parse("Hello 世界 hello\n", None);
        let matches = find_matches(&doc, "hello");
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn quote_ends_and_table_rows_are_typed() {
        let doc = parse(
            "> 引用\n\n## 标题\n\n| 列一 | 列二 |\n| --- | --- |\n| 甲 | 乙 |\n",
            None,
        );
        assert!(matches!(doc.blocks[0].kind, BlockKind::Quote));
        assert!(matches!(doc.blocks[1].kind, BlockKind::Heading(2)));
        assert!(matches!(
            doc.blocks[2].kind,
            BlockKind::TableRow { header: true }
        ));
        assert!(matches!(
            doc.blocks[3].kind,
            BlockKind::TableRow { header: false }
        ));
    }
}
