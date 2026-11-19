use crate::doc::{self, Block, BlockKind, Doc, Link, TextMatch};
use crate::offsets;
use crate::theme::{self, Palette, Theme};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::button::Button;
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    button, column, container, mouse_area, operation, row, scrollable, span, stack, svg, text,
    text_editor, text_input, Column, Space,
};
use iced::{Alignment, Border, Color, Element, Font, Length, Padding, Shadow, Size, Task};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

const SIDEBAR_WIDTH: f32 = 260.0;
const HEADER_HEIGHT: f32 = 40.0;
const STATUS_HEIGHT: f32 = 26.0;
const MIN_CONTENT: f32 = 420.0;

#[derive(Clone)]
pub struct BodyData {
    pub doc: Arc<Doc>,
    pub pal: Palette,
    pub body_font: Font,
    pub mono_font: Font,
    pub size: f32,
    pub line: f32,
    pub search: Option<Arc<SearchView>>,
    pub renders: Arc<AtomicU32>,
}

#[derive(Clone)]
pub struct SearchView {
    pub matches: Arc<Vec<TextMatch>>,
    pub current: Option<usize>,
}

#[derive(Clone)]
pub struct BodyKey {
    pub data: Arc<BodyData>,
    pub doc_rev: u64,
    pub theme: u8,
    pub size_q: u16,
    pub line_q: u16,
    pub search_rev: u64,
}

impl Hash for BodyKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.doc_rev.hash(state);
        self.theme.hash(state);
        self.size_q.hash(state);
        self.line_q.hash(state);
        self.search_rev.hash(state);
    }
}

#[derive(Clone)]
pub struct QuickState {
    pub root: PathBuf,
    pub files: Arc<Vec<PathBuf>>,
    pub query: String,
}

#[derive(Clone)]
pub enum Message {
    NextTheme,
    SetTheme(usize),
    FontInc,
    FontDec,
    ToggleSidebar,
    ToggleSettings,
    ToggleEdit,
    EditorAction(text_editor::Action),
    Save,
    CloseSettings,
    ToggleAbout,
    CloseAbout,
    OpenFind,
    CloseFind,
    FindInput(String),
    FindNext,
    FindPrev,
    OpenFile,
    OpenFolder,
    Reload,
    Minimize,
    ToggleMaximize,
    CloseWindow,
    DragWindow,
    Resize(iced::window::Direction),
    WindowId(Option<iced::window::Id>),
    WindowOpened(iced::window::Id, Size),
    WindowResized(Size),
    WindowCloseRequested,
    Measure,
    Offsets(HashMap<usize, f32>),
    Jump(usize),
    Link(Link),
    Scroll(scrollable::Viewport),
    Wheel(f32),
    WheelPixels(f32),
    ProbeFrame,
    QuickInput(String),
    QuickOpen(PathBuf),
    QuickOpenFirst,
    QuickClose,
    Escape,
    KeyEnter,
    KeyShiftEnter,
    KeyF3,
    KeyShiftF3,
    Noop,
}

pub struct App {
    doc: Arc<Doc>,
    doc_rev: u64,
    path: Option<PathBuf>,
    root: Option<PathBuf>,
    theme_idx: usize,
    font_scale: f32,
    sidebar_open: bool,
    settings_open: bool,
    editing: bool,
    editor: Option<text_editor::Content>,
    dirty: bool,
    edit_ratio: f32,
    about_open: bool,
    find_open: bool,
    find_input: String,
    matches: Arc<Vec<TextMatch>>,
    current_match: Option<usize>,
    search_rev: u64,
    current_heading: Option<usize>,
    offsets: HashMap<usize, f32>,
    pending_jump: Option<usize>,
    pending_anchor: Option<String>,
    viewport: Option<scrollable::Viewport>,
    scroll_ratio: f32,
    anim_current: f32,
    notice: Option<String>,
    last_link: Option<String>,
    quick: Option<QuickState>,
    window_id: Option<iced::window::Id>,
    window_size: Size,
    maximized: bool,
    start: Instant,
    probe: Option<PathBuf>,
    renders: Arc<AtomicU32>,
}

