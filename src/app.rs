use crate::doc::{self, BlockKind, Doc, Link, TextMatch};
use crate::doc_widget::{DocAction, DocWidget, SearchView};
use crate::editor::{clamp_pos, DocPos, EditorState, FormatKind, TableOp};
use crate::images::ImageStore;
use crate::offsets;
use crate::session::{self, Session};
use crate::table::CellAlign;
use crate::theme::{self, Palette, Theme};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::button::Button;
use iced::widget::text::Wrapping;
use iced::widget::{
    button, column, container, mouse_area, operation, row, scrollable, stack, svg, text,
    text_input, Column, Row, Space,
};
use iced::{Alignment, Border, Color, Element, Length, Padding, Point, Shadow, Size, Task};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::Instant;

const SIDEBAR_WIDTH: f32 = 260.0;
const HEADER_HEIGHT: f32 = 40.0;
const STATUS_HEIGHT: f32 = 26.0;
const MIN_CONTENT: f32 = 420.0;

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
    WindowMoved(Point),
    WindowResized(Size),
    WindowCloseRequested(iced::window::Id),
    Measure,
    Offsets {
        map: HashMap<usize, f32>,
        target_y: Option<f32>,
        visible_block: Option<usize>,
    },
    Jump(usize),
    Scroll(scrollable::Viewport),
    Wheel(f32),
    WheelPixels(f32),
    PageUp,
    PageDown,
    ScrollHome,
    ScrollEnd,
    LineUp,
    LineDown,
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
    ShiftChanged(bool),
    Doc(DocAction),
    Noop,
}

pub struct App {
    doc: Arc<Doc>,
    doc_rev: u64,
    edited_block: Option<usize>,
    materialize_hint: Option<usize>,
    images: Arc<ImageStore>,
    path: Option<PathBuf>,
    root: Option<PathBuf>,
    session: Session,
    theme_idx: usize,
    font_scale: f32,
    sidebar_open: bool,
    settings_open: bool,
    editing: bool,
    editor: EditorState,
    pending_edit_focus: bool,
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
    window_pos: Option<Point>,
    maximized: bool,
    shift_held: bool,
    start: Instant,
    probe: Option<PathBuf>,
    renders: Arc<AtomicU32>,
}

impl App {
    pub fn new(
        initial: Option<PathBuf>,
        restored: Session,
        start: Instant,
        probe: Option<PathBuf>,
    ) -> (Self, Task<Message>) {
        let mut notice = None;
        let mut initial_ratio = 0.0f32;
        let (text_doc, path) = match initial.as_deref() {
            Some(path) => match doc::load(path) {
                Ok(loaded) => {
                    initial_ratio = restored.ratio_for(path).unwrap_or(0.0);
                    (loaded, Some(path.to_path_buf()))
                }
                Err(error) => {
                    notice = Some(error);
                    (doc::quickstart(), None)
                }
            },
            None => {
                let candidate = if std::env::var_os("SUDU_NO_SESSION").is_none() {
                    restored
                        .last_path
                        .as_deref()
                        .map(PathBuf::from)
                        .filter(|p| p.exists())
                } else {
                    None
                };
                if let Some(last) = candidate {
                    match doc::load(&last) {
                        Ok(loaded) => {
                            initial_ratio = restored
                                .ratio_for(&last)
                                .unwrap_or(restored.scroll_ratio);
                            (loaded, Some(last))
                        }
                        Err(error) => {
                            notice = Some(error);
                            (doc::quickstart(), None)
                        }
                    }
                } else {
                    (doc::quickstart(), None)
                }
            }
        };

        let env_theme = std::env::var("SUDU_THEME")
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        let theme_idx = env_theme.unwrap_or(restored.theme_idx) % 4;
        let font_scale = restored.font_scale.clamp(0.8, 1.6);
        let sidebar_open = restored.sidebar_open;
        let window_size = Size::new(
            restored.window_size[0].clamp(720.0, 3840.0),
            restored.window_size[1].clamp(480.0, 2160.0),
        );
        let window_pos = restored.window_pos.map(|[x, y]| Point::new(x, y));
        let maximized = restored.maximized;

        let mut editor = EditorState::new();
        editor.reset(&text_doc);

        let app = Self {
            doc: Arc::new(text_doc),
            doc_rev: 1,
            edited_block: None,
            materialize_hint: None,
            images: Arc::new(ImageStore::new()),
            path,
            root: None,
            session: restored,
            theme_idx,
            font_scale,
            sidebar_open,
            settings_open: false,
            editing: false,
            editor,
            pending_edit_focus: false,
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
            scroll_ratio: initial_ratio.clamp(0.0, 1.0),
            anim_current: 0.0,
            notice,
            last_link: None,
            quick: None,
            window_id: None,
            window_size,
            window_pos,
            maximized,
            shift_held: false,
            start,
            probe,
            renders: Arc::new(AtomicU32::new(0)),
        };

        let task = if initial_ratio > 0.001 {
            offsets::snap_to_ratio(initial_ratio)
        } else {
            Task::none()
        };
        (app, task)
    }

