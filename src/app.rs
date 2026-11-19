use crate::offsets;
use crate::theme::{self, Palette};
use iced::widget::text::LineHeight;
use iced::widget::{button, column, container, row, scrollable, span, text, Column, Space};
use iced::{Alignment, Element, Length, Padding, Task};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    H1,
    H2,
    H3,
    Paragraph,
    Quote,
    Code,
}

#[derive(Clone)]
pub struct Block {
    pub id: iced::widget::Id,
    pub kind: BlockKind,
    pub text: String,
    pub link: Option<String>,
}

#[derive(Clone)]
pub struct BodyData {
    pub blocks: Arc<Vec<Block>>,
    pub pal: Palette,
    pub body_font: iced::Font,
    pub mono_font: iced::Font,
    pub size: f32,
    pub line: f32,
    pub renders: Arc<AtomicU32>,
}

#[derive(Clone)]
pub struct BodyKey {
    pub data: Arc<BodyData>,
    pub doc_rev: u64,
    pub theme: u8,
    pub size_q: u16,
    pub line_q: u16,
    pub width_q: u16,
    pub scale_q: u16,
    pub search_rev: u64,
}

impl Hash for BodyKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.doc_rev.hash(state);
        self.theme.hash(state);
        self.size_q.hash(state);
        self.line_q.hash(state);
        self.width_q.hash(state);
        self.scale_q.hash(state);
        self.search_rev.hash(state);
    }
}

#[derive(Clone)]
pub enum Message {
    NextTheme,
    Measure,
    Offsets(HashMap<usize, f32>),
    Jump(usize),
    Scroll(scrollable::Viewport),
    OpenLink(String),
    ProbeFrame,
}

pub struct App {
    blocks: Arc<Vec<Block>>,
    doc_rev: u64,
    theme_idx: usize,
    offsets: HashMap<usize, f32>,
    pending_jump: Option<usize>,
    scroll_ratio: f32,
    last_link: Option<String>,
    renders: Arc<AtomicU32>,
    start: Instant,
    probe: Option<PathBuf>,
    source_name: String,
}

impl App {
    pub fn new(initial: Option<PathBuf>, start: Instant, probe: Option<PathBuf>) -> Self {
        let (blocks, source_name) = load_blocks(initial.as_deref());
        let theme_idx = std::env::var("SUDU_THEME")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            % 4;
        Self {
            blocks: Arc::new(blocks),
            doc_rev: 1,
            theme_idx,
            offsets: HashMap::new(),
            pending_jump: None,
            scroll_ratio: 0.0,
            last_link: None,
            renders: Arc::new(AtomicU32::new(0)),
            start,
            probe,
            source_name,
        }
    }

    pub fn title(&self) -> String {
        format!("素读 — {}", self.source_name)
    }