impl App {
    pub fn new(initial: Option<PathBuf>, start: Instant, probe: Option<PathBuf>) -> Self {
        let (text_doc, path, notice) = match initial.as_deref() {
            Some(path) => match doc::load(path) {
                Ok(loaded) => (loaded, Some(path.to_path_buf()), None),
                Err(error) => (doc::quickstart(), None, Some(error)),
            },
            None => (doc::quickstart(), None, None),
        };
        let theme_idx = std::env::var("SUDU_THEME")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0)
            % 4;
        Self {
            doc: Arc::new(text_doc),
            doc_rev: 1,
            path,
            root: None,
            theme_idx,
            font_scale: 1.0,
            sidebar_open: false,
            settings_open: false,
            editing: false,
            editor: None,
            dirty: false,
            edit_ratio: 0.0,
            about_open: false,
            find_open: false,
            find_input: String::new(),
            matches: Arc::new(Vec::new()),
            current_match: None,
            search_rev: 0,
            current_heading: None,
            offsets: HashMap::new(),
            pending_jump: None,
            pending_anchor: None,
            viewport: None,
            scroll_ratio: 0.0,
            anim_current: 0.0,
            notice,
            last_link: None,
            quick: None,
            window_id: None,
            window_size: Size::new(1120.0, 820.0),
            maximized: false,
            start,
            probe,
            renders: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn title(&self) -> String {
        format!("素读 — {}", self.source_name())
    }

    pub fn theme(&self) -> iced::Theme {
        let all = theme::all();
        theme::base_theme(&all[self.theme_idx])
    }

    fn source_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| String::from("快速入门.md"))
    }

    pub fn subscription(&self) -> iced::Subscription<Message> {
        let keys = iced::event::listen_with(key_event);
        let windows = if self.probe.is_some() {
            iced::window::events().filter_map(window_event_probe)
        } else {
            iced::window::events().filter_map(window_event)
        };
        iced::Subscription::batch([keys, windows])
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NextTheme => {
                self.theme_idx = (self.theme_idx + 1) % 4;
                self.after_metrics_change()
            }
            Message::SetTheme(index) => {
                if index < 4 {
                    self.theme_idx = index;
                }
                self.after_metrics_change()
            }
            Message::FontInc => {
                self.font_scale = (self.font_scale + 0.05).min(1.6);
                self.after_metrics_change()
            }
            Message::FontDec => {
                self.font_scale = (self.font_scale - 0.05).max(0.8);
                self.after_metrics_change()
            }
            Message::ToggleSidebar => {
                self.sidebar_open = !self.sidebar_open;
                self.offsets.clear();
                self.current_heading = None;
                offsets::snap_to_ratio(self.scroll_ratio)
            }
            Message::ToggleSettings => {
                self.settings_open = !self.settings_open;
                self.about_open = false;
                self.quick = None;
                Task::none()
            }
            Message::ToggleEdit => {
                if self.editing {
                    self.save_and_finish()
                } else {
                    self.begin_edit()
                }
            }
            Message::EditorAction(action) => {
                if let Some(editor) = self.editor.as_mut() {
                    if action.is_edit() {
                        self.dirty = true;
                    }
                    editor.perform(action);
                }
                Task::none()
            }
            Message::Save => {
                if self.editing {
                    self.save_and_finish()
                } else {
                    Task::none()
                }
            }
            Message::CloseSettings => {
                self.settings_open = false;
                Task::none()
            }
            Message::ToggleAbout => {
                self.about_open = !self.about_open;
                self.settings_open = false;
                Task::none()
            }
            Message::CloseAbout => {
                self.about_open = false;
                Task::none()
            }
            Message::OpenFind => {
                if self.editing {
                    self.notice = Some(String::from("先保存并退出编辑，再在本文中查找"));
                    return Task::none();
                }
                self.find_open = true;
                self.settings_open = false;
                let focus = operation::focus(find_input_id());
                if !self.find_input.is_empty() {
                    self.recompute_matches();
                    return Task::batch([focus, self.jump_current_match()]);
                }
                focus
            }
            Message::CloseFind => {
                self.find_open = false;
                self.matches = Arc::new(Vec::new());
                self.current_match = None;
                self.search_rev += 1;
                Task::none()
            }
            Message::FindInput(query) => {
                self.find_input = query;
                self.recompute_matches();
                self.jump_current_match()
            }
            Message::FindNext => {
                if self.matches.is_empty() {
                    return Task::none();
                }
                let len = self.matches.len();
                self.current_match = Some(match self.current_match {
                    Some(index) => (index + 1) % len,
                    None => 0,
                });
                self.search_rev += 1;
                self.jump_current_match()
            }
            Message::FindPrev => {
                if self.matches.is_empty() {
                    return Task::none();
                }
                let len = self.matches.len();
                self.current_match = Some(match self.current_match {
                    Some(0) | None => len - 1,
                    Some(index) => index - 1,
                });
                self.search_rev += 1;
                self.jump_current_match()
            }
            Message::OpenFile => {
                if !self.can_leave_document() {
                    return Task::none();
                }
                self.settings_open = false;
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Markdown", &["md", "markdown", "mdx"])
                    .pick_file()
                {
                    self.pending_anchor = None;
                    return self.open_path(path, None);
                }
                Task::none()
            }
            Message::OpenFolder => {
                if !self.can_leave_document() {
                    return Task::none();
                }
                self.settings_open = false;
                if let Some(root) = rfd::FileDialog::new().pick_folder() {
                    let files = scan_markdown(&root);
                    self.notice = if files.is_empty() {
                        Some(String::from("这个文件夹里没有 Markdown 文件"))
                    } else {
                        None
                    };
                    self.quick = Some(QuickState {
                        root,
                        files: Arc::new(files),
                        query: String::new(),
                    });
                    return operation::focus(quick_input_id());
                }
                Task::none()
            }
            Message::Reload => {
                if !self.can_leave_document() {
                    return Task::none();
                }
                if let Some(path) = self.path.clone() {
                    let ratio = self.scroll_ratio;
                    return self.open_path(path, Some(ratio));
                }
                Task::none()
            }
            Message::Minimize => self.win(|id| iced::window::minimize(id, true)),
            Message::ToggleMaximize => {
                self.maximized = !self.maximized;
                self.win(iced::window::toggle_maximize)
            }
            Message::CloseWindow => self.win(iced::window::close),
            Message::DragWindow => self.win(iced::window::drag),
            Message::Resize(direction) => self.win(|id| iced::window::drag_resize(id, direction)),
            Message::WindowId(id) => {
                if let Some(id) = id {
                    self.window_id = Some(id);
                }
                Task::none()
            }
            Message::WindowOpened(id, size) => {
                self.window_id = Some(id);
                self.window_size = size;
                self.write_probe();
                self.measure()
            }
            Message::WindowResized(size) => {
                if size != self.window_size {
                    self.window_size = size;
                    self.offsets.clear();
                }
                Task::none()
            }
            Message::WindowCloseRequested => {
                if self.editing && self.dirty {
                    self.notice = Some(String::from("有未保存修改，按 Ctrl+S 保存后再关闭"));
                    Task::none()
                } else {
                    self.win(iced::window::close)
                }
            }
            Message::Measure => self.measure(),
            Message::Offsets(map) => {
                self.offsets = map;
                self.refresh_heading(
                    self.viewport
                        .map(|viewport| viewport.absolute_offset().y)
                        .unwrap_or(0.0),
                );
                if let Some(index) = self.pending_jump.take() {
                    if let Some(y) = self.offsets.get(&index).copied() {
                        self.anim_current = y - 8.0;
                        return offsets::scroll_to_y(y - 8.0);
                    }
                }
                Task::none()
            }
            Message::Jump(index) => self.jump_block(index),
            Message::Link(link) => self.follow_link(link),
            Message::Scroll(viewport) => {
                let offset = viewport.absolute_offset().y;
                self.viewport = Some(viewport);
                self.anim_current = offset;
                self.sync_ratio(offset);
                self.refresh_heading(offset);
                if self.offsets.is_empty() {
                    return self.measure();
                }
                Task::none()
            }
            Message::Wheel(dy) => self.scroll_by(dy),
            Message::WheelPixels(dy) => self.scroll_by(dy),
            Message::ProbeFrame => {
                self.write_probe();
                Task::none()
            }
            Message::QuickInput(query) => {
                if let Some(quick) = self.quick.as_mut() {
                    quick.query = query;
                }
                Task::none()
            }
            Message::QuickOpen(path) => {
                if !self.can_leave_document() {
                    return Task::none();
                }
                self.quick = None;
                self.pending_anchor = None;
                self.open_path(path, None)
            }
            Message::QuickOpenFirst => {
                if !self.can_leave_document() {
                    return Task::none();
                }
                if let Some((path, _)) = self.quick_matches().into_iter().next() {
                    self.quick = None;
                    self.pending_anchor = None;
                    return self.open_path(path, None);
                }
                Task::none()
            }
            Message::QuickClose => {
                self.quick = None;
                Task::none()
            }
            Message::Escape => {
                if self.quick.is_some() {
                    self.quick = None;
                } else if self.about_open {
                    self.about_open = false;
                } else if self.settings_open {
                    self.settings_open = false;
                } else if self.find_open {
                    self.find_open = false;
                    self.matches = Arc::new(Vec::new());
                    self.current_match = None;
                    self.search_rev += 1;
                } else if self.editing {
                    if self.dirty {
                        self.notice = Some(String::from("有未保存修改，按 Ctrl+S 保存"));
                    } else {
                        self.editing = false;
                        self.editor = None;
                    }
                }
                Task::none()
            }
            Message::KeyEnter => {
                if self.quick.is_some() {
                    return self.update(Message::QuickOpenFirst);
                }
                if self.find_open {
                    return self.update(Message::FindNext);
                }
                Task::none()
            }
            Message::KeyShiftEnter => {
                if self.find_open {
                    return self.update(Message::FindPrev);
                }
                Task::none()
            }
            Message::KeyF3 => {
                if self.find_open {
                    return self.update(Message::FindNext);
                }
                Task::none()
            }
            Message::KeyShiftF3 => {
                if self.find_open {
                    return self.update(Message::FindPrev);
                }
                Task::none()
            }
            Message::Noop => Task::none(),
        }
    }

    fn win<F>(&self, action: F) -> Task<Message>
    where
        F: FnOnce(iced::window::Id) -> Task<Message>,
    {
        match self.window_id {
            Some(id) => action(id),
            None => Task::none(),
        }
    }

    fn write_probe(&mut self) {
        if let Some(path) = self.probe.take() {
            let _ = std::fs::write(path, self.start.elapsed().as_millis().to_string());
        }
    }

    fn after_metrics_change(&mut self) -> Task<Message> {
        self.offsets.clear();
        self.current_heading = None;
        offsets::snap_to_ratio(self.scroll_ratio)
    }

    fn can_leave_document(&mut self) -> bool {
        if self.editing && self.dirty {
            self.notice = Some(String::from("有未保存修改，按 Ctrl+S 保存后再切换"));
            false
        } else {
            true
        }
    }

    fn begin_edit(&mut self) -> Task<Message> {
        self.edit_ratio = self.scroll_ratio;
        self.editor = Some(text_editor::Content::with_text(&self.doc.source));
        self.editing = true;
        self.dirty = false;
        self.find_open = false;
        self.settings_open = false;
        self.quick = None;
        self.notice = None;
        Task::none()
    }

    fn save_and_finish(&mut self) -> Task<Message> {
        let Some(text) = self.editor.as_ref().map(|editor| editor.text()) else {
            self.editing = false;
            return Task::none();
        };
        let path = match self.path.clone() {
            Some(path) => path,
            None => match rfd::FileDialog::new()
                .add_filter("Markdown", &["md", "markdown", "mdx"])
                .set_file_name("快速入门.md")
                .save_file()
            {
                Some(path) => path,
                None => return Task::none(),
            },
        };
        if let Err(error) = std::fs::write(&path, &text) {
            self.notice = Some(format!("保存失败：{error}"));
            return Task::none();
        }
        let parsed = doc::parse(&text, Some(&path));
        self.doc = Arc::new(parsed);
        self.path = Some(path.clone());
        if self.root.is_none() {
            self.root = path.parent().map(Path::to_path_buf);
        }
        self.doc_rev += 1;
        self.offsets.clear();
        self.current_heading = None;
        self.pending_jump = None;
        self.find_open = false;
        self.find_input.clear();
        self.matches = Arc::new(Vec::new());
        self.current_match = None;
        self.search_rev += 1;
        self.editing = false;
        self.editor = None;
        self.dirty = false;
        self.notice = Some(String::from("已保存"));
        self.scroll_ratio = self.edit_ratio;
        offsets::snap_to_ratio(self.scroll_ratio)
    }

    fn sync_ratio(&mut self, offset: f32) {
        if let Some(viewport) = self.viewport {
            let content = viewport.content_bounds().height;
            let view = viewport.bounds().height;
            self.scroll_ratio = if content > view {
                (offset / (content - view)).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
    }

    fn scroll_by(&mut self, delta: f32) -> Task<Message> {
        let current = self
            .viewport
            .map(|viewport| viewport.absolute_offset().y)
            .unwrap_or(self.anim_current);
        let max = self
            .viewport
            .map(|viewport| (viewport.content_bounds().height - viewport.bounds().height).max(0.0));
        let target = match max {
            Some(max) => (current + delta).clamp(0.0, max),
            None => (current + delta).max(0.0),
        };
        if (target - current).abs() < 0.1 {
            return Task::none();
        }
        self.anim_current = target;
        self.sync_ratio(target);
        offsets::scroll_to_y(target)
    }

    fn refresh_heading(&mut self, offset: f32) {
        let probe = offset + 80.0;
        self.current_heading = self
            .doc
            .headings
            .iter()
            .enumerate()
            .rev()
            .find(|(_, heading)| {
                self.offsets
                    .get(&heading.block)
                    .is_some_and(|y| *y <= probe)
            })
            .map(|(index, _)| index);
    }

    fn recompute_matches(&mut self) {
        self.matches = Arc::new(doc::find_matches(&self.doc, &self.find_input));
        self.current_match = if self.matches.is_empty() {
            None
        } else {
            Some(0)
        };
        self.search_rev += 1;
    }

    fn jump_current_match(&mut self) -> Task<Message> {
        match self.current_match.and_then(|index| self.matches.get(index)) {
            Some(found) => self.jump_block(found.block),
            None => Task::none(),
        }
    }

    fn jump_block(&mut self, index: usize) -> Task<Message> {
        if let Some(y) = self.offsets.get(&index).copied() {
            self.anim_current = y - 8.0;
            offsets::scroll_to_y(y - 8.0)
        } else {
            self.pending_jump = Some(index);
            self.measure()
        }
    }

    fn follow_link(&mut self, link: Link) -> Task<Message> {
        match link {
            Link::External(url) => {
                if std::env::var_os("SUDU_NO_OPEN").is_none() {
                    let _ = open::that(&url);
                }
                self.last_link = Some(url);
                Task::none()
            }
            Link::Internal { path, anchor } => {
                let same = self.path.as_ref().is_some_and(|current| current == &path);
                if same {
                    if let Some(anchor) = anchor {
                        self.jump_anchor(&anchor)
                    } else {
                        self.jump_block(0)
                    }
                } else {
                    self.pending_anchor = anchor;
                    self.open_path(path, None)
                }
            }
        }
    }

    fn jump_anchor(&mut self, anchor: &str) -> Task<Message> {
        let anchor = doc::percent_decode(anchor);
        let block = self
            .doc
            .headings
            .iter()
            .find(|heading| heading.slug == anchor || heading.text == anchor)
            .map(|heading| heading.block);
        match block {
            Some(block) => self.jump_block(block),
            None => Task::none(),
        }
    }

    fn open_path(&mut self, path: PathBuf, ratio: Option<f32>) -> Task<Message> {
        match doc::load(&path) {
            Ok(loaded) => {
                self.doc = Arc::new(loaded);
                if self.root.is_none() {
                    self.root = path.parent().map(Path::to_path_buf);
                }
                self.path = Some(path);
                self.doc_rev += 1;
                self.offsets.clear();
                self.current_heading = None;
                self.pending_jump = None;
                self.find_open = false;
                self.find_input.clear();
                self.matches = Arc::new(Vec::new());
                self.current_match = None;
                self.search_rev += 1;
                self.quick = None;
                self.notice = None;
                self.editing = false;
                self.editor = None;
                self.dirty = false;
                self.anim_current = 0.0;
                self.scroll_ratio = ratio.unwrap_or(0.0);
                let mut task = offsets::snap_to_ratio(self.scroll_ratio);
                if let Some(anchor) = self.pending_anchor.take() {
                    if let Some(block) = self
                        .doc
                        .headings
                        .iter()
                        .find(|heading| heading.slug == anchor || heading.text == anchor)
                        .map(|heading| heading.block)
                    {
                        task = Task::batch([task, self.jump_block(block)]);
                    }
                }
                task
            }
            Err(error) => {
                self.notice = Some(error);
                Task::none()
            }
        }
    }

    fn measure(&self) -> Task<Message> {
        let targets = self
            .doc
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.id.clone(), index))
            .collect();
        offsets::collect(targets)
    }

    fn content_width(&self, max_width: f32) -> f32 {
        let sidebar = if self.sidebar_open && !self.editing {
            SIDEBAR_WIDTH
        } else {
            0.0
        };
        (self.window_size.width - sidebar - 120.0).clamp(MIN_CONTENT, max_width)
    }

    fn body_key(&self, t: &Theme) -> BodyKey {
        let search = if self.find_open && !self.matches.is_empty() {
            Some(Arc::new(SearchView {
                matches: self.matches.clone(),
                current: self.current_match,
            }))
        } else {
            None
        };
        BodyKey {
            data: Arc::new(BodyData {
                doc: self.doc.clone(),
                pal: t.pal,
                body_font: t.body,
                mono_font: t.mono,
                size: t.size * self.font_scale,
                line: t.line,
                search,
                renders: self.renders.clone(),
            }),
            doc_rev: self.doc_rev,
            theme: self.theme_idx as u8,
            size_q: (t.size * self.font_scale * 64.0) as u16,
            line_q: (t.line * 64.0) as u16,
            search_rev: self.search_rev,
        }
    }

    fn quick_matches(&self) -> Vec<(PathBuf, String)> {
        let Some(quick) = &self.quick else {
            return Vec::new();
        };
        let needle = quick.query.to_lowercase();
        let mut out = Vec::new();
        for file in quick.files.iter() {
            let relative = file
                .strip_prefix(&quick.root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            if needle.is_empty() || relative.to_lowercase().contains(&needle) {
                out.push((file.clone(), relative));
                if out.len() >= 8 {
                    break;
                }
            }
        }
        out
    }

    pub fn view(&self) -> Element<'_, Message> {
        let all = theme::all();
        let t = &all[self.theme_idx];
        let pal = t.pal;
        let main = column![
            self.header(t, pal),
            row![self.sidebar(t, pal), self.content(t, pal)].height(Length::Fill),
            self.status(t, pal)
        ]
        .height(Length::Fill);

        let mut layers: Vec<Element<'_, Message>> = vec![main.into()];
        if self.settings_open {
            layers.push(self.settings_overlay(t, pal));
        }
        if self.about_open {
            layers.push(self.about_overlay(t, pal));
        }
        if self.quick.is_some() {
            layers.push(self.quick_overlay(t, pal));
        }
        if !self.maximized {
            layers.extend(self.resize_edges());
        }
        container(stack(layers))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(pal.bg.into()),
                text_color: Some(pal.text),
                ..Default::default()
            })
            .into()
    }

    fn header(&self, _t: &Theme, pal: Palette) -> Element<'_, Message> {
        let sidebar = icon_button(
            icon_sidebar(),
            self.sidebar_open,
            pal,
            Message::ToggleSidebar,
        );
        let title = mouse_area(
            container(
                text(if self.dirty {
                    format!("素读 — {} *", self.source_name())
                } else {
                    format!("素读 — {}", self.source_name())
                })
                .size(12)
                .color(pal.muted),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .align_y(Vertical::Center)
            .padding(Padding {
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
                left: 10.0,
            }),
        )
        .on_press(Message::DragWindow)
        .on_double_click(Message::ToggleMaximize);
        let edit = if self.editing {
            icon_button(icon_save(), true, pal, Message::Save)
        } else {
            icon_button(icon_edit(), false, pal, Message::ToggleEdit)
        };
        let search = icon_button(icon_search(), self.find_open, pal, Message::OpenFind);
        let settings = icon_button(
            icon_settings(),
            self.settings_open,
            pal,
            Message::ToggleSettings,
        );
        let min = window_icon_button(icon_minimize(), false, pal, Message::Minimize);
        let max_icon = if self.maximized {
            icon_restore()
        } else {
            icon_maximize()
        };
        let max = window_icon_button(max_icon, false, pal, Message::ToggleMaximize);
        let close = window_icon_button(icon_close(), true, pal, Message::CloseWindow);
        column![
            container(
                row![sidebar, title, edit, search, settings, min, max, close]
                    .height(Length::Fill)
                    .align_y(Alignment::Center)
            )
            .height(Length::Fixed(HEADER_HEIGHT))
            .width(Length::Fill)
            .style(move |_| container::Style {
                background: Some(pal.panel.into()),
                text_color: Some(pal.text),
                ..Default::default()
            }),
            hline(pal.border, 1.0)
        ]
        .into()
    }

    fn sidebar(&self, _t: &Theme, pal: Palette) -> Element<'_, Message> {
        if !self.sidebar_open || self.editing {
            return Space::new().into();
        }
        let mut list = Column::new().spacing(1).width(Length::Fill);
        if self.doc.headings.is_empty() {
            list = list.push(
                container(text("本文没有标题").size(12).color(pal.muted))
                    .padding(Padding::from([12.0, 12.0])),
            );
        }
        for (index, heading) in self.doc.headings.iter().enumerate() {
            let selected = self.current_heading == Some(index);
            let indent = 10.0 + (heading.level.saturating_sub(1) as f32) * 14.0;
            let label = text(heading.text.clone())
                .size(13)
                .wrapping(Wrapping::Word)
                .color(if selected { pal.accent } else { pal.text });
            let item = button(container(label).width(Length::Fill).padding(Padding {
                top: 5.0,
                right: 10.0,
                bottom: 5.0,
                left: indent,
            }))
            .width(Length::Fill)
            .on_press(Message::Jump(heading.block))
            .style(move |_, status| {
                let hovered = matches!(status, button::Status::Hovered);
                button::Style {
                    background: if selected {
                        Some(pal.selection.into())
                    } else if hovered {
                        Some(pal.code_bg.into())
                    } else {
                        None
                    },
                    text_color: if selected { pal.accent } else { pal.text },
                    border: Border::default(),
                    shadow: Shadow::default(),
                    ..Default::default()
                }
            });
            list = list.push(item);
        }
        let header = row![
            text("大纲").size(12).color(pal.muted),
            Space::new().width(Length::Fill),
            text(format!("{}", self.doc.headings.len()))
                .size(11)
                .color(pal.muted)
        ]
        .padding(Padding::from([10.0, 12.0]))
        .align_y(Alignment::Center);
        let body = container(list).padding(Padding {
            top: 0.0,
            right: 0.0,
            bottom: 16.0,
            left: 0.0,
        });
        let panel = container(column![
            header,
            hline(pal.border, 1.0),
            scrollable(body).height(Length::Fill)
        ])
        .width(Length::Fixed(SIDEBAR_WIDTH - 1.0))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(pal.panel.into()),
            ..Default::default()
        });
        row![panel, vline(pal.border, 1.0)]
            .height(Length::Fill)
            .into()
    }

    fn content(&self, t: &Theme, pal: Palette) -> Element<'_, Message> {
        if self.editing {
            if let Some(editor) = &self.editor {
                let width = self.content_width(t.width);
                let editor = text_editor(editor)
                    .on_action(Message::EditorAction)
                    .font(t.mono)
                    .size(t.size * self.font_scale * 0.95)
                    .line_height(LineHeight::Relative(1.65))
                    .padding(Padding::from([20.0, 16.0]))
                    .height(Length::Fill)
                    .style(move |_, _| text_editor::Style {
                        background: pal.bg.into(),
                        border: Border::default(),
                        placeholder: pal.muted,
                        value: pal.text,
                        selection: pal.selection,
                    });
                let paper = container(editor)
                    .width(Length::Fixed(width))
                    .height(Length::Fill)
                    .style(move |_| container::Style {
                        background: Some(pal.bg.into()),
                        border: Border {
                            color: pal.border,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    });
                return container(paper)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .padding(Padding {
                        top: 16.0,
                        right: 0.0,
                        bottom: 16.0,
                        left: 0.0,
                    })
                    .into();
            }
        }
        let width = self.content_width(t.width);
        let body = iced::widget::lazy(self.body_key(t), |key| render_body(key));
        let paper = container(body)
            .width(Length::Fixed(width))
            .padding(Padding {
                top: 24.0,
                right: 0.0,
                bottom: 40.0,
                left: 0.0,
            });
        let centered = container(paper).width(Length::Fill).center_x(Length::Fill);
        let scroller = crate::smooth::SmoothScroll::new(
            scrollable(centered)
                .id(offsets::body_id())
                .on_scroll(Message::Scroll)
                .height(Length::Fill),
            t.size * self.font_scale * t.line,
        );
        let base: Element<'_, Message> = scroller.into();
        if self.find_open {
            stack(vec![base, self.find_bar(t, pal)]).into()
        } else {
            base
        }
    }

    fn find_bar(&self, _t: &Theme, pal: Palette) -> Element<'_, Message> {
        let count = if self.find_input.is_empty() {
            String::from("输入关键词")
        } else if self.matches.is_empty() {
            String::from("0 / 0")
        } else {
            format!(
                "{} / {}",
                self.current_match.map(|index| index + 1).unwrap_or(0),
                self.matches.len()
            )
        };
        let input = text_input("在本文中查找", &self.find_input)
            .id(find_input_id())
            .on_input(Message::FindInput)
            .on_submit(Message::FindNext)
            .size(13)
            .padding(Padding::from([6.0, 8.0]))
            .width(Length::Fixed(200.0))
            .style(move |_, _| text_input_style(pal));
        let bar = container(
            row![
                input,
                text(count).size(11).color(pal.muted),
                flat_button("↑", false, pal).on_press(Message::FindPrev),
                flat_button("↓", false, pal).on_press(Message::FindNext),
                flat_button("✕", false, pal).on_press(Message::CloseFind),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding(Padding::from([8.0, 10.0]))
        .width(Length::Fixed(380.0))
        .style(move |_| container::Style {
            background: Some(pal.panel.into()),
            border: Border {
                color: pal.border,
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: Shadow::default(),
            ..Default::default()
        });
        container(bar)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Right)
            .align_y(Vertical::Top)
            .padding(Padding {
                top: 12.0,
                right: 16.0,
                bottom: 0.0,
                left: 0.0,
            })
            .into()
    }

    fn settings_overlay(&self, t: &Theme, pal: Palette) -> Element<'_, Message> {
        let scrim = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseSettings);
        let menu = mouse_area(self.settings_menu(t, pal)).on_press(Message::Noop);
        stack(vec![
            scrim.into(),
            container(menu)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Horizontal::Right)
                .align_y(Vertical::Top)
                .padding(Padding {
                    top: HEADER_HEIGHT + 6.0,
                    right: 8.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .into(),
        ])
        .into()
    }

    fn settings_menu(&self, _t: &Theme, pal: Palette) -> Element<'_, Message> {
        let mut menu = Column::new().spacing(1).width(Length::Fill);
        menu = menu.push(section_label("主题", pal));
        for (index, theme) in theme::all().iter().enumerate() {
            let selected = index == self.theme_idx;
            let label = text(format!(
                "{}{}",
                if selected { "● " } else { "   " },
                theme.name
            ))
            .size(13)
            .color(if selected { pal.accent } else { pal.text });
            menu = menu.push(
                button(label)
                    .width(Length::Fill)
                    .padding(Padding::from([6.0, 8.0]))
                    .on_press(Message::SetTheme(index))
                    .style(move |_, status| menu_button_style(status, pal)),
            );
        }
        menu = menu.push(hline(pal.border, 1.0));
        menu = menu.push(menu_row(
            "大纲侧栏",
            "Ctrl+B",
            Some(self.sidebar_open),
            Message::ToggleSidebar,
            pal,
        ));
        menu = menu.push(menu_row(
            "在本文中查找",
            "Ctrl+F",
            Some(self.find_open),
            Message::OpenFind,
            pal,
        ));
        menu = menu.push(menu_row(
            "打开文件…",
            "Ctrl+O",
            None,
            Message::OpenFile,
            pal,
        ));
        menu = menu.push(menu_row(
            "打开文件夹…",
            "Ctrl+Shift+O",
            None,
            Message::OpenFolder,
            pal,
        ));
        menu = menu.push(menu_row("放大字号", "Ctrl+=", None, Message::FontInc, pal));
        menu = menu.push(menu_row("缩小字号", "Ctrl+-", None, Message::FontDec, pal));
        menu = menu.push(menu_row("重读文件", "Ctrl+R", None, Message::Reload, pal));
        menu = menu.push(hline(pal.border, 1.0));
        menu = menu.push(menu_row("关于素读", "", None, Message::ToggleAbout, pal));
        container(menu.padding(Padding::from([6.0, 6.0])))
            .width(Length::Fixed(250.0))
            .style(move |_| container::Style {
                background: Some(pal.panel.into()),
                border: Border {
                    color: pal.border,
                    width: 1.0,
                    radius: 10.0.into(),
                },
                shadow: Shadow::default(),
                ..Default::default()
            })
            .into()
    }

    fn about_overlay(&self, t: &Theme, pal: Palette) -> Element<'_, Message> {
        let scrim = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::CloseAbout);
        let card = container(
            column![
                text("素读").size(24).font(t.body).color(pal.text),
                text("轻量中文 Markdown 阅读器").size(13).color(pal.muted),
                hline(pal.border, 1.0),
                text("v0.1.0 · iced 0.14 · pulldown-cmark · tiny-skia")
                    .size(11)
                    .color(pal.muted),
                text("GNU toolchain build").size(11).color(pal.muted),
                flat_button("关闭", false, pal).on_press(Message::CloseAbout),
            ]
            .spacing(10)
            .padding(Padding::from([20.0, 22.0]))
            .align_x(Alignment::Center),
        )
        .width(Length::Fixed(340.0))
        .style(move |_| container::Style {
            background: Some(pal.panel.into()),
            border: Border {
                color: pal.border,
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: Shadow::default(),
            ..Default::default()
        });
        stack(vec![
            scrim.into(),
            container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Center)
                .into(),
        ])
        .into()
    }

    fn quick_overlay(&self, _t: &Theme, pal: Palette) -> Element<'_, Message> {
        let scrim = mouse_area(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::QuickClose);
        let input = text_input(
            "搜索文件…",
            &self
                .quick
                .as_ref()
                .map(|q| q.query.clone())
                .unwrap_or_default(),
        )
        .id(quick_input_id())
        .on_input(Message::QuickInput)
        .on_submit(Message::QuickOpenFirst)
        .size(13)
        .padding(Padding::from([7.0, 9.0]))
        .width(Length::Fill)
        .style(move |_, _| text_input_style(pal));
        let mut list = Column::new().spacing(1).width(Length::Fill);
        for (path, relative) in self.quick_matches() {
            list = list.push(
                button(text(relative).size(12).color(pal.text))
                    .width(Length::Fill)
                    .padding(Padding::from([6.0, 8.0]))
                    .on_press(Message::QuickOpen(path))
                    .style(move |_, status| menu_button_style(status, pal)),
            );
        }
        if self.quick_matches().is_empty() {
            list = list.push(
                container(text("没有匹配的文件").size(12).color(pal.muted))
                    .padding(Padding::from([10.0, 8.0])),
            );
        }
        let card = container(
            column![
                input,
                hline(pal.border, 1.0),
                scrollable(list).height(Length::Fixed(240.0)),
                text(
                    self.quick
                        .as_ref()
                        .map(|quick| format!("共 {} 个 Markdown 文件", quick.files.len()))
                        .unwrap_or_default()
                )
                .size(11)
                .color(pal.muted),
            ]
            .spacing(8)
            .padding(Padding::from([14.0, 14.0])),
        )
        .width(Length::Fixed(560.0))
        .style(move |_| container::Style {
            background: Some(pal.panel.into()),
            border: Border {
                color: pal.border,
                width: 1.0,
                radius: 10.0.into(),
            },
            shadow: Shadow::default(),
            ..Default::default()
        });
        stack(vec![
            scrim.into(),
            container(card)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Horizontal::Center)
                .align_y(Vertical::Top)
                .padding(Padding {
                    top: 80.0,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .into(),
        ])
        .into()
    }

    fn status(&self, _t: &Theme, pal: Palette) -> Element<'_, Message> {
        let notice = self
            .notice
            .clone()
            .or_else(|| self.last_link.clone())
            .unwrap_or_default();
        let progress = if self.editing {
            String::from("编辑中")
        } else {
            format!("{:.0}%", self.scroll_ratio * 100.0)
        };
        let words = if self.editing {
            String::from("— 字")
        } else {
            format!("{} 字", self.doc.words)
        };
        let lines = if self.editing {
            self.editor
                .as_ref()
                .map(|editor| format!("{} 行", editor.line_count()))
                .unwrap_or_default()
        } else {
            format!("{} 行", self.doc.lines)
        };
        column![
            hline(pal.border, 1.0),
            container(
                row![
                    text(progress).size(11).color(pal.muted),
                    text(words).size(11).color(pal.muted),
                    text(lines).size(11).color(pal.muted),
                    Space::new().width(Length::Fill),
                    text(notice).size(11).color(pal.muted),
                    text(self.source_name()).size(11).color(pal.muted),
                ]
                .spacing(14)
                .align_y(Alignment::Center)
            )
            .height(Length::Fixed(STATUS_HEIGHT))
            .width(Length::Fill)
            .padding(Padding::from([0.0, 14.0]))
            .style(move |_| container::Style {
                background: Some(pal.panel.into()),
                text_color: Some(pal.muted),
                ..Default::default()
            })
        ]
        .into()
    }

    fn resize_edges(&self) -> Vec<Element<'_, Message>> {
        vec![
            resize_strip(
                iced::window::Direction::North,
                Length::Fill,
                Length::Fixed(4.0),
                Horizontal::Center,
                Vertical::Top,
                iced::mouse::Interaction::ResizingVertically,
            ),
            resize_strip(
                iced::window::Direction::South,
                Length::Fill,
                Length::Fixed(4.0),
                Horizontal::Center,
                Vertical::Bottom,
                iced::mouse::Interaction::ResizingVertically,
            ),
            resize_strip(
                iced::window::Direction::West,
                Length::Fixed(4.0),
                Length::Fill,
                Horizontal::Left,
                Vertical::Center,
                iced::mouse::Interaction::ResizingHorizontally,
            ),
            resize_strip(
                iced::window::Direction::East,
                Length::Fixed(4.0),
                Length::Fill,
                Horizontal::Right,
                Vertical::Center,
                iced::mouse::Interaction::ResizingHorizontally,
            ),
            resize_strip(
                iced::window::Direction::NorthWest,
                Length::Fixed(8.0),
                Length::Fixed(8.0),
                Horizontal::Left,
                Vertical::Top,
                iced::mouse::Interaction::ResizingDiagonallyDown,
            ),
            resize_strip(
                iced::window::Direction::NorthEast,
                Length::Fixed(8.0),
                Length::Fixed(8.0),
                Horizontal::Right,
                Vertical::Top,
                iced::mouse::Interaction::ResizingDiagonallyUp,
            ),
            resize_strip(
                iced::window::Direction::SouthWest,
                Length::Fixed(8.0),
                Length::Fixed(8.0),
                Horizontal::Left,
                Vertical::Bottom,
                iced::mouse::Interaction::ResizingDiagonallyUp,
            ),
            resize_strip(
                iced::window::Direction::SouthEast,
                Length::Fixed(8.0),
                Length::Fixed(8.0),
                Horizontal::Right,
                Vertical::Bottom,
                iced::mouse::Interaction::ResizingDiagonallyDown,
            ),
        ]
    }
}

