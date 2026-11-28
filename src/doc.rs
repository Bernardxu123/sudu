use crate::table::{self, Table};
use iced::widget::Id as WidgetId;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Doc {
    pub blocks: Vec<Block>,
    pub headings: Vec<Heading>,
    pub frontmatter: Option<String>,
    pub path: Option<PathBuf>,
    #[allow(dead_code)]
    pub source: String,
    pub words: usize,
    pub lines: usize,
}

#[derive(Clone)]
pub struct Block {
    pub id: WidgetId,
    pub kind: BlockKind,
    pub runs: Vec<Run>,
    pub plain: String,
    pub span: Range<usize>,
    pub raw: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    Code { lang: String },
    Quote,
    Rule,
    ListItem {
        indent: u8,
        marker: String,
        ordered: bool,
        number: u64,
        checked: Option<bool>,
    },
    Image {
        alt: String,
        src: String,
    },
    Table(Box<Table>),
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
        raw: String,
        path: PathBuf,
        anchor: Option<String>,
    },
}

impl Link {
    pub fn raw_url(&self) -> &str {
        match self {
            Link::External(url) => url,
            Link::Internal { raw, .. } => raw,
        }
    }
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
    let mut builder = Builder::new(path, text);
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        builder.event(event, range);
    }
    builder.finish_all(text)
}

pub fn rebuild_metadata(doc: &mut Doc) {
    doc.headings.clear();
    for (index, block) in doc.blocks.iter_mut().enumerate() {
        block.id = WidgetId::from(format!("block-{index}"));
        if let BlockKind::Table(table) = &mut block.kind {
            table.refresh_plain();
            block.plain = table.plain.clone();
        } else {
            block.plain = block.runs.iter().map(|r| r.text.as_str()).collect();
        }
        if let BlockKind::Heading(level) = block.kind {
            let text = block.plain.trim().to_string();
            let base = slugify(&text);
            let mut slug = base.clone();
            let mut suffix = 1;
            while doc.headings.iter().any(|h| h.slug == slug) {
                slug = format!("{base}-{suffix}");
                suffix += 1;
            }
            doc.headings.push(Heading {
                level,
                text,
                block: index,
                slug,
            });
        }
    }
    doc.words = count_words(&doc.blocks);
}