    pub fn theme(&self) -> iced::Theme {
        let all = theme::all();
        theme::base_theme(&all[self.theme_idx])
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let keys = iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if !modifiers.command() {
                    return None;
                }
                match key {
                    iced::keyboard::Key::Character(c) if c.as_str() == "t" => {
                        Some(Message::NextTheme)
                    }
                    iced::keyboard::Key::Character(c) if c.as_str() == "m" => {
                        Some(Message::Measure)
                    }
                    iced::keyboard::Key::Character(c) if c.as_str() == "j" => {
                        Some(Message::Jump(3))
                    }
                    _ => None,
                }
            }
            _ => None,
        });
        let probe = if self.probe.is_some() {
            iced::window::events().filter_map(|(_id, event)| match event {
                iced::window::Event::RedrawRequested(_)
                | iced::window::Event::Opened { .. }
                | iced::window::Event::Focused => Some(Message::ProbeFrame),
                _ => None,
            })
        } else {
            iced::Subscription::none()
        };
        iced::Subscription::batch([keys, probe])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NextTheme => {
                self.theme_idx = (self.theme_idx + 1) % 4;
                self.offsets.clear();
                Task::none()
            }
            Message::Measure => self.measure(),
            Message::Offsets(map) => {
                self.offsets = map;
                if let Some(index) = self.pending_jump.take() {
                    if let Some(y) = self.offsets.get(&index).copied() {
                        return offsets::scroll_to_y(y - 8.0);
                    }
                }
                Task::none()
            }
            Message::Jump(index) => {
                if let Some(y) = self.offsets.get(&index).copied() {
                    offsets::scroll_to_y(y - 8.0)
                } else {
                    self.pending_jump = Some(index);
                    self.measure()
                }
            }
            Message::Scroll(viewport) => {
                let offset = viewport.absolute_offset().y;
                let content = viewport.content_bounds().height;
                let view = viewport.bounds().height;
                self.scroll_ratio = if content > view {
                    (offset / (content - view)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Task::none()
            }
            Message::OpenLink(url) => {
                if (url.starts_with("http://") || url.starts_with("https://"))
                    && std::env::var_os("SUDU_NO_OPEN").is_none()
                {
                    let _ = open::that(&url);
                }
                self.last_link = Some(url);
                Task::none()
            }
            Message::ProbeFrame => {
                if let Some(path) = self.probe.take() {
                    let _ = std::fs::write(path, self.start.elapsed().as_millis().to_string());
                }
                Task::none()
            }
        }
    }

    fn measure(&self) -> Task<Message> {
        let targets = self
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.id.clone(), index))
            .collect();
        offsets::collect(targets)
    }

    fn body_key(&self) -> BodyKey {
        let all = theme::all();
        let t = &all[self.theme_idx];
        BodyKey {
            data: Arc::new(BodyData {
                blocks: self.blocks.clone(),
                pal: t.pal,
                body_font: t.body,
                mono_font: t.mono,
                size: t.size,
                line: t.line,
                renders: self.renders.clone(),
            }),
            doc_rev: self.doc_rev,
            theme: self.theme_idx as u8,
            size_q: (t.size * 64.0) as u16,
            line_q: (t.line * 64.0) as u16,
            width_q: t.width as u16,
            scale_q: 100,
            search_rev: 0,
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let all = theme::all();
        let t = &all[self.theme_idx];
        let pal = t.pal;
        let body = iced::widget::lazy(self.body_key(), |key| render_body(key));
        let paper = container(body)
            .width(Length::Fill)
            .max_width(t.width)
            .padding(Padding::from([28.0, 12.0]));
        let centered = container(paper).width(Length::Fill).center_x(Length::Fill);
        let scroller = scrollable(centered)
            .id(offsets::body_id())
            .on_scroll(Message::Scroll)
            .height(Length::Fill);

        let top = container(
            row![
                text("素读").size(18).font(t.ui).color(pal.text),
                text(t.name).size(12).color(pal.muted),
                Space::new().width(Length::Fill),
                button("切换主题").on_press(Message::NextTheme),
                button("测量").on_press(Message::Measure),
                button("跳到第 4 块").on_press(Message::Jump(3)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([10.0, 16.0]))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(pal.panel.into()),
            text_color: Some(pal.text),
            ..Default::default()
        });

        let status = container(
            row![
                text(format!("{:.0}%", self.scroll_ratio * 100.0))
                    .size(12)
                    .color(pal.muted),
                text(format!("offsets {}", self.offsets.len()))
                    .size(12)
                    .color(pal.muted),
                text(format!("renders {}", self.renders.load(Ordering::Relaxed)))
                    .size(12)
                    .color(pal.muted),
                Space::new().width(Length::Fill),
                text(self.last_link.clone().unwrap_or_default())
                    .size(12)
                    .color(pal.muted),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([6.0, 16.0]))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(pal.panel.into()),
            text_color: Some(pal.muted),
            ..Default::default()
        });

        container(column![top, scroller, status].height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(pal.bg.into()),
                text_color: Some(pal.text),
                ..Default::default()
            })
            .into()
    }
}

fn render_body(key: &BodyKey) -> Element<'static, Message> {
    let started = std::time::Instant::now();
    key.data.renders.fetch_add(1, Ordering::Relaxed);
    let mut col = Column::new().spacing(10).width(Length::Fill);
    for block in key.data.blocks.iter() {
        col = col.push(block_view(key, block));
    }
    let element: Element<'static, Message> = col.into();
    if let Some(path) = std::env::var_os("SUDU_TIMING") {
        let _ = std::fs::write(path, format!("render_ms={}", started.elapsed().as_millis()));
    }
    element
}

fn block_view(key: &BodyKey, block: &Block) -> Element<'static, Message> {
    let pal = key.data.pal;
    let (size, mut font, color) = match block.kind {
        BlockKind::H1 => (key.data.size * 1.8, key.data.body_font, pal.text),
        BlockKind::H2 => (key.data.size * 1.4, key.data.body_font, pal.text),
        BlockKind::H3 => (key.data.size * 1.18, key.data.body_font, pal.text),
        BlockKind::Paragraph => (key.data.size, key.data.body_font, pal.text),
        BlockKind::Quote => (key.data.size, key.data.body_font, pal.muted),
        BlockKind::Code => (key.data.size * 0.9, key.data.mono_font, pal.text),
    };
    if matches!(block.kind, BlockKind::H1 | BlockKind::H2 | BlockKind::H3) {
        font.weight = iced::font::Weight::Bold;
    }
    let mut head = span(block.text.clone()).size(size).font(font).color(color);
    if matches!(block.kind, BlockKind::Code) {
        head = head.background(pal.code_bg);
    }
    if let Some(url) = &block.link {
        head = head
            .link(Message::OpenLink(url.clone()))
            .color(pal.accent)
            .underline(true);
    }
    let selection = pal.selection;
    let rich = iced_selection::rich_text(vec![head])
        .on_link_click(|message| message)
        .size(size)
        .line_height(LineHeight::Relative(key.data.line))
        .style(move |_| iced_selection::text::Style {
            color: None,
            selection,
        });
    let pad = match block.kind {
        BlockKind::H1 => Padding {
            top: 20.0,
            right: 0.0,
            bottom: 4.0,
            left: 0.0,
        },
        BlockKind::H2 => Padding {
            top: 12.0,
            right: 0.0,
            bottom: 2.0,
            left: 0.0,
        },
        BlockKind::H3 => Padding {
            top: 8.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        },
        BlockKind::Code => Padding {
            top: 10.0,
            right: 12.0,
            bottom: 10.0,
            left: 12.0,
        },
        BlockKind::Quote => Padding {
            top: 2.0,
            right: 12.0,
            bottom: 2.0,
            left: 12.0,
        },
        BlockKind::Paragraph => Padding::from(0.0),
    };
    let code_bg = pal.code_bg;
    let border_color = pal.border;
    let is_code = matches!(block.kind, BlockKind::Code);
    container(rich)
        .id(block.id.clone())
        .padding(pad)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: if is_code { Some(code_bg.into()) } else { None },
            border: if is_code {
                iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: 4.0.into(),
                }
            } else {
                iced::Border::default()
            },
            ..Default::default()
        })
        .into()
}