fn window_event((id, event): (iced::window::Id, iced::window::Event)) -> Option<Message> {
    match event {
        iced::window::Event::Opened { size, .. } => Some(Message::WindowOpened(id, size)),
        iced::window::Event::Resized(size) => Some(Message::WindowResized(size)),
        iced::window::Event::CloseRequested => Some(Message::WindowCloseRequested),
        _ => None,
    }
}

fn window_event_probe((id, event): (iced::window::Id, iced::window::Event)) -> Option<Message> {
    match event {
        iced::window::Event::RedrawRequested(_) | iced::window::Event::Focused => {
            Some(Message::ProbeFrame)
        }
        other => window_event((id, other)),
    }
}

fn key_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    let iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) = event
    else {
        return None;
    };
    let command = modifiers.command();
    let shift = modifiers.shift();
    match key {
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) => Some(Message::Escape),
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) if shift => {
            Some(Message::KeyShiftEnter)
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Enter) => Some(Message::KeyEnter),
        iced::keyboard::Key::Named(iced::keyboard::key::Named::F3) if shift => {
            Some(Message::KeyShiftF3)
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::F3) => Some(Message::KeyF3),
        iced::keyboard::Key::Character(c) if command => match c.as_str() {
            "b" => Some(Message::ToggleSidebar),
            "f" => Some(Message::OpenFind),
            "e" => Some(Message::ToggleEdit),
            "s" => Some(Message::Save),
            "o" if shift => Some(Message::OpenFolder),
            "o" => Some(Message::OpenFile),
            "p" => Some(Message::OpenFolder),
            "t" => Some(Message::NextTheme),
            "r" => Some(Message::Reload),
            "=" | "+" => Some(Message::FontInc),
            "-" => Some(Message::FontDec),
            _ => None,
        },
        _ => None,
    }
}