pub fn serialize(doc: &Doc) -> String {
    let mut out = String::new();
    if let Some(frontmatter) = &doc.frontmatter {
        out.push_str(frontmatter.trim_end_matches(['\r', '\n']));
        out.push_str("\n\n");
    }
    for (index, block) in doc.blocks.iter().enumerate() {
        if index > 0 {
            let prev_list = matches!(doc.blocks[index - 1].kind, BlockKind::ListItem { .. });
            let curr_list = matches!(block.kind, BlockKind::ListItem { .. });
            if prev_list && curr_list {
                out.push('\n');
            } else {
                out.push_str("\n\n");
            }
        }
        if let Some(raw) = &block.raw {
            out.push_str(raw.trim_end_matches(['\r', '\n']));
        } else {
            out.push_str(&serialize_block(block));
        }
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn serialize_block(block: &Block) -> String {
    match &block.kind {
        BlockKind::Heading(level) => {
            let hashes = "#".repeat((*level).clamp(1, 6) as usize);
            format!("{hashes} {}", serialize_runs(&block.runs))
        }
        BlockKind::Paragraph => serialize_runs(&block.runs),
        BlockKind::Code { lang } => {
            let body = block.plain.trim_end_matches('\n');
            if lang.is_empty() {
                format!("```\n{body}\n```")
            } else {
                format!("```{lang}\n{body}\n```")
            }
        }
        BlockKind::Quote => {
            let body = serialize_runs(&block.runs);
            body.split('\n')
                .map(|line| {
                    if line.is_empty() {
                        String::from(">")
                    } else {
                        format!("> {line}")
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        BlockKind::Rule => String::from("---"),
        BlockKind::ListItem {
            indent,
            ordered,
            number,
            checked,
            ..
        } => {
            let pad = "  ".repeat(*indent as usize);
            let prefix = if *ordered {
                format!("{number}. ")
            } else {
                String::from("- ")
            };
            let task = match checked {
                Some(true) => "[x] ",
                Some(false) => "[ ] ",
                None => "",
            };
            format!("{pad}{prefix}{task}{}", serialize_runs(&block.runs))
        }
        BlockKind::Image { alt, src } => format!("![{alt}]({src})"),
        BlockKind::Table(table) => table.to_markdown_or_html(),
    }
}

pub fn serialize_runs(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        let mut piece = if run.code {
            let max_ticks = longest_backtick_run(&run.text);
            let fence = "`".repeat(max_ticks + 1);
            if run.text.starts_with('`') || run.text.ends_with('`') {
                format!("{fence} {} {fence}", run.text)
            } else {
                format!("{fence}{}{fence}", run.text)
            }
        } else {
            run.text.replace('\n', "  \n")
        };
        if !run.code {
            if run.bold && run.italic {
                piece = format!("***{piece}***");
            } else if run.bold {
                piece = format!("**{piece}**");
            } else if run.italic {
                piece = format!("*{piece}*");
            }
            if run.strike {
                piece = format!("~~{piece}~~");
            }
        }
        if let Some(link) = &run.link {
            piece = format!("[{piece}]({})", link.raw_url());
        }
        out.push_str(&piece);
    }
    out
}

fn longest_backtick_run(text: &str) -> usize {
    let mut best = 0usize;
    let mut cur = 0usize;
    for b in text.bytes() {
        if b == b'`' {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

struct ListState {
    ordered: bool,
    next: u64,
}

fn block_span(source: &str, kind: &BlockKind, start: usize, end: usize) -> Range<usize> {
    let bytes = source.as_bytes();
    let mut end = end.max(start).min(source.len());
    while end > start && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    let start = start.min(end);
    match kind {
        BlockKind::Heading(level) => {
            let mut pos = start;
            for _ in 0..*level {
                if bytes.get(pos) == Some(&b'#') {
                    pos += 1;
                }
            }
            if bytes.get(pos) == Some(&b' ') {
                pos += 1;
            }
            pos.min(end)..end
        }
        BlockKind::ListItem { .. } => {
            let mut pos = start;
            while pos < end && bytes[pos] == b' ' {
                pos += 1;
            }
            let marker_end = match bytes.get(pos).copied() {
                Some(b'-' | b'*' | b'+') if bytes.get(pos + 1) == Some(&b' ') => Some(pos + 2),
                Some(b'0'..=b'9') => {
                    let mut digits = pos;
                    while digits < end && bytes[digits].is_ascii_digit() {
                        digits += 1;
                    }
                    match bytes.get(digits).copied() {
                        Some(b'.' | b')') if bytes.get(digits + 1) == Some(&b' ') => {
                            Some(digits + 2)
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            marker_end.unwrap_or(start).min(end)..end
        }
        BlockKind::Code { .. } => {
            let text = &source[start..end];
            let mut body_start = start;
            if let Some(newline) = text.find('\n') {
                let first = text[..newline].trim();
                if first.starts_with("```") || first.starts_with("~~~") {
                    body_start = start + newline + 1;
                }
            }
            let body = &source[body_start..end];
            let mut body_end = end;
            if let Some(newline) = body.rfind('\n') {
                let last = body[newline + 1..].trim();
                if last.starts_with("```") || last.starts_with("~~~") {
                    body_end = body_start + newline;
                }
            } else {
                let last = body.trim();
                if last.starts_with("```") || last.starts_with("~~~") {
                    body_end = body_start;
                }
            }
            body_start..body_end.max(body_start)
        }
        _ => start..end,
    }
}

struct Builder<'a> {
    path: Option<&'a Path>,
    source: &'a str,
    blocks: Vec<Block>,
    headings: Vec<Heading>,
    frontmatter: Option<String>,
    runs: Vec<Run>,
    kind: BlockKind,
    started: bool,
    metadata: usize,
    in_html_block: bool,
    in_table: bool,
    skip_until: usize,
    allow_table_fallback: bool,
    quote: usize,
    lists: Vec<ListState>,
    bold: usize,
    italic: usize,
    strike: usize,
    link: Option<Link>,
    code_text: String,
    image_src: Option<String>,
    image_alt: Option<String>,
    para_images: Vec<(String, String)>,
    para_has_text: bool,
    block_start: usize,
    block_end: usize,
}

impl<'a> Builder<'a> {
    fn new(path: Option<&'a Path>, source: &'a str) -> Self {
        Self {
            path,
            source,
            blocks: Vec::new(),
            headings: Vec::new(),
            frontmatter: None,
            runs: Vec::new(),
            kind: BlockKind::Paragraph,
            started: false,
            metadata: 0,
            in_html_block: false,
            in_table: false,
            skip_until: 0,
            allow_table_fallback: true,
            quote: 0,
            lists: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            link: None,
            code_text: String::new(),
            image_src: None,
            image_alt: None,
            para_images: Vec::new(),
            para_has_text: false,
            block_start: 0,
            block_end: 0,
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        if self.in_html_block {
            if matches!(event, Event::End(TagEnd::HtmlBlock)) {
                self.in_html_block = false;
            }
            return;
        }
        if self.metadata > 0 {
            match event {
                Event::Start(Tag::MetadataBlock(_)) => {
                    self.metadata += 1;
                }
                Event::End(TagEnd::MetadataBlock(_)) => {
                    self.metadata = self.metadata.saturating_sub(1);
                }
                _ => {}
            }
            return;
        }
        if self.skip_until > 0 {
            if range.start < self.skip_until && range.end <= self.skip_until {
                if matches!(event, Event::Start(Tag::HtmlBlock)) {
                    self.in_html_block = true;
                }
                return;
            }
            if range.start >= self.skip_until {
                self.skip_until = 0;
            }
        }
        if self.in_table {
            if matches!(event, Event::End(TagEnd::Table)) {
                self.in_table = false;
            }
            return;
        }

        match event {
            Event::Start(Tag::MetadataBlock(_)) => {
                self.finish();
                if self.frontmatter.is_none() && range.end <= self.source.len() {
                    self.frontmatter = Some(self.source[range.clone()].to_string());
                }
                self.metadata += 1;
            }
            Event::Start(Tag::Table(_)) => {
                self.finish();
                let end = range.end.min(self.source.len());
                let start = range.start.min(end);
                if let Some(tbl) =
                    table::parse_gfm_table(&self.source[start..end], self.quote, self.path)
                {
                    let plain = tbl.plain.clone();
                    let raw = Some(self.source[start..end].trim_end_matches(['\r', '\n']).to_string());
                    self.push_block_with_plain(
                        BlockKind::Table(Box::new(tbl)),
                        Vec::new(),
                        start..end,
                        plain,
                        raw,
                    );
                    self.in_table = true;
                }
            }
            Event::Start(Tag::HtmlBlock) => {
                self.finish();
                if let Some((abs_start, abs_end, tbl)) =
                    table::try_extract_html_table(self.source, range.start, range.end, self.path)
                {
                    let plain = tbl.plain.clone();
                    let raw = Some(
                        self.source[abs_start..abs_end]
                            .trim_end_matches(['\r', '\n'])
                            .to_string(),
                    );
                    self.push_block_with_plain(
                        BlockKind::Table(Box::new(tbl)),
                        Vec::new(),
                        abs_start..abs_end,
                        plain,
                        raw,
                    );
                    self.skip_until = abs_end;
                } else if let Some((alt, src)) =
                    try_extract_standalone_html_img(&self.source[range.start..range.end.min(self.source.len())])
                {
                    let raw = Some(
                        self.source[range.start..range.end.min(self.source.len())]
                            .trim_end_matches(['\r', '\n'])
                            .to_string(),
                    );
                    let plain = format_image_placeholder(&alt);
                    self.push_block_with_plain(
                        BlockKind::Image { alt, src },
                        Vec::new(),
                        range.clone(),
                        plain,
                        raw,
                    );
                }
                self.in_html_block = true;
            }
            Event::Start(tag) => self.start_tag(tag, range.start),
            Event::End(tag) => {
                self.block_end = range.end;
                self.end_tag(tag);
            }
            Event::Text(text) => {
                if let Some(alt) = self.image_alt.as_mut() {
                    alt.push_str(&text);
                } else {
                    if !text.trim().is_empty() {
                        self.para_has_text = true;
                    }
                    self.push_text(&text, range.start);
                }
            }
            Event::Code(text) => {
                self.para_has_text = true;
                self.ensure_start(range.start);
                self.push_run(&text, true);
            }
            Event::SoftBreak => self.push_text(" ", range.start),
            Event::HardBreak => self.push_text("\n", range.start),
            Event::Rule => {
                self.finish();
                let raw = raw_slice(self.source, &range);
                self.push_block(BlockKind::Rule, Vec::new(), range, raw);
            }
            Event::TaskListMarker(checked) => {
                if let BlockKind::ListItem {
                    checked: slot, ..
                } = &mut self.kind
                {
                    *slot = Some(checked);
                } else {
                    self.push_text(if checked { "[x] " } else { "[ ] " }, range.start);
                }
            }
            Event::InlineHtml(html) => {
                if table::is_br_tag(&html) {
                    self.push_text("\n", range.start);
                }
            }
            Event::Html(_) | Event::FootnoteReference(_) => {}
            _ => {}
        }
    }

    fn ensure_start(&mut self, start: usize) {
        if !self.started {
            let kind = if self.quote > 0 {
                BlockKind::Quote
            } else {
                BlockKind::Paragraph
            };
            self.start(kind, start);
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>, start: usize) {
        match tag {
            Tag::Paragraph => {
                if !self.started {
                    let kind = if self.quote > 0 {
                        BlockKind::Quote
                    } else {
                        BlockKind::Paragraph
                    };
                    self.start(kind, start);
                }
            }
            Tag::Heading { level, .. } => {
                self.finish();
                self.start(BlockKind::Heading(heading_level(level)), start);
            }
            Tag::CodeBlock(kind) => {
                self.finish();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.start(BlockKind::Code { lang }, start);
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
                let (ordered, number, marker) = match self.lists.last_mut() {
                    Some(list) if list.ordered => {
                        let num = list.next;
                        let marker = format!("{num}. ");
                        list.next += 1;
                        (true, num, marker)
                    }
                    _ => (false, 1, String::from("• ")),
                };
                self.start(
                    BlockKind::ListItem {
                        indent,
                        marker,
                        ordered,
                        number,
                        checked: None,
                    },
                    start,
                );
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                self.link = resolve_link(&dest_url, self.path);
            }
            Tag::Image { dest_url, .. } => {
                self.image_src = Some(dest_url.to_string());
                self.image_alt = Some(String::new());
            }
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
                let alt = self.image_alt.take().unwrap_or_default();
                let src = self.image_src.take().unwrap_or_default();
                let label = alt.trim().to_string();
                if matches!(self.kind, BlockKind::Paragraph) && !self.para_has_text {
                    self.para_images.push((label.clone(), src));
                } else {
                    self.para_has_text = true;
                }
                let at = self.block_start;
                self.push_text(&format_image_placeholder(&label), at);
            }
            _ => {}
        }
    }

    fn start(&mut self, kind: BlockKind, start: usize) {
        if self.started {
            self.block_end = start;
            self.finish();
        }
        self.kind = kind;
        self.started = true;
        self.runs.clear();
        self.code_text.clear();
        self.para_images.clear();
        self.para_has_text = false;
        self.block_start = start;
        self.block_end = start;
    }

    fn push_text(&mut self, text: &str, start: usize) {
        if self.metadata > 0 || text.is_empty() {
            return;
        }
        self.ensure_start(start);
        if matches!(self.kind, BlockKind::Code { .. }) {
            self.code_text.push_str(text);
        } else {
            self.push_run(text, false);
        }
    }

    fn push_run(&mut self, text: &str, code: bool) {
        if text.is_empty() {
            return;
        }
        if matches!(self.kind, BlockKind::Code { .. }) {
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
        let span = block_span(self.source, &kind, self.block_start, self.block_end);
        let raw_range = self.block_start.min(self.block_end)..self.block_end.max(self.block_start);
        let mut runs = std::mem::take(&mut self.runs);
        let images = std::mem::take(&mut self.para_images);
        let had_text = self.para_has_text;

        if matches!(kind, BlockKind::Paragraph) && !had_text && images.len() == 1 {
            let (alt, src) = images.into_iter().next().unwrap();
            let plain = format_image_placeholder(&alt);
            let raw = raw_slice(self.source, &raw_range);
            self.push_block_with_plain(
                BlockKind::Image { alt, src },
                Vec::new(),
                span,
                plain,
                raw,
            );
            return;
        }

        if self.allow_table_fallback
            && matches!(
                kind,
                BlockKind::Paragraph | BlockKind::Quote | BlockKind::ListItem { .. }
            )
        {
            if let Some((rel_start, rel_end)) =
                table::find_gfm_table_in_lines(&self.source[span.clone()], self.quote)
            {
                let tbl_start = span.start + rel_start;
                let tbl_end = span.start + rel_end;
                if let Some(tbl) = table::parse_gfm_table(
                    &self.source[tbl_start..tbl_end],
                    self.quote,
                    self.path,
                ) {
                    if rel_start > 0 {
                        self.emit_subslice(span.start, tbl_start, Some(&kind));
                    }
                    let plain = tbl.plain.clone();
                    let raw = raw_slice(self.source, &(tbl_start..tbl_end));
                    self.push_block_with_plain(
                        BlockKind::Table(Box::new(tbl)),
                        Vec::new(),
                        tbl_start..tbl_end,
                        plain,
                        raw,
                    );
                    if tbl_end < span.end {
                        self.emit_subslice(tbl_end, span.end, None);
                    }
                    return;
                }
            }
        }

        if matches!(kind, BlockKind::Code { .. }) {
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
        if plain.trim().is_empty() && !matches!(kind, BlockKind::Rule | BlockKind::Code { .. }) {
            return;
        }
        let index = self.blocks.len();
        let heading = match kind {
            BlockKind::Heading(level) => Some((level, plain.trim().to_string())),
            _ => None,
        };
        let raw = if matches!(kind, BlockKind::ListItem { .. }) {
            None
        } else {
            raw_slice(self.source, &raw_range)
        };
        self.blocks.push(Block {
            id: WidgetId::from(format!("block-{index}")),
            kind,
            runs,
            plain,
            span,
            raw,
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

    fn emit_subslice(
        &mut self,
        start: usize,
        end: usize,
        override_kind: Option<&BlockKind>,
    ) {
        let slice = &self.source[start..end];
        if slice.trim().is_empty() {
            return;
        }
        let mut options = Options::empty();
        options.insert(
            Options::ENABLE_TABLES
                | Options::ENABLE_STRIKETHROUGH
                | Options::ENABLE_TASKLISTS,
        );
        let mut sub = Builder::new(self.path, slice);
        sub.allow_table_fallback = override_kind.is_none();
        sub.quote = self.quote;
        for (event, range) in Parser::new_ext(slice, options).into_offset_iter() {
            sub.event(event, range);
        }
        sub.finish();
        for mut block in sub.blocks {
            let index = self.blocks.len();
            block.id = WidgetId::from(format!("block-{index}"));
            block.span = (start + block.span.start)..(start + block.span.end);
            block.raw = None;
            if let Some(kind) = override_kind {
                if matches!(block.kind, BlockKind::Paragraph) {
                    block.kind = kind.clone();
                }
            } else if self.quote > 0 && matches!(block.kind, BlockKind::Paragraph) {
                block.kind = BlockKind::Quote;
            }
            self.blocks.push(block);
        }
    }

    fn push_block(
        &mut self,
        kind: BlockKind,
        runs: Vec<Run>,
        span: Range<usize>,
        raw: Option<String>,
    ) {
        let plain = runs.iter().map(|run| run.text.as_str()).collect::<String>();
        self.push_block_with_plain(kind, runs, span, plain, raw);
    }

    fn push_block_with_plain(
        &mut self,
        kind: BlockKind,
        runs: Vec<Run>,
        span: Range<usize>,
        plain: String,
        raw: Option<String>,
    ) {
        let index = self.blocks.len();
        self.blocks.push(Block {
            id: WidgetId::from(format!("block-{index}")),
            kind,
            runs,
            plain,
            span,
            raw,
        });
    }

    fn finish_all(mut self, source: &str) -> Doc {
        self.finish();
        let words = count_words(&self.blocks);
        Doc {
            blocks: self.blocks,
            headings: self.headings,
            frontmatter: self.frontmatter,
            path: self.path.map(Path::to_path_buf),
            source: source.to_string(),
            words,
            lines: source.lines().count(),
        }
    }
}

fn raw_slice(source: &str, range: &Range<usize>) -> Option<String> {
    let start = range.start.min(source.len());
    let end = range.end.max(start).min(source.len());
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return None;
    }
    Some(
        source[start..end]
            .trim_end_matches(['\r', '\n'])
            .to_string(),
    )
}

fn format_image_placeholder(alt: &str) -> String {
    let label = alt.trim();
    if label.is_empty() {
        String::from("[图片]")
    } else {
        format!("[图片：{label}]")
    }
}

fn try_extract_standalone_html_img(html: &str) -> Option<(String, String)> {
    let trimmed = html.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("<img") {
        return None;
    }
    let gt = trimmed.find('>')?;
    if !trimmed[gt + 1..].trim().is_empty() {
        return None;
    }
    let attrs = &trimmed[4..gt];
    let src = extract_html_attr(attrs, "src")?;
    let alt = extract_html_attr(attrs, "alt").unwrap_or_default();
    Some((alt, src))
}

fn extract_html_attr(attrs: &str, name: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let k_start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let key = &attrs[k_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let val = if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i];
            i += 1;
            let v_start = i;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            let slice = &attrs[v_start..i];
            if i < bytes.len() {
                i += 1;
            }
            slice
        } else {
            let v_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            &attrs[v_start..i]
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(val.to_string());
        }
    }
    None
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
        if let BlockKind::Table(table) = &item.kind {
            for cell in &table.cells {
                find_matches_in_str(&cell.plain, &needle, block, cell.plain_range.start, &mut out);
                if out.len() >= 5000 {
                    return out;
                }
            }
            continue;
        }
        find_matches_in_str(&item.plain, &needle, block, 0, &mut out);
        if out.len() >= 5000 {
            return out;
        }
    }
    out
}

fn find_matches_in_str(
    text: &str,
    needle_lower: &str,
    block: usize,
    base_offset: usize,
    out: &mut Vec<TextMatch>,
) {
    if text.is_empty() || needle_lower.is_empty() {
        return;
    }
    let mut lower = String::with_capacity(text.len());
    let mut char_starts = Vec::with_capacity(text.len() + 1);
    let mut char_ends = Vec::with_capacity(text.len() + 1);
    for (orig_start, ch) in text.char_indices() {
        let orig_end = orig_start + ch.len_utf8();
        for (idx, lc) in ch.to_lowercase().enumerate() {
            let l_len = lc.len_utf8();
            for byte_i in 0..l_len {
                char_starts.push(if idx == 0 && byte_i == 0 {
                    Some(orig_start)
                } else {
                    None
                });
                char_ends.push(orig_end);
            }
            lower.push(lc);
        }
    }
    for (l_start, found) in lower.match_indices(needle_lower) {
        let l_end = l_start + found.len();
        if l_end == 0 || l_end > char_ends.len() {
            continue;
        }
        let orig_start = char_starts[l_start].unwrap_or_else(|| {
            char_starts[..l_start]
                .iter()
                .rev()
                .find_map(|s| *s)
                .unwrap_or(0)
        });
        let orig_end = char_ends[l_end - 1];
        if orig_start < orig_end && orig_end <= text.len() {
            out.push(TextMatch {
                block,
                start: base_offset + orig_start,
                end: base_offset + orig_end,
            });
            if out.len() >= 5000 {
                return;
            }
        }
    }
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
    Some(Link::Internal {
        raw: dest.to_string(),
        path,
        anchor,
    })
}

pub fn resolve_image_path(src: &str, current: Option<&Path>) -> Option<PathBuf> {
    let trimmed = src.trim().trim_start_matches('<').trim_end_matches('>');
    if trimmed.is_empty() {
        return None;
    }
    let decoded = percent_decode(trimmed);
    let lower = decoded.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return None;
    }
    let clean = decoded.strip_prefix("file:///").unwrap_or(&decoded);
    let path = Path::new(clean);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else if let Some(base) = current.and_then(|p| p.parent()) {
        Some(base.join(path))
    } else {
        Some(path.to_path_buf())
    }
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
            Some(Link::Internal { raw, path, anchor })
                if raw == "./x.md#%E4%B8%80" && path.ends_with("x.md") && anchor.as_deref() == Some("一")
        ));
    }

    #[test]
    fn search_is_case_insensitive_and_unicode_safe() {
        let doc = parse("Hello 世界 hello\n\nİzmir hello\n", None);
        let matches = find_matches(&doc, "hello");
        assert_eq!(matches.len(), 3);
        let m2 = &matches[2];
        assert_eq!(&doc.blocks[m2.block].plain[m2.start..m2.end], "hello");
        let img_path = resolve_image_path("assets/%E6%9E%B6%E6%9E%84%20%E5%9B%BE.png", Some(Path::new("D:/docs/a.md")));
        assert_eq!(img_path.unwrap(), PathBuf::from("D:/docs").join("assets/架构 图.png"));
    }

    #[test]
    fn blocks_expose_editable_source_spans() {
        let source = "# 标题 **粗**\n\n- 项目一\n\n> 引用\n\n```rs\nlet x = 1;\n```\n";
        let doc = parse(source, None);
        let slices: Vec<&str> = doc
            .blocks
            .iter()
            .map(|block| &source[block.span.clone()])
            .collect();
        assert_eq!(slices, vec!["标题 **粗**", "项目一", "引用", "let x = 1;"]);
    }

    #[test]
    fn quote_ends_and_table_is_structured() {
        let doc = parse(
            "> 引用\n\n## 标题\n\n| 列一 | 列二 |\n| :--- | ---: |\n| 甲 | 乙 |\n",
            None,
        );
        assert!(matches!(doc.blocks[0].kind, BlockKind::Quote));
        assert!(matches!(doc.blocks[1].kind, BlockKind::Heading(2)));
        assert_eq!(doc.blocks.len(), 3);
        let BlockKind::Table(table) = &doc.blocks[2].kind else {
            panic!("expected table block");
        };
        assert_eq!(table.rows, 2);
        assert_eq!(table.cols, 2);
        assert_eq!(
            table.alignments,
            vec![table::CellAlign::Left, table::CellAlign::Right]
        );
        assert!(table.cells[0].header);
        assert!(!table.cells[2].header);
    }

    #[test]
    fn parses_adjacent_blocks_and_code_pipe_header() {
        let md = "# 标题\n| `a | b` | 说明 |\n| --- | --- |\n| 1 | 2 |\n## 下节\n\n- 列表\n| 列1 | 列2 |\n| --- | --- |\n| x | y |\n```rs\nlet k = 1;\n```\n";
        let doc = parse(md, None);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Heading(1)));
        assert!(matches!(doc.blocks[1].kind, BlockKind::Table(_)));
        assert!(matches!(doc.blocks[2].kind, BlockKind::Heading(2)));
        assert!(matches!(doc.blocks[3].kind, BlockKind::ListItem { .. }));
        assert!(matches!(doc.blocks[4].kind, BlockKind::Table(_)));
        assert!(matches!(doc.blocks[5].kind, BlockKind::Code { .. }));

        let matches = find_matches(&doc, "a | b");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].block, 1);
    }

    #[test]
    fn parses_html_table_block_and_ignores_non_table_html() {
        let md = "<div align=\"center\">\n\n# 标题\n\n</div>\n\n<table><tr><th colspan=\"2\">合并</th></tr><tr><td>左</td><td>右</td></tr></table>\n";
        let doc = parse(md, None);
        assert_eq!(doc.blocks.len(), 2);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Heading(1)));
        let BlockKind::Table(table) = &doc.blocks[1].kind else {
            panic!("expected html table");
        };
        assert_eq!(table.cols, 2);
        assert_eq!(table.rows, 2);
        assert_eq!(table.cells[0].colspan, 2);
    }

    #[test]
    fn parses_standalone_image_and_serializes_roundtrip() {
        let md = "---\ntitle: 测试\n---\n\n# 标题\n\n![架构图](assets/%E5%9B%BE.png)\n\n正文 **加粗** 与 [链接](./a.md)\n";
        let mut doc = parse(md, None);
        assert!(doc.frontmatter.is_some());
        assert!(matches!(doc.blocks[1].kind, BlockKind::Image { .. }));
        doc.blocks[2].raw = None;
        let saved = serialize(&doc);
        assert!(saved.contains("title: 测试"));
        assert!(saved.contains("![架构图](assets/%E5%9B%BE.png)"));
        assert!(saved.contains("正文 **加粗** 与 [链接](./a.md)"));
    }

    #[test]
    fn parses_table_showcase_completely() {
        let showcase = include_str!("../assets/table-showcase.md");
        let doc = parse(showcase, None);
        let tables: Vec<&table::Table> = doc
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::Table(table) => Some(table.as_ref()),
                _ => None,
            })
            .collect();
        assert_eq!(tables.len(), 11);
        assert_eq!((tables[0].rows, tables[0].cols), (4, 3));
        assert_eq!((tables[1].rows, tables[1].cols), (5, 4));
        assert_eq!((tables[2].rows, tables[2].cols), (5, 10));
        assert_eq!((tables[3].rows, tables[3].cols), (4, 3));
        assert_eq!((tables[4].rows, tables[4].cols), (6, 3));
        assert_eq!((tables[5].rows, tables[5].cols), (2, 3));
        assert_eq!((tables[6].rows, tables[6].cols), (6, 5));
        assert_eq!((tables[7].rows, tables[7].cols), (2, 2));
        assert_eq!((tables[8].rows, tables[8].cols), (2, 2));
        assert_eq!((tables[9].rows, tables[9].cols), (2, 2));
        assert_eq!((tables[10].rows, tables[10].cols), (5, 4));
        assert!(tables[10].has_rowspan);
    }
}