    pub fn enter_edit_on_start(&mut self) {
        self.editing = true;
        self.editor.clamp(&self.doc);
    }

    pub fn title(&self) -> String {
        if self.editor.dirty {
            format!("素读 — {} *", self.source_name())
        } else {
            format!("素读 — {}", self.source_name())
        }
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
                self.persist_session();
                self.measure()
            }
            Message::SetTheme(index) => {
                if index < 4 {
                    self.theme_idx = index;
                }
                self.persist_session();
                self.measure()
            }
            Message::FontInc => {
                self.font_scale = (self.font_scale + 0.05).min(1.6);
                self.persist_session();
                self.after_metrics_change()
            }
            Message::FontDec => {
                self.font_scale = (self.font_scale - 0.05).max(0.8);
                self.persist_session();
                self.after_metrics_change()
            }
            Message::ToggleSidebar => {
                self.sidebar_open = !self.sidebar_open;
                self.offsets.clear();
                self.current_heading = None;
                self.persist_session();
                self.measure()
            }
            Message::ToggleSettings => {
                self.settings_open = !self.settings_open;
                self.about_open = false;
                self.quick = None;
                Task::none()
            }
            Message::ToggleEdit => {
                if self.editing {
                    if self.editor.dirty {
                        self.save_doc(true)
                    } else {
                        self.editing = false;
                        self.notice = None;
                        Task::none()
                    }
                } else {
                    self.begin_edit()
                }
            }
            Message::Save => self.save_doc(false),
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
                    let ratio = self.session.ratio_for(&path);
                    return self.open_path(path, ratio);
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
                self.images.clear();
                if let Some(path) = self.path.clone() {
                    let ratio = self.scroll_ratio;
                    return self.open_path(path, Some(ratio));
                }
                Task::none()
            }
            Message::Minimize => self.win(|id| iced::window::minimize(id, true)),
            Message::ToggleMaximize => {
                self.maximized = !self.maximized;
                self.persist_session();
                self.win(iced::window::toggle_maximize)
            }
            Message::CloseWindow => {
                if self.editor.dirty {
                    self.notice = Some(String::from("有未保存修改，按 Ctrl+S 保存后再关闭"));
                    Task::none()
                } else {
                    self.persist_session();
                    self.win(iced::window::close)
                }
            }
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
            Message::WindowMoved(pos) => {
                if !self.maximized {
                    self.window_pos = Some(pos);
                }
                Task::none()
            }
            Message::WindowResized(size) => {
                if size != self.window_size {
                    self.window_size = size;
                    self.offsets.clear();
                }
                Task::none()
            }
            Message::WindowCloseRequested(id) => {
                if self.editor.dirty {
                    self.notice = Some(String::from("有未保存修改，按 Ctrl+S 保存后再关闭"));
                    Task::none()
                } else {
                    self.persist_session();
                    iced::window::close(id)
                }
            }
            Message::Measure => self.measure(),
            Message::Offsets {
                map,
                target_y,
                visible_block,
            } => {
                self.offsets = map;
                self.refresh_heading(
                    self.viewport
                        .map(|viewport| viewport.absolute_offset().y)
                        .unwrap_or(0.0),
                );
                if self.pending_edit_focus {
                    self.pending_edit_focus = false;
                    if let Some(vb) = visible_block {
                        let clamped = clamp_pos(&self.doc, DocPos::new(vb, 0, 0));
                        self.editor.caret = clamped;
                        self.editor.anchor = None;
                    }
                }
                if let Some(index) = self.pending_jump.take() {
                    self.materialize_hint = None;
                    if let Some(y) = target_y.or_else(|| self.offsets.get(&index).copied()) {
                        let dest = (y - 8.0).max(0.0);
                        self.anim_current = dest;
                        return offsets::scroll_to_y(dest);
                    }
                }
                Task::none()
            }
            Message::Jump(index) => self.jump_block(index),
            Message::Scroll(viewport) => {
                let offset = viewport.absolute_offset().y;
                self.viewport = Some(viewport);
                self.anim_current = offset;
                self.sync_ratio(offset);
                self.refresh_heading(offset);
                if self.offsets.is_empty() && !self.doc.headings.is_empty() {
                    return self.measure();
                }
                Task::none()
            }
            Message::Wheel(dy) => self.scroll_by(dy),
            Message::WheelPixels(dy) => self.scroll_by(dy),
            Message::PageUp => {
                if self.editing || self.modal_open() {
                    return Task::none();
                }
                let step = self
                    .viewport
                    .map(|v| (v.bounds().height * 0.88).max(160.0))
                    .unwrap_or(560.0);
                self.scroll_by(-step)
            }
            Message::PageDown => {
                if self.editing || self.modal_open() {
                    return Task::none();
                }
                let step = self
                    .viewport
                    .map(|v| (v.bounds().height * 0.88).max(160.0))
                    .unwrap_or(560.0);
                self.scroll_by(step)
            }
            Message::ScrollHome => {
                if self.editing || self.modal_open() {
                    return Task::none();
                }
                self.anim_current = 0.0;
                self.sync_ratio(0.0);
                offsets::scroll_to_y(0.0)
            }
            Message::ScrollEnd => {
                if self.editing || self.modal_open() {
                    return Task::none();
                }
                let max = self
                    .viewport
                    .map(|v| (v.content_bounds().height - v.bounds().height).max(0.0))
                    .unwrap_or(0.0);
                self.anim_current = max;
                self.sync_ratio(max);
                offsets::scroll_to_y(max)
            }
            Message::LineUp => {
                if self.editing || self.modal_open() {
                    return Task::none();
                }
                self.scroll_by(-84.0)
            }
            Message::LineDown => {
                if self.editing || self.modal_open() {
                    return Task::none();
                }
                self.scroll_by(84.0)
            }
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
                let ratio = self.session.ratio_for(&path);
                self.open_path(path, ratio)
            }
            Message::QuickOpenFirst => {
                if !self.can_leave_document() {
                    return Task::none();
                }
                if let Some((path, _)) = self.quick_matches().into_iter().next() {
                    self.quick = None;
                    self.pending_anchor = None;
                    let ratio = self.session.ratio_for(&path);
                    return self.open_path(path, ratio);
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
                    if self.editor.dirty {
                        self.notice = Some(String::from("有未保存修改，按 Ctrl+S 保存"));
                    } else {
                        self.editing = false;
                        self.notice = None;
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
            Message::ShiftChanged(held) => {
                self.shift_held = held;
                Task::none()
            }
            Message::Doc(action) => self.handle_doc_action(action),
            Message::Noop => Task::none(),
        }
    }

    fn handle_doc_action(&mut self, action: DocAction) -> Task<Message> {
        match action {
            DocAction::SetSelection { caret, anchor } => {
                self.editor.caret = clamp_pos(&self.doc, caret);
                self.editor.anchor = anchor.map(|a| clamp_pos(&self.doc, a));
                Task::none()
            }
            DocAction::MoveWord { left, extend } => {
                if left {
                    self.editor.move_word_left(&self.doc, extend);
                } else {
                    self.editor.move_word_right(&self.doc, extend);
                }
                Task::none()
            }
            DocAction::MoveDocEdge { end, extend } => {
                if end {
                    self.editor.move_doc_end(&self.doc, extend);
                } else {
                    self.editor.move_doc_start(&self.doc, extend);
                }
                Task::none()
            }
            DocAction::SelectAll => {
                self.editor.select_all(&self.doc);
                Task::none()
            }
            DocAction::FollowLink(link) => self.follow_link(link),
            DocAction::ScrollToY(y) => offsets::scroll_to_y(y),
            DocAction::AdjustScrollY(dy) => {
                let current = self
                    .viewport
                    .map(|v| v.absolute_offset().y)
                    .unwrap_or(self.anim_current);
                let target = (current + dy).max(0.0);
                self.anim_current = target;
                offsets::scroll_to_y(target)
            }
            mutating => {
                let prev_len = self.doc.blocks.len();
                let prev_block = self.editor.caret.block;
                let doc_path = self.path.clone();
                let changed = {
                    let doc_mut = Arc::make_mut(&mut self.doc);
                    match mutating {
                        DocAction::InsertText(text) => {
                            self.editor.insert_text(doc_mut, &text, doc_path.as_deref())
                        }
                        DocAction::Enter { shift } => {
                            self.editor.enter(doc_mut, shift, doc_path.as_deref())
                        }
                        DocAction::Tab { shift } => self.editor.tab(doc_mut, shift),
                        DocAction::Backspace => self.editor.backspace(doc_mut),
                        DocAction::Delete => self.editor.delete_forward(doc_mut),
                        DocAction::DeleteSelection => self.editor.delete_selection(doc_mut),
                        DocAction::ToggleFormat(fmt) => self.editor.toggle_format(doc_mut, fmt),
                        DocAction::InsertTable => self.editor.insert_table(doc_mut, 3, 3),
                        DocAction::TableOp(op) => self.editor.table_op(doc_mut, op),
                        DocAction::ToggleCheckbox(block_idx) => {
                            let ok = self.editor.toggle_task_checkbox(doc_mut, block_idx);
                            if ok && !self.editing {
                                self.notice = Some(String::from("有勾选改动，按 Ctrl+S 保存"));
                            }
                            ok
                        }
                        DocAction::Undo => self.editor.undo(doc_mut),
                        DocAction::Redo => self.editor.redo(doc_mut),
                        _ => false,
                    }
                };
                if changed {
                    let new_len = self.doc.blocks.len();
                    let new_block = self.editor.caret.block;
                    self.edited_block = if prev_len == new_len && prev_block == new_block {
                        Some(new_block)
                    } else {
                        None
                    };
                    self.doc_rev += 1;
                    self.offsets.clear();
                    if self.find_open && !self.find_input.is_empty() {
                        self.recompute_matches();
                    }
                }
                Task::none()
            }
        }
    }

    fn modal_open(&self) -> bool {
        self.quick.is_some() || self.about_open || self.settings_open
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

    fn persist_session(&mut self) {
        if std::env::var_os("SUDU_NO_SESSION").is_some() {
            return;
        }
        self.session.last_path = self.path.as_ref().map(|p| p.to_string_lossy().to_string());
        self.session.scroll_ratio = self.scroll_ratio;
        if let Some(path) = self.path.clone() {
            self.session.record_ratio(&path, self.scroll_ratio);
        }
        self.session.theme_idx = self.theme_idx;
        self.session.font_scale = self.font_scale;
        self.session.sidebar_open = self.sidebar_open;
        self.session.window_size = [self.window_size.width, self.window_size.height];
        if !self.maximized {
            if let Some(pos) = self.window_pos {
                self.session.window_pos = Some([pos.x, pos.y]);
            }
        }
        self.session.maximized = self.maximized;
        session::save(&self.session);
    }

    fn after_metrics_change(&mut self) -> Task<Message> {
        self.offsets.clear();
        self.current_heading = None;
        offsets::snap_to_ratio(self.scroll_ratio)
    }

    fn can_leave_document(&mut self) -> bool {
        if self.editor.dirty {
            self.notice = Some(String::from("有未保存修改，按 Ctrl+S 保存后再切换"));
            false
        } else {
            true
        }
    }

    fn begin_edit(&mut self) -> Task<Message> {
        self.editing = true;
        self.settings_open = false;
        self.quick = None;
        self.notice = None;
        self.editor.clamp(&self.doc);
        self.pending_edit_focus = true;
        self.measure()
    }

    fn save_doc(&mut self, exit_after: bool) -> Task<Message> {
        if !self.editing && !self.editor.dirty {
            return Task::none();
        }
        let source = doc::serialize(&self.doc);
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
        if let Err(error) = std::fs::write(&path, &source) {
            self.notice = Some(format!("保存失败：{error}"));
            return Task::none();
        }
        let mut saved_doc = doc::parse(&source, Some(&path));
        doc::rebuild_metadata(&mut saved_doc);
        self.doc = Arc::new(saved_doc);
        self.path = Some(path.clone());
        if self.root.is_none() {
            self.root = path.parent().map(Path::to_path_buf);
        }
        self.edited_block = None;
        self.doc_rev += 1;
        self.offsets.clear();
        self.editor.dirty = false;
        self.editor.clamp(&self.doc);
        self.notice = Some(String::from("已保存"));
        if exit_after {
            self.editing = false;
        }
        self.persist_session();
        self.measure()
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
        self.pending_jump = Some(index);
        self.materialize_hint = Some(index);
        self.measure_target(Some(index))
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
            Link::Internal { path, anchor, .. } => {
                let same = self.path.as_ref().is_some_and(|current| current == &path);
                if same {
                    if let Some(anchor) = anchor {
                        self.jump_anchor(&anchor)
                    } else {
                        self.jump_block(0)
                    }
                } else {
                    self.pending_anchor = anchor;
                    let ratio = self.session.ratio_for(&path);
                    self.open_path(path, ratio)
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
        self.persist_session();
        match doc::load(&path) {
            Ok(loaded) => {
                self.doc = Arc::new(loaded);
                if self.root.is_none() {
                    self.root = path.parent().map(Path::to_path_buf);
                }
                self.path = Some(path);
                self.edited_block = None;
                self.materialize_hint = None;
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
                self.editor.reset(&self.doc);
                self.anim_current = 0.0;
                self.scroll_ratio = ratio.unwrap_or(0.0).clamp(0.0, 1.0);
                let mut tasks = vec![offsets::snap_to_ratio(self.scroll_ratio), self.measure()];
                if let Some(anchor) = self.pending_anchor.take() {
                    if let Some(block) = self
                        .doc
                        .headings
                        .iter()
                        .find(|heading| heading.slug == anchor || heading.text == anchor)
                        .map(|heading| heading.block)
                    {
                        tasks.push(self.jump_block(block));
                    }
                }
                self.persist_session();
                Task::batch(tasks)
            }
            Err(error) => {
                self.notice = Some(error);
                Task::none()
            }
        }
    }

    fn measure(&self) -> Task<Message> {
        self.measure_target(self.pending_jump)
    }

    fn measure_target(&self, target: Option<usize>) -> Task<Message> {
        let headings = self.doc.headings.iter().map(|h| h.block).collect();
        offsets::collect(headings, target)
    }

    fn content_width(&self, max_width: f32) -> f32 {
        let sidebar = if self.sidebar_open {
            SIDEBAR_WIDTH
        } else {
            0.0
        };
        (self.window_size.width - sidebar - 120.0).clamp(MIN_CONTENT, max_width)
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
        let mut main_col = vec![self.header(t, pal)];
        if self.editing {
            main_col.push(self.edit_toolbar(pal));
        }
        main_col.push(row![self.sidebar(t, pal), self.content(t, pal)].height(Length::Fill).into());
        main_col.push(self.status(t, pal));
        let main = Column::with_children(main_col).height(Length::Fill);

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
                text(self.title())
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
        let save_btn: Element<'_, Message> = if self.editing || self.editor.dirty {
            icon_button(icon_save(), self.editor.dirty, pal, Message::Save).into()
        } else {
            Space::new().width(Length::Fixed(0.0)).into()
        };
        let edit = icon_button(icon_edit(), self.editing, pal, Message::ToggleEdit);
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
                row![sidebar, title, save_btn, edit, search, settings, min, max, close]
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

    fn edit_toolbar(&self, pal: Palette) -> Element<'_, Message> {
        let mut items: Vec<Element<'_, Message>> = vec![
            flat_button("B", false, pal)
                .on_press(Message::Doc(DocAction::ToggleFormat(FormatKind::Bold)))
                .into(),
            flat_button("I", false, pal)
                .on_press(Message::Doc(DocAction::ToggleFormat(FormatKind::Italic)))
                .into(),
            flat_button("</>", false, pal)
                .on_press(Message::Doc(DocAction::ToggleFormat(FormatKind::Code)))
                .into(),
            flat_button("S", false, pal)
                .on_press(Message::Doc(DocAction::ToggleFormat(FormatKind::Strike)))
                .into(),
            vline(pal.border, 1.0),
            flat_button("撤销", false, pal)
                .on_press(Message::Doc(DocAction::Undo))
                .into(),
            flat_button("重做", false, pal)
                .on_press(Message::Doc(DocAction::Redo))
                .into(),
            flat_button("+表格", false, pal)
                .on_press(Message::Doc(DocAction::InsertTable))
                .into(),
        ];

        if let Some(block) = self.doc.blocks.get(self.editor.caret.block) {
            if let BlockKind::Table(tbl) = &block.kind {
                let cur_cell = tbl.cells.get(self.editor.caret.cell);
                let cell_info = cur_cell
                    .map(|c| format!("表 R{}·C{}", c.row + 1, c.col + 1))
                    .unwrap_or_else(|| String::from("表格"));
                items.extend([
                    vline(pal.border, 1.0),
                    text(cell_info).size(11).color(pal.muted).into(),
                    flat_button("+行", false, pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::InsertRowBelow)))
                        .into(),
                    flat_button("+上行", false, pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::InsertRowAbove)))
                        .into(),
                    flat_button("-行", false, pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::DeleteRow)))
                        .into(),
                    flat_button("+列", false, pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::InsertColRight)))
                        .into(),
                    flat_button("-列", false, pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::DeleteCol)))
                        .into(),
                    flat_button("左", cur_cell.is_some_and(|c| c.align == CellAlign::Left), pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::Align(CellAlign::Left))))
                        .into(),
                    flat_button("中", cur_cell.is_some_and(|c| c.align == CellAlign::Center), pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::Align(CellAlign::Center))))
                        .into(),
                    flat_button("右", cur_cell.is_some_and(|c| c.align == CellAlign::Right), pal)
                        .on_press(Message::Doc(DocAction::TableOp(TableOp::Align(CellAlign::Right))))
                        .into(),
                ]);
            }
        }

        items.push(Space::new().width(Length::Fill).into());
        items.push(
            text("直接点击排版文字即可编辑 · Tab 跳格 · Ctrl+S 保存")
                .size(11)
                .color(pal.muted)
                .into(),
        );

        column![
            container(
                Row::with_children(items)
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .height(Length::Fill)
            )
            .height(Length::Fixed(32.0))
            .width(Length::Fill)
            .padding(Padding::from([0.0, 12.0]))
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
        if !self.sidebar_open {
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
        let width = self.content_width(t.width);
        let search = if self.find_open && !self.matches.is_empty() {
            Some(Arc::new(SearchView {
                matches: self.matches.clone(),
                current: self.current_match,
            }))
        } else {
            None
        };
        let doc_widget = DocWidget {
            id: offsets::doc_id(),
            doc: self.doc.clone(),
            doc_rev: self.doc_rev,
            edited_block: self.edited_block,
            materialize_hint: self.materialize_hint,
            images: self.images.clone(),
            pal,
            theme_idx: self.theme_idx,
            body_font: t.body,
            mono_font: t.mono,
            size: t.size * self.font_scale,
            line: t.line,
            content_width: width,
            editing: self.editing,
            overlay_open: self.modal_open(),
            caret: self.editor.caret,
            anchor: self.editor.anchor,
            search,
            search_rev: self.search_rev,
            renders: self.renders.clone(),
            on_action: Box::new(Message::Doc),
        }
        .into_element();

        let paper = container(doc_widget)
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
            self.shift_held,
        );
        let base: Element<'_, Message> = scroller.into();
        let layers = if self.find_open {
            vec![base, self.find_bar(t, pal)]
        } else {
            vec![base]
        };
        stack(layers).into()
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
            "所见即所得编辑",
            "Ctrl+E",
            Some(self.editing),
            Message::ToggleEdit,
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
                text("轻量中文 Markdown 阅读与所见即所得编辑器")
                    .size(13)
                    .color(pal.muted),
                hline(pal.border, 1.0),
                text("阅读：PageUp/PageDown 翻页 · 拖选跨段跨表复制 · Ctrl+F 查找")
                    .size(11)
                    .color(pal.muted),
                text("编辑：Ctrl+E 进入所见即所得 · Tab 跳表单元格 · Alt+方向键 增删行列")
                    .size(11)
                    .color(pal.muted),
                text("格式：Ctrl+Shift+B/I/K/X 粗斜码删 · Ctrl+Z/Y 撤销重做 · Ctrl+S 保存")
                    .size(11)
                    .color(pal.muted),
                flat_button("关闭", false, pal).on_press(Message::CloseAbout),
            ]
            .spacing(10)
            .padding(Padding::from([20.0, 22.0]))
            .align_x(Alignment::Center),
        )
        .width(Length::Fixed(420.0))
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
            format!("编辑 · 块 {}/{}", self.editor.caret.block + 1, self.doc.blocks.len().max(1))
        } else {
            format!("{:.0}%", self.scroll_ratio * 100.0)
        };
        let words = format!("{} 字", self.doc.words);
        let lines = format!("{} 行", self.doc.lines);
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
        iced::window::Event::Moved(pos) => Some(Message::WindowMoved(pos)),
        iced::window::Event::Resized(size) => Some(Message::WindowResized(size)),
        iced::window::Event::CloseRequested => Some(Message::WindowCloseRequested(id)),
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
    status: iced::event::Status,
    _window: iced::window::Id,
) -> Option<Message> {
    if let iced::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) = event {
        return Some(Message::ShiftChanged(modifiers.shift()));
    }
    if status == iced::event::Status::Captured {
        return None;
    }
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
        iced::keyboard::Key::Named(iced::keyboard::key::Named::PageUp) => Some(Message::PageUp),
        iced::keyboard::Key::Named(iced::keyboard::key::Named::PageDown) => {
            Some(Message::PageDown)
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::Home) if !command => {
            Some(Message::ScrollHome)
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::End) if !command => {
            Some(Message::ScrollEnd)
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowUp) if !command => {
            Some(Message::LineUp)
        }
        iced::keyboard::Key::Named(iced::keyboard::key::Named::ArrowDown) if !command => {
            Some(Message::LineDown)
        }
        iced::keyboard::Key::Character(c) if command => match c.to_ascii_lowercase().as_str() {
            "b" if !shift => Some(Message::ToggleSidebar),
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
        .padding(Padding::from([4.0, 8.0]))
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
    button(content)
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
        })
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
    button(content)
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
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_width_stays_positive() {
        let (mut app, _) = App::new(None, Session::default(), Instant::now(), None);
        app.window_size = Size::new(720.0, 800.0);
        app.sidebar_open = true;
        assert!(app.content_width(760.0) >= MIN_CONTENT);
    }

    #[test]
    fn session_restores_theme_scale_and_last_file() {
        let dir = std::env::temp_dir().join("sudu_test_app_restore");
        let _ = std::fs::create_dir_all(&dir);
        let md = dir.join("note.md");
        std::fs::write(&md, "# 标题\n\n正文内容").expect("write md");

        let mut sess = Session::default();
        sess.last_path = Some(md.to_string_lossy().to_string());
        sess.theme_idx = 2;
        sess.font_scale = 1.25;
        sess.sidebar_open = true;
        sess.record_ratio(&md, 0.42);

        let prev_no = std::env::var_os("SUDU_NO_SESSION");
        let prev_theme = std::env::var_os("SUDU_THEME");
        unsafe {
            std::env::remove_var("SUDU_NO_SESSION");
            std::env::remove_var("SUDU_THEME");
        }
        let (app, _) = App::new(None, sess, Instant::now(), None);
        assert_eq!(app.path.as_deref(), Some(md.as_path()));
        assert_eq!(app.theme_idx, 2);
        assert!((app.font_scale - 1.25).abs() < 0.01);
        assert!(app.sidebar_open);
        assert!((app.scroll_ratio - 0.42).abs() < 0.01);

        if let Some(v) = prev_no {
            unsafe { std::env::set_var("SUDU_NO_SESSION", v) };
        }
        if let Some(v) = prev_theme {
            unsafe { std::env::set_var("SUDU_THEME", v) };
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_and_escape_keep_edit_and_scroll_intact() {
        let (mut app, _) = App::new(None, Session::default(), Instant::now(), None);
        app.scroll_ratio = 0.55;
        let _ = app.update(Message::ToggleEdit);
        assert!(app.editing);
        let _ = app.update(Message::Doc(DocAction::InsertText(String::from("测试插入"))));
        assert!(app.editor.dirty);
        let _ = app.update(Message::OpenFind);
        assert!(app.find_open);
        let _ = app.update(Message::Escape);
        assert!(!app.find_open);
        assert!(app.editing);
        assert!((app.scroll_ratio - 0.55).abs() < 0.001);
    }
}