fn find_input_id() -> iced::widget::Id {
    iced::widget::Id::new("find-input")
}

fn quick_input_id() -> iced::widget::Id {
    iced::widget::Id::new("quick-input")
}

fn scan_markdown(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if out.len() >= 3000 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if name.starts_with('.')
                    || matches!(name, "node_modules" | "target" | "dist" | "build")
                {
                    continue;
                }
                walk(&path, out);
            } else if file_type.is_file() {
                if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
                    if matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "md" | "markdown" | "mdx"
                    ) {
                        out.push(path);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort_by(|left, right| {
        left.to_string_lossy()
            .to_lowercase()
            .cmp(&right.to_string_lossy().to_lowercase())
    });
    out
}

fn flat_button(label: &str, active: bool, pal: Palette) -> Button<'static, Message> {
    button(text(label.to_string()).size(12))
        .padding(Padding::from([5.0, 8.0]))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: if active {
                    Some(pal.selection.into())
                } else if hovered {
                    Some(pal.code_bg.into())
                } else {
                    None
                },
                text_color: if active { pal.accent } else { pal.muted },
                border: Border::default(),
                shadow: Shadow::default(),
                ..Default::default()
            }
        })
}

fn icon_search() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/search.svg")[..])
}

fn icon_settings() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/settings.svg")[..])
}