fn load_blocks(initial: Option<&Path>) -> (Vec<Block>, String) {
    let Some(path) = initial else {
        return (sample_blocks(), String::from("Gate"));
    };
    if path.is_file() {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                let text = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| String::from("Gate"));
                return (blocks_from_text(text), name);
            }
            Err(error) => {
                let mut blocks = vec![Block {
                    id: iced::widget::Id::from("block-error"),
                    kind: BlockKind::Paragraph,
                    text: format!("无法读取：{}（{}）", path.display(), error),
                    link: None,
                }];
                blocks.extend(sample_blocks());
                return (blocks, String::from("Gate"));
            }
        }
    }
    (sample_blocks(), String::from("Gate"))
}

fn blocks_from_text(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    for part in text.split("\n\n") {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        let index = out.len();
        let kind = if trimmed.starts_with("# ") {
            BlockKind::H1
        } else if trimmed.starts_with("## ") {
            BlockKind::H2
        } else if trimmed.starts_with("### ") {
            BlockKind::H3
        } else if trimmed.starts_with("```") {
            BlockKind::Code
        } else if trimmed.starts_with('>') {
            BlockKind::Quote
        } else {
            BlockKind::Paragraph
        };
        let text = trimmed
            .trim_start_matches('#')
            .trim_start_matches('>')
            .trim()
            .to_string();
        out.push(Block {
            id: iced::widget::Id::from(format!("block-{index}")),
            kind,
            text,
            link: None,
        });
    }
    out
}

fn sample_blocks() -> Vec<Block> {
    let mut texts: Vec<(BlockKind, String)> = vec![
        (BlockKind::H1, String::from("素读 · 渲染骨架")),
        (
            BlockKind::Paragraph,
            String::from(
                "这是素读的第一版骨架。中文排版的重点不在花哨，而在字距、行高、段距和留白。\
                 这一段混合了 English words and numbers 12345，用来检查中西文混排时的基线、\
                 行首行尾禁则和断行位置。",
            ),
        ),
        (BlockKind::H2, String::from("四个主题")),
        (
            BlockKind::Paragraph,
            String::from(
                "白、纸、夜、米黄。纸与米黄使用衬线字体，白与夜使用无衬线字体。\
                 按下 Ctrl+T 可以循环切换，观察字体、底色和选区颜色是否一起变化。",
            ),
        ),
        (BlockKind::H2, String::from("选区与复制")),
        (
            BlockKind::Paragraph,
            String::from(
                "用鼠标拖过这一段文字，应该出现选区高亮；按 Ctrl+C 应复制为纯文本。\
                 然后拖选下一段，前一段的选区应当消失。这是 iced_selection 的关键验证项。",
            ),
        ),
        (
            BlockKind::Paragraph,
            String::from(
                "Second paragraph for cross-block selection testing. 选中这一段时，\
                 上一段的高亮是否立即清除？如果两段同时保留高亮，就需要在阶段②处理。",
            ),
        ),
        (BlockKind::H2, String::from("链接")),
        (
            BlockKind::Paragraph,
            String::from("这是一个站内链接测试段落：打开示例链接。"),
        ),
        (BlockKind::H2, String::from("代码块")),
        (
            BlockKind::Code,
            String::from("fn main() {\n    println!(\"你好，素读\");\n}"),
        ),
        (BlockKind::H2, String::from("引用与长文")),
        (
            BlockKind::Quote,
            String::from("引用：阅读器应当让文字自己说话，界面退到背景里。"),
        ),
    ];
    for index in 0..24 {
        texts.push((
            BlockKind::Paragraph,
            format!(
                "第 {} 段长文测试。中文标点：，。；：？！「」『』（）【】。\
                 英文标点: , . ; : ? ! ( ) [ ]. 数字 2026-11-19。\
                 阅读进度、滚动定位和选区都需要在长文中验证；\
                 这一段会重复出现，用来把页面撑到可以滚动的长度。",
                index + 1
            ),
        ));
    }
    let mut out = Vec::new();
    for (index, (kind, text)) in texts.into_iter().enumerate() {
        let link = if index == 8 {
            Some(String::from("https://example.com/sudu"))
        } else {
            None
        };
        out.push(Block {
            id: iced::widget::Id::from(format!("block-{index}")),
            kind,
            text,
            link,
        });
    }
    out
}