fn icon_sidebar() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/sidebar.svg")[..])
}

fn icon_edit() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/edit.svg")[..])
}

fn icon_save() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/save.svg")[..])
}

fn icon_minimize() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/minimize.svg")[..])
}

fn icon_maximize() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/maximize.svg")[..])
}

fn icon_restore() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/restore.svg")[..])
}

fn icon_close() -> svg::Handle {
    svg::Handle::from_memory(&include_bytes!("../assets/icons/close.svg")[..])
}

fn icon_button(
    handle: svg::Handle,
    active: bool,
    pal: Palette,
    message: Message,
) -> Button<'static, Message> {
    let icon = svg(handle)
        .width(Length::Fixed(17.0))
        .height(Length::Fixed(17.0))
        .style(move |_, status| svg::Style {
            color: Some(if active {
                pal.accent
            } else if matches!(status, svg::Status::Hovered) {
                pal.accent
            } else {
                pal.text
            }),
        });
    let content = container(icon)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    let button = button(content)
        .width(Length::Fixed(34.0))
        .height(Length::Fixed(HEADER_HEIGHT))
        .padding(Padding::from(0.0))
        .on_press(message)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: if active {
                    Some(pal.selection.into())
                } else if hovered {
                    Some(pal.code_bg.into())
                } else {
                    None
                },
                text_color: pal.text,
                border: Border::default(),
                shadow: Shadow::default(),
                ..Default::default()
            }
        });
    button
}

fn window_icon_button(
    handle: svg::Handle,
    close: bool,
    pal: Palette,
    message: Message,
) -> Button<'static, Message> {
    let icon = svg(handle)
        .width(Length::Fixed(17.0))
        .height(Length::Fixed(17.0))
        .style(move |_, status| svg::Style {
            color: Some(if matches!(status, svg::Status::Hovered) {
                if close {
                    Color::WHITE
                } else {
                    pal.accent
                }
            } else {
                pal.text
            }),
        });
    let content = container(icon)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);
    let button = button(content)
        .width(Length::Fixed(42.0))
        .height(Length::Fixed(HEADER_HEIGHT))
        .padding(Padding::from(0.0))
        .on_press(message)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let background = if hovered {
                if close {
                    Color::from_rgb8(0xE8, 0x11, 0x23)
                } else {
                    pal.code_bg
                }
            } else {
                pal.panel
            };
            button::Style {
                background: Some(background.into()),
                text_color: if hovered && close {
                    Color::WHITE
                } else {
                    pal.muted
                },
                border: Border::default(),
                shadow: Shadow::default(),
                ..Default::default()
            }
        });
    button
}

fn menu_row(
    label: &str,
    shortcut: &str,
    checked: Option<bool>,
    message: Message,
    pal: Palette,
) -> Button<'static, Message> {
    let row = row![
        text(label.to_string()).size(13).color(pal.text),
        Space::new().width(Length::Fill),
        text(if checked == Some(true) { "✓" } else { "" })
            .size(12)
            .color(pal.accent),
        text(shortcut.to_string()).size(11).color(pal.muted),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    button(row)
        .width(Length::Fill)
        .padding(Padding::from([7.0, 8.0]))
        .on_press(message)
        .style(move |_, status| menu_button_style(status, pal))
}

fn menu_button_style(status: button::Status, pal: Palette) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: if hovered {
            Some(pal.code_bg.into())
        } else {
            None
        },
        text_color: pal.text,
        border: Border::default(),
        shadow: Shadow::default(),
        ..Default::default()
    }
}

fn section_label(label: &str, pal: Palette) -> Element<'static, Message> {
    container(text(label.to_string()).size(11).color(pal.muted))
        .padding(Padding {
            top: 8.0,
            right: 8.0,
            bottom: 4.0,
            left: 8.0,
        })
        .into()
}

fn text_input_style(pal: Palette) -> text_input::Style {
    text_input::Style {
        background: pal.code_bg.into(),
        border: Border {
            color: pal.border,
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: pal.muted,
        placeholder: pal.muted,
        value: pal.text,
        selection: pal.selection,
    }
}

fn hline(color: Color, height: f32) -> Element<'static, Message> {
    container(Space::new().height(Length::Fixed(height)))
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: Some(color.into()),
            ..Default::default()
        })
        .into()
}

fn vline(color: Color, width: f32) -> Element<'static, Message> {
    container(Space::new().width(Length::Fixed(width)))
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(color.into()),
            ..Default::default()
        })
        .into()
}

fn resize_strip(
    direction: iced::window::Direction,
    width: Length,
    height: Length,
    horizontal: Horizontal,
    vertical: Vertical,
    interaction: iced::mouse::Interaction,
) -> Element<'static, Message> {
    let strip = mouse_area(
        container(Space::new().width(width).height(height))
            .width(width)
            .height(height),
    )
    .on_press(Message::Resize(direction))
    .interaction(interaction);
    container(strip)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(horizontal)
        .align_y(vertical)
        .into()
}

fn render_body(key: &BodyKey) -> Element<'static, Message> {
    let started = std::time::Instant::now();
    key.data.renders.fetch_add(1, Ordering::Relaxed);
    let mut col = Column::new().spacing(8).width(Length::Fill);
    for (index, block) in key.data.doc.blocks.iter().enumerate() {
        col = col.push(block_view(key, index, block));
    }
    let element: Element<'static, Message> = col.into();
    if let Some(path) = std::env::var_os("SUDU_TIMING") {
        let _ = std::fs::write(path, format!("render_ms={}", started.elapsed().as_millis()));
    }
    element
}

fn block_view(key: &BodyKey, index: usize, block: &Block) -> Element<'static, Message> {
    let pal = key.data.pal;
    let line_height = match block.kind {
        BlockKind::Code => 1.55,
        BlockKind::Heading(_) => 1.35,
        _ => key.data.line,
    };
    if matches!(block.kind, BlockKind::Rule) {
        let line_color = pal.border;
        return container(
            container(Space::new().height(Length::Fixed(1.0)))
                .width(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(line_color.into()),
                    ..Default::default()
                }),
        )
        .id(block.id.clone())
        .width(Length::Fill)
        .padding(Padding {
            top: 10.0,
            right: 0.0,
            bottom: 10.0,
            left: 0.0,
        })
        .into();
    }
    let size = block_size(key, &block.kind);
    let spans = block_spans(key, index, block, size);
    let selection = pal.selection;
    let rich = iced_selection::rich_text(spans)
        .on_link_click(|message| message)
        .size(size)
        .line_height(LineHeight::Relative(line_height))
        .style(move |_| iced_selection::text::Style {
            color: None,
            selection,
        });
    let rich: Element<'static, Message> = rich.into();
    let pad = match block.kind {
        BlockKind::Heading(1) => Padding {
            top: 14.0,
            right: 0.0,
            bottom: 8.0,
            left: 0.0,
        },
        BlockKind::Heading(2) => Padding {
            top: 12.0,
            right: 0.0,
            bottom: 4.0,
            left: 0.0,
        },
        BlockKind::Heading(3) => Padding {
            top: 10.0,
            right: 0.0,
            bottom: 2.0,
            left: 0.0,
        },
        BlockKind::Heading(_) => Padding {
            top: 8.0,
            right: 0.0,
            bottom: 2.0,
            left: 0.0,
        },
        BlockKind::Code => Padding {
            top: 10.0,
            right: 12.0,
            bottom: 10.0,
            left: 12.0,
        },
        BlockKind::Quote => Padding {
            top: 4.0,
            right: 12.0,
            bottom: 4.0,
            left: 12.0,
        },
        BlockKind::ListItem { indent, .. } => Padding {
            top: 2.0,
            right: 0.0,
            bottom: 2.0,
            left: 8.0 + indent as f32 * 18.0,
        },
        BlockKind::TableRow { .. } => Padding {
            top: 3.0,
            right: 8.0,
            bottom: 3.0,
            left: 8.0,
        },
        _ => Padding::from(0.0),
    };
    let is_code = matches!(block.kind, BlockKind::Code);
    let is_header = matches!(block.kind, BlockKind::TableRow { header: true });
    let bg = if is_code || is_header {
        Some(pal.code_bg.into())
    } else {
        None
    };
    let border = if is_code {
        Border {
            color: pal.border,
            width: 1.0,
            radius: 4.0.into(),
        }
    } else {
        Border::default()
    };
    container(rich)
        .id(block.id.clone())
        .padding(pad)
        .width(Length::Fill)
        .style(move |_| container::Style {
            background: bg,
            border,
            ..Default::default()
        })
        .into()
}

fn block_size(key: &BodyKey, kind: &BlockKind) -> f32 {
    match kind {
        BlockKind::Heading(1) => key.data.size * 1.7,
        BlockKind::Heading(2) => key.data.size * 1.35,
        BlockKind::Heading(3) => key.data.size * 1.15,
        BlockKind::Heading(4) => key.data.size * 1.05,
        BlockKind::Heading(_) => key.data.size,
        BlockKind::Code => key.data.size * 0.9,
        _ => key.data.size,
    }
}

fn block_spans(
    key: &BodyKey,
    index: usize,
    block: &Block,
    size: f32,
) -> Vec<iced::advanced::text::Span<'static, Message, Font>> {
    let pal = key.data.pal;
    let heading = matches!(block.kind, BlockKind::Heading(_));
    let quote = matches!(block.kind, BlockKind::Quote);
    let header = matches!(block.kind, BlockKind::TableRow { header: true });
    let code_block = matches!(block.kind, BlockKind::Code);
    let mut spans = Vec::new();
    if let BlockKind::ListItem { marker, .. } = &block.kind {
        spans.push(
            span(marker.clone())
                .size(size)
                .font(key.data.body_font)
                .color(pal.muted),
        );
    }
    let search = key.data.search.as_ref();
    let mut offset = 0usize;
    for run in &block.runs {
        let run_start = offset;
        let run_end = offset + run.text.len();
        let mut cuts = vec![run_start];
        let mut matches_here: Vec<(usize, &TextMatch)> = Vec::new();
        if let Some(view) = search {
            for (match_index, found) in view.matches.iter().enumerate() {
                if found.block == index && found.start < run_end && found.end > run_start {
                    cuts.push(found.start.max(run_start));
                    cuts.push(found.end.min(run_end));
                    matches_here.push((match_index, found));
                }
            }
        }
        cuts.push(run_end);
        cuts.sort_unstable();
        cuts.dedup();
        for pair in cuts.windows(2) {
            let start = pair[0];
            let end = pair[1];
            if start >= end {
                continue;
            }
            let text = &run.text[start - run_start..end - run_start];
            if text.is_empty() {
                continue;
            }
            let mut font = if run.code {
                key.data.mono_font
            } else {
                key.data.body_font
            };
            if heading || run.bold || header {
                font.weight = iced::font::Weight::Bold;
            } else if !run.code {
                font.weight = iced::font::Weight::Medium;
            }
            if run.italic {
                font.style = iced::font::Style::Italic;
            }
            let color = if run.link.is_some() {
                pal.accent
            } else if quote {
                pal.muted
            } else {
                pal.text
            };
            let mut piece = span(text.to_string()).size(size).font(font).color(color);
            if run.strike {
                piece = piece.strikethrough(true);
            }
            if run.code && !code_block {
                piece = piece.background(pal.code_bg).border(Border {
                    color: pal.border,
                    width: 1.0,
                    radius: 3.0.into(),
                });
            }
            if let Some((match_index, _)) = matches_here
                .iter()
                .find(|(_, found)| found.start <= start && found.end >= end)
            {
                let current = search.and_then(|view| view.current) == Some(*match_index);
                piece = piece.background(if current {
                    pal.highlight_current
                } else {
                    pal.highlight
                });
            }
            if let Some(link) = &run.link {
                piece = piece.link(Message::Link(link.clone())).underline(true);
            }
            spans.push(piece);
        }
        offset = run_end;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_width_stays_positive() {
        let mut app = App::new(None, Instant::now(), None);
        app.window_size = Size::new(720.0, 800.0);
        app.sidebar_open = true;
        assert!(app.content_width(760.0) >= MIN_CONTENT);
    }
}
