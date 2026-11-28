use crate::doc::{Block, BlockKind, Doc, Link, Run, TextMatch};
use crate::editor::{
    self, clamp_pos, next_cluster_boundary, prev_cluster_boundary, unit_plain, DocPos, FormatKind,
    TableOp,
};
use crate::images::{CachedImage, ImageStore};
use crate::table::{CellAlign, Table};
use crate::table_widget::{allocate_column_widths, TablePalette};
use crate::theme::Palette;
use iced::advanced::graphics::text::{cosmic_text, Paragraph};
use iced::advanced::input_method::{self, InputMethod, Purpose};
use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::text::{Alignment as TextAlignment, LineHeight, Paragraph as _, Shaping, Span, Wrapping};
use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{clipboard, image as core_image, svg as core_svg, Clipboard, Layout, Shell};
use iced::keyboard;
use iced::{
    border, window, Border, Color, Element, Event, Font, Length, Point, Rectangle, Size, Vector,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BLOCK_GAP: f32 = 8.0;
const PRELOAD_PX: f32 = 900.0;
const EVICT_PX: f32 = 2600.0;
const BLINK_MS: u128 = 530;
const BLINK_PERIOD_MS: u128 = 1060;
const DOUBLE_CLICK_MS: u128 = 420;
const MAX_IMAGE_HEIGHT: f32 = 560.0;
const SCROLLBAR_HEIGHT: f32 = 12.0;

#[derive(Clone, Debug)]
pub enum DocAction {
    SetSelection {
        caret: DocPos,
        anchor: Option<DocPos>,
    },
    InsertText(String),
    Enter {
        shift: bool,
    },
    Tab {
        shift: bool,
    },
    Backspace,
    Delete,
    MoveWord {
        left: bool,
        extend: bool,
    },
    MoveDocEdge {
        end: bool,
        extend: bool,
    },
    SelectAll,
    DeleteSelection,
    ToggleFormat(FormatKind),
    InsertTable,
    TableOp(TableOp),
    ToggleCheckbox(usize),
    Undo,
    Redo,
    FollowLink(Link),
    ScrollToY(f32),
    AdjustScrollY(f32),
}

pub struct SearchView {
    pub matches: Arc<Vec<TextMatch>>,
    pub current: Option<usize>,
}

pub struct DocWidget<'a> {
    pub id: iced::widget::Id,
    pub doc: Arc<Doc>,
    pub doc_rev: u64,
    pub edited_block: Option<usize>,
    pub materialize_hint: Option<usize>,
    pub images: Arc<ImageStore>,
    pub pal: Palette,
    pub theme_idx: usize,
    pub body_font: Font,
    pub mono_font: Font,
    pub size: f32,
    pub line: f32,
    pub content_width: f32,
    pub editing: bool,
    pub overlay_open: bool,
    pub caret: DocPos,
    pub anchor: Option<DocPos>,
    pub search: Option<Arc<SearchView>>,
    pub search_rev: u64,
    pub renders: Arc<AtomicU32>,
    pub on_action: Box<dyn Fn(DocAction) -> crate::app::Message + 'a>,
}

impl<'a> DocWidget<'a> {
    pub fn into_element(self) -> Element<'a, crate::app::Message> {
        Element::new(self)
    }
}

#[derive(Clone)]
struct SpanVisualMeta {
    highlight_bg: Option<Color>,
    highlight_border: Border,
    padding: iced::Padding,
    underline: bool,
    strikethrough: bool,
    link: Option<Link>,
    color: Color,
    size: f32,
    line_height: f32,
}

#[derive(Clone)]
struct ShapedText {
    paragraph: Paragraph,
    meta: Vec<SpanVisualMeta>,
    line_starts: Vec<usize>,
    plain_len: usize,
    align_x: TextAlignment,
}

#[derive(Clone)]
struct MaterializedCell {
    cell_idx: usize,
    row: usize,
    col: usize,
    colspan: usize,
    rowspan: usize,
    header: bool,
    box_rect: Rectangle,
    text_origin: Point,
    shaped: ShapedText,
}

#[derive(Clone)]
struct MaterializedTable {
    rows: usize,
    cols: usize,
    total_w: f32,
    total_h: f32,
    col_xs: Vec<f32>,
    col_widths: Vec<f32>,
    row_ys: Vec<f32>,
    row_heights: Vec<f32>,
    row_is_header: Vec<bool>,
    has_rowspan: bool,
    cells: Vec<MaterializedCell>,
}

#[derive(Clone)]
enum BlockContent {
    Text {
        shaped: ShapedText,
        text_rect: Rectangle,
        marker: Option<String>,
        marker_origin: Point,
        checkbox_rect: Option<Rectangle>,
        checked: Option<bool>,
    },
    Table(MaterializedTable),
    Image {
        image: CachedImage,
        draw_rect: Rectangle,
        caption: Option<Paragraph>,
        caption_pos: Point,
        caption_text: String,
    },
    Rule {
        y_offset: f32,
    },
}

struct CachedBlock {
    id: iced::widget::Id,
    height: f32,
    measured_width: f32,
    material: Option<BlockContent>,
    shaped_search_rev: u64,
    shaped_has_preedit: bool,
}

#[derive(Clone, Copy)]
enum DragMode {
    Char,
    Word(usize, usize),
    Block(usize),
}

struct TableScrollDrag {
    block: usize,
    start_cursor_x: f32,
    start_scroll_x: f32,
    max_scroll: f32,
    thumb_travel: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct UnitId {
    block: usize,
    cell: usize,
}

struct ClickRecord {
    at: Instant,
    pos: Point,
    count: u8,
}

pub struct DocWidgetState {
    doc_rev: u64,
    doc_ptr: usize,
    width_q: u16,
    size_q: u16,
    line_q: u16,
    theme_idx: usize,
    editing: bool,
    blocks: Vec<CachedBlock>,
    prefix_ys: Vec<f32>,
    total_height: f32,
    last_local_top: f32,
    last_local_bottom: f32,
    table_scroll_x: HashMap<usize, f32>,
    table_drag: Option<TableScrollDrag>,
    hovered_table: Option<(usize, usize)>,
    hovered_link: Option<(UnitId, usize)>,
    read_caret: Option<DocPos>,
    read_anchor: Option<DocPos>,
    dragging: Option<DragMode>,
    last_click: Option<ClickRecord>,
    modifiers: keyboard::Modifiers,
    focused: bool,
    window_focused: bool,
    blink_reset: Instant,
    now: Instant,
    goal_x: Option<f32>,
    preedit: Option<input_method::Preedit>,
    ime_opened: bool,
}

impl DocWidgetState {
    pub fn read_offsets(&self, headings: &[usize], target: Option<usize>) -> (HashMap<usize, f32>, Option<f32>) {
        let mut out = HashMap::with_capacity(headings.len());
        for &h in headings {
            if let Some(&y) = self.prefix_ys.get(h) {
                out.insert(h, y);
            }
        }
        let target_y = target.and_then(|idx| self.prefix_ys.get(idx).copied());
        (out, target_y)
    }

    pub fn first_visible_block(&self) -> usize {
        if self.blocks.is_empty() {
            return 0;
        }
        self.prefix_ys
            .partition_point(|&y| y < self.last_local_top)
            .saturating_sub(1)
            .min(self.blocks.len() - 1)
    }
}

impl Default for DocWidgetState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            doc_rev: 0,
            doc_ptr: 0,
            width_q: 0,
            size_q: 0,
            line_q: 0,
            theme_idx: 0,
            editing: false,
            blocks: Vec::new(),
            prefix_ys: Vec::new(),
            total_height: 0.0,
            last_local_top: 0.0,
            last_local_bottom: 1200.0,
            table_scroll_x: HashMap::new(),
            table_drag: None,
            hovered_table: None,
            hovered_link: None,
            read_caret: None,
            read_anchor: None,
            dragging: None,
            last_click: None,
            modifiers: keyboard::Modifiers::default(),
            focused: true,
            window_focused: true,
            blink_reset: now,
            now,
            goal_x: None,
            preedit: None,
            ime_opened: false,
        }
    }
}

impl DocWidgetState {
    fn active_caret(&self, widget: &DocWidget<'_>) -> Option<DocPos> {
        if widget.editing {
            Some(widget.caret)
        } else {
            self.read_caret
        }
    }

    fn active_selection(&self, widget: &DocWidget<'_>) -> Option<(DocPos, DocPos)> {
        let (anchor, caret) = if widget.editing {
            (widget.anchor?, widget.caret)
        } else {
            (self.read_anchor?, self.read_caret?)
        };
        if anchor == caret {
            None
        } else if anchor < caret {
            Some((anchor, caret))
        } else {
            Some((caret, anchor))
        }
    }

    fn sync(&mut self, widget: &DocWidget<'_>) {
        let doc_ptr = Arc::as_ptr(&widget.doc) as usize;
        let width_q = (widget.content_width * 2.0).round() as u16;
        let size_q = (widget.size * 10.0).round() as u16;
        let line_q = (widget.line * 100.0).round() as u16;
        let style_changed = self.width_q != width_q
            || self.size_q != size_q
            || self.line_q != line_q
            || self.theme_idx != widget.theme_idx
            || self.editing != widget.editing;

        let doc_replaced = self.doc_ptr != doc_ptr || self.doc_rev != widget.doc_rev;
        let same_count = self.blocks.len() == widget.doc.blocks.len();

        if !doc_replaced && !style_changed {
            return;
        }

        if style_changed {
            for b in &mut self.blocks {
                b.measured_width = 0.0;
                b.material = None;
            }
            self.width_q = width_q;
            self.size_q = size_q;
            self.line_q = line_q;
            self.theme_idx = widget.theme_idx;
            if self.editing != widget.editing {
                self.editing = widget.editing;
                self.read_caret = None;
                self.read_anchor = None;
                self.preedit = None;
                self.blink_reset = Instant::now();
            }
        }

        if doc_replaced {
            if let Some(edited) = widget.edited_block {
                if same_count && edited < self.blocks.len() {
                    let block = &widget.doc.blocks[edited];
                    self.blocks[edited].id = block.id.clone();
                    self.blocks[edited].measured_width = 0.0;
                    self.blocks[edited].material = None;
                    if let Some(cached) = self.blocks.get_mut(edited) {
                        let est = estimate_block_height(
                            block,
                            widget.content_width,
                            widget.size,
                            widget.line,
                            widget.editing,
                        );
                        cached.height = est;
                    }
                    self.doc_ptr = doc_ptr;
                    self.doc_rev = widget.doc_rev;
                    self.recompute_prefixes();
                    return;
                }
            }

            if !same_count || widget.edited_block.is_none() {
                self.read_caret = None;
                self.read_anchor = None;
                self.hovered_link = None;
                self.hovered_table = None;
                self.table_scroll_x
                    .retain(|&idx, _| idx < widget.doc.blocks.len());
                self.blocks = widget
                    .doc
                    .blocks
                    .iter()
                    .map(|block| CachedBlock {
                        id: block.id.clone(),
                        height: estimate_block_height(
                            block,
                            widget.content_width,
                            widget.size,
                            widget.line,
                            widget.editing,
                        ),
                        measured_width: 0.0,
                        material: None,
                        shaped_search_rev: 0,
                        shaped_has_preedit: false,
                    })
                    .collect();
            } else {
                for (i, block) in widget.doc.blocks.iter().enumerate() {
                    if self.blocks[i].id != block.id {
                        self.blocks[i].id = block.id.clone();
                        self.blocks[i].measured_width = 0.0;
                        self.blocks[i].material = None;
                        self.blocks[i].height = estimate_block_height(
                            block,
                            widget.content_width,
                            widget.size,
                            widget.line,
                            widget.editing,
                        );
                    }
                }
            }
            self.doc_ptr = doc_ptr;
            self.doc_rev = widget.doc_rev;
        }

        self.recompute_prefixes();
    }

    fn recompute_prefixes(&mut self) {
        self.prefix_ys.clear();
        self.prefix_ys.reserve(self.blocks.len() + 1);
        let mut y = 0.0f32;
        for (i, b) in self.blocks.iter().enumerate() {
            self.prefix_ys.push(y);
            y += b.height;
            if i + 1 < self.blocks.len() {
                y += BLOCK_GAP;
            }
        }
        self.prefix_ys.push(y);
        self.total_height = y.max(40.0);
    }

    fn block_range_for_y(&self, y_start: f32, y_end: f32) -> std::ops::Range<usize> {
        let n = self.blocks.len();
        if n == 0 {
            return 0..0;
        }
        let first = self
            .prefix_ys
            .partition_point(|&y| y < y_start)
            .saturating_sub(1)
            .min(n - 1);
        let mut idx = first;
        while idx < n {
            if self.prefix_ys[idx] > y_end {
                break;
            }
            idx += 1;
        }
        let end = idx.min(n);
        if first >= end {
            first..(first + 1).min(n)
        } else {
            first..end
        }
    }

    fn materialize_range(
        &mut self,
        widget: &DocWidget<'_>,
        range: std::ops::Range<usize>,
        local_top: f32,
        track_delta_above: bool,
    ) -> (bool, f32) {
        let mut changed = false;
        let mut delta_above = 0.0f32;
        let width_q = (widget.content_width * 2.0).round() as u16;
        let target_w = widget.content_width.max(240.0);
        let has_preedit = self
            .preedit
            .as_ref()
            .is_some_and(|p| !p.content.is_empty());

        for idx in range {
            let Some(block) = widget.doc.blocks.get(idx) else {
                continue;
            };
            let is_caret_block = widget.editing && widget.caret.block == idx;
            let want_preedit = is_caret_block && has_preedit;
            let cached = &self.blocks[idx];
            let needs_measure = (cached.measured_width - target_w).abs() > 0.5
                || cached.material.is_none()
                || cached.shaped_search_rev != widget.search_rev
                || cached.shaped_has_preedit != want_preedit;

            if !needs_measure {
                let _ = width_q;
                continue;
            }

            let preedit_payload = if want_preedit {
                self.preedit
                    .as_ref()
                    .map(|p| (widget.caret.cell, widget.caret.offset, p.content.as_str()))
            } else {
                None
            };

            let (content, new_h) = materialize_block(widget, idx, block, target_w, preedit_payload);
            let old_h = self.blocks[idx].height;
            let block_top = self.prefix_ys.get(idx).copied().unwrap_or(0.0);
            if track_delta_above && block_top < local_top && (new_h - old_h).abs() > 0.5 {
                delta_above += new_h - old_h;
            }
            if (new_h - old_h).abs() > 0.5 {
                self.blocks[idx].height = new_h;
                changed = true;
            }
            self.blocks[idx].measured_width = target_w;
            self.blocks[idx].material = Some(content);
            self.blocks[idx].shaped_search_rev = widget.search_rev;
            self.blocks[idx].shaped_has_preedit = want_preedit;
            widget.renders.fetch_add(1, Ordering::Relaxed);
        }

        if changed {
            self.recompute_prefixes();
        }
        (changed, delta_above)
    }

    fn evict_outside(&mut self, keep_start: usize, keep_end: usize, protected_block: Option<usize>) {
        for (i, b) in self.blocks.iter_mut().enumerate() {
            if (i < keep_start || i >= keep_end) && Some(i) != protected_block {
                b.material = None;
            }
        }
    }

    fn caret_is_visible_blink(&self) -> bool {
        if !self.focused || !self.window_focused {
            return false;
        }
        let elapsed = self.now.saturating_duration_since(self.blink_reset).as_millis();
        (elapsed % BLINK_PERIOD_MS) < BLINK_MS
    }

    fn reset_blink(&mut self) {
        let now = Instant::now();
        self.blink_reset = now;
        self.now = now;
    }
}

impl Widget<crate::app::Message, iced::Theme, iced::Renderer> for DocWidget<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DocWidgetState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DocWidgetState::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.content_width), Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<DocWidgetState>();
        state.sync(self);

        let top = (state.last_local_top - PRELOAD_PX).max(0.0);
        let bottom = (state.last_local_bottom + PRELOAD_PX).max(top + 1200.0);
        let range = state.block_range_for_y(top, bottom);
        state.materialize_range(self, range, state.last_local_top, false);
        if self.editing && self.caret.block < state.blocks.len() {
            state.materialize_range(
                self,
                self.caret.block..self.caret.block + 1,
                state.last_local_top,
                false,
            );
        }

        let resolved = limits.resolve(
            Length::Fixed(self.content_width),
            Length::Fixed(state.total_height),
            Size::new(self.content_width, state.total_height),
        );
        if let Some(hint) = self.materialize_hint {
            if hint < state.blocks.len() {
                let lo = hint.saturating_sub(2);
                let hi = (hint + 14).min(state.blocks.len());
                state.materialize_range(self, lo..hi, state.last_local_top, false);
            }
        }
        layout::Node::new(resolved)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<DocWidgetState>();
        state.sync(self);
        let bounds = layout.bounds();
        operation.custom(Some(&self.id), bounds, state);
        operation.container(Some(&self.id), bounds);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, crate::app::Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<DocWidgetState>();
        state.sync(self);

        let bounds = layout.bounds();
        let local_top = (viewport.y - bounds.y).max(0.0);
        let local_bottom = (viewport.y + viewport.height - bounds.y).max(local_top);
        state.last_local_top = local_top;
        state.last_local_bottom = local_bottom;

        let visible_range = state.block_range_for_y(
            (local_top - PRELOAD_PX).max(0.0),
            local_bottom + PRELOAD_PX,
        );
        let (height_changed, delta_above) =
            state.materialize_range(self, visible_range, local_top, true);
        if height_changed {
            shell.invalidate_layout();
            shell.request_redraw();
            if delta_above.abs() > 0.5 {
                shell.publish((self.on_action)(DocAction::AdjustScrollY(delta_above)));
            }
        }

        let keep_range = state.block_range_for_y(
            (local_top - EVICT_PX).max(0.0),
            local_bottom + EVICT_PX,
        );
        state.evict_outside(
            keep_range.start,
            keep_range.end,
            self.editing.then_some(self.caret.block),
        );

        match event {
            Event::Window(window::Event::Focused) => {
                state.window_focused = true;
                state.reset_blink();
                shell.request_redraw();
            }
            Event::Window(window::Event::Unfocused) => {
                state.window_focused = false;
                shell.request_redraw();
            }
            Event::Window(window::Event::RedrawRequested(now)) => {
                if self.editing && state.focused && state.window_focused && !self.overlay_open {
                    state.now = *now;
                    let until = BLINK_MS
                        - (state.now.saturating_duration_since(state.blink_reset).as_millis()
                            % BLINK_MS);
                    shell.request_redraw_at(state.now + Duration::from_millis(until as u64 + 8));
                    if let Some(caret_rect) = caret_screen_rect(state, self, bounds.position()) {
                        let ime_rect = Rectangle {
                            x: caret_rect.x,
                            y: caret_rect.y,
                            width: 2.0,
                            height: caret_rect.height.max(16.0),
                        };
                        if bounds.intersects(viewport) {
                            shell.request_input_method::<&str>(&InputMethod::Enabled {
                                cursor: ime_rect,
                                purpose: Purpose::Normal,
                                preedit: None,
                            });
                        }
                    }
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                state.modifiers = *m;
            }
            _ => {}
        }

        if let Event::Mouse(mouse_event) = event {
            self.handle_mouse(state, mouse_event, bounds, cursor, shell, viewport);
        }

        if self.overlay_open {
            return;
        }

        if self.editing && state.focused {
            match event {
                Event::InputMethod(ime_event) => match ime_event {
                    input_method::Event::Opened => {
                        state.ime_opened = true;
                        state.preedit = Some(input_method::Preedit::new());
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    input_method::Event::Preedit(content, selection) => {
                        state.preedit = Some(input_method::Preedit {
                            content: content.clone(),
                            selection: selection.clone(),
                            text_size: Some(iced::Pixels(self.size)),
                        });
                        state.blocks[self.caret.block].measured_width = 0.0;
                        shell.invalidate_layout();
                        shell.request_redraw();
                        shell.capture_event();
                    }
                    input_method::Event::Commit(text) => {
                        state.preedit = None;
                        if !text.is_empty() {
                            state.reset_blink();
                            state.goal_x = None;
                            if self.caret.block < state.blocks.len() {
                                state.blocks[self.caret.block].measured_width = 0.0;
                            }
                            shell.publish((self.on_action)(DocAction::InsertText(text.clone())));
                        } else {
                            shell.invalidate_layout();
                            shell.request_redraw();
                        }
                        shell.capture_event();
                    }
                    input_method::Event::Closed => {
                        state.ime_opened = false;
                        state.preedit = None;
                        if self.caret.block < state.blocks.len() {
                            state.blocks[self.caret.block].measured_width = 0.0;
                        }
                        shell.invalidate_layout();
                        shell.request_redraw();
                        shell.capture_event();
                    }
                },
                Event::Keyboard(keyboard::Event::KeyPressed { key, text, modifiers, .. }) => {
                    state.modifiers = *modifiers;
                    if self.handle_edit_key(
                        state,
                        key,
                        text.as_deref(),
                        *modifiers,
                        bounds,
                        viewport,
                        clipboard,
                        shell,
                    ) {
                        shell.capture_event();
                    }
                }
                _ => {}
            }
        } else {
            if let Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
                state.modifiers = *modifiers;
                if self.handle_read_key(state, key, *modifiers, clipboard, shell) {
                    shell.capture_event();
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<DocWidgetState>();
        if state.table_drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if state.hovered_link.is_some() && !self.editing {
            return mouse::Interaction::Pointer;
        }
        if let Some(pos) = cursor.position_in(layout.bounds()) {
            if let Some(hit) = hit_checkbox(state, self, pos) {
                let _ = hit;
                return mouse::Interaction::Pointer;
            }
            if hit_table_scrollbar(state, self, pos).is_some() {
                return mouse::Interaction::Pointer;
            }
            return mouse::Interaction::Text;
        }
        mouse::Interaction::None
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<DocWidgetState>();
        let bounds = layout.bounds();
        if !bounds.intersects(viewport) || state.blocks.is_empty() {
            return;
        }

        let local_top = (viewport.y - bounds.y).max(0.0);
        let local_bottom = (viewport.y + viewport.height - bounds.y).max(local_top);
        let visible_range = state.block_range_for_y(local_top - 40.0, local_bottom + 40.0);
        let selection = state.active_selection(self);
        let show_caret = self.editing && state.caret_is_visible_blink();
        let tp = TablePalette::from_palette(self.pal, false);

        for idx in visible_range {
            let Some(block) = self.doc.blocks.get(idx) else {
                continue;
            };
            let Some(cached) = state.blocks.get(idx) else {
                continue;
            };
            let Some(material) = &cached.material else {
                continue;
            };
            let block_y = bounds.y + state.prefix_ys[idx];
            let block_rect = Rectangle::new(
                Point::new(bounds.x, block_y),
                Size::new(bounds.width, cached.height),
            );
            if !block_rect.intersects(viewport) {
                continue;
            }

            if self.editing && self.caret.block == idx && !matches!(block.kind, BlockKind::Table(_)) {
                draw_active_block_cue(renderer, block_rect, self.pal, &block.kind);
            }

            match material {
                BlockContent::Rule { y_offset } => {
                    use iced::advanced::Renderer as _;
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(bounds.x, block_y + *y_offset),
                                Size::new(bounds.width, 1.0),
                            ),
                            snap: true,
                            ..Default::default()
                        },
                        self.pal.border,
                    );
                }
                BlockContent::Image {
                    image,
                    draw_rect,
                    caption,
                    caption_pos,
                    caption_text,
                } => {
                    let abs_rect = Rectangle::new(
                        Point::new(bounds.x + draw_rect.x, block_y + draw_rect.y),
                        draw_rect.size(),
                    );
                    draw_cached_image(renderer, image, abs_rect, self.pal, viewport);
                    if let Some(cap) = caption {
                        let _ = caption_text;
                        let origin = Point::new(bounds.x + caption_pos.x, block_y + caption_pos.y);
                        use iced::advanced::text::Renderer as _;
                        renderer.fill_paragraph(cap, origin, self.pal.muted, *viewport);
                    }
                    if show_caret && self.caret.block == idx {
                        use iced::advanced::Renderer as _;
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle::new(
                                    Point::new(abs_rect.x - 4.0, abs_rect.y),
                                    Size::new(2.5, abs_rect.height),
                                ),
                                ..Default::default()
                            },
                            self.pal.accent,
                        );
                    }
                }
                BlockContent::Text {
                    shaped,
                    text_rect,
                    marker,
                    marker_origin,
                    checkbox_rect,
                    checked,
                } => {
                    draw_block_chrome(
                        renderer,
                        block,
                        block_rect,
                        self.pal,
                        self.editing && self.caret.block == idx,
                        self.mono_font,
                        self.size,
                        viewport,
                    );

                    if let Some(cb) = checkbox_rect {
                        let abs_cb = Rectangle::new(
                            Point::new(bounds.x + cb.x, block_y + cb.y),
                            cb.size(),
                        );
                        draw_checkbox(renderer, abs_cb, checked.unwrap_or(false), self.pal);
                    } else if let Some(m) = marker {
                        let marker_pos =
                            Point::new(bounds.x + marker_origin.x, block_y + marker_origin.y);
                        let m_par = Paragraph::with_text(iced::advanced::Text {
                            content: m.as_str(),
                            bounds: Size::new(60.0, self.size * 2.0),
                            size: iced::Pixels(self.size),
                            line_height: LineHeight::Relative(self.line),
                            font: self.body_font,
                            align_x: TextAlignment::Left,
                            align_y: iced::alignment::Vertical::Top,
                            shaping: Shaping::Auto,
                            wrapping: Wrapping::None,
                        });
                        use iced::advanced::text::Renderer as _;
                        renderer.fill_paragraph(&m_par, marker_pos, self.pal.muted, *viewport);
                    }

                    let origin = Point::new(bounds.x + text_rect.x, block_y + text_rect.y);
                    let unit_sel = selection_for_unit(
                        selection,
                        idx,
                        0,
                        shaped.plain_len,
                        &self.doc.blocks[idx],
                    );
                    let hover_span = if !self.editing {
                        state
                            .hovered_link
                            .and_then(|(u, s)| (u.block == idx && u.cell == 0).then_some(s))
                    } else {
                        None
                    };
                    draw_shaped_text(
                        renderer,
                        shaped,
                        origin,
                        self.pal,
                        defaults,
                        unit_sel,
                        hover_span,
                        viewport,
                    );
                    if show_caret && self.caret.block == idx {
                        let pre_len = state
                            .preedit
                            .as_ref()
                            .map(|p| p.content.len())
                            .unwrap_or(0);
                        draw_caret_in_shaped(
                            renderer,
                            shaped,
                            origin,
                            self.caret.offset + pre_len,
                            self.pal.accent,
                        );
                    }
                }
                BlockContent::Table(table_mat) => {
                    let scroll_x = state.table_scroll_x.get(&idx).copied().unwrap_or(0.0);
                    let table_origin = Point::new(bounds.x - scroll_x, block_y + 4.0);
                    let clip_rect = Rectangle::new(
                        Point::new(bounds.x, block_y),
                        Size::new(bounds.width, cached.height),
                    )
                    .intersection(viewport)
                    .unwrap_or(*viewport);
                    let hovered_row = state
                        .hovered_table
                        .and_then(|(b, r)| (b == idx).then_some(r));
                    let active_cell = (self.editing && self.caret.block == idx)
                        .then_some(self.caret.cell);
                    let mut table_pal = tp;
                    table_pal.zebra = !table_mat.has_rowspan;

                    let pre_len = state
                        .preedit
                        .as_ref()
                        .map(|p| p.content.len())
                        .unwrap_or(0);
                    draw_materialized_table(
                        renderer,
                        table_mat,
                        table_origin,
                        clip_rect,
                        table_pal,
                        self.pal,
                        defaults,
                        selection,
                        idx,
                        hovered_row,
                        active_cell,
                        show_caret.then_some(self.caret.offset + pre_len),
                        if !self.editing {
                            state.hovered_link
                        } else {
                            None
                        },
                        state,
                    );

                    if table_mat.total_w > bounds.width + 1.0 {
                        draw_table_scrollbar(
                            renderer,
                            Point::new(bounds.x, block_y + 4.0 + table_mat.total_h + 3.0),
                            bounds.width,
                            table_mat.total_w,
                            scroll_x,
                            self.pal,
                            state.table_drag.as_ref().is_some_and(|d| d.block == idx),
                        );
                    }
                }
            }
        }
    }
}

impl<'a> DocWidget<'a> {
    fn handle_mouse(
        &self,
        state: &mut DocWidgetState,
        mouse_event: &mouse::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, crate::app::Message>,
        viewport: &Rectangle,
    ) {
        match mouse_event {
            mouse::Event::WheelScrolled { delta } => {
                if let Some(pos) = cursor.position_in(bounds) {
                    if let Some((block_idx, table_mat)) = table_under_point(state, pos) {
                        if table_mat.total_w > bounds.width + 1.0 {
                            let dx = match *delta {
                                mouse::ScrollDelta::Lines { x, y } => {
                                    if state.modifiers.shift() && y != 0.0 {
                                        -y * 48.0
                                    } else if x != 0.0 {
                                        -x * 48.0
                                    } else {
                                        0.0
                                    }
                                }
                                mouse::ScrollDelta::Pixels { x, y } => {
                                    if state.modifiers.shift() && y.abs() > x.abs() {
                                        -y
                                    } else if x.abs() > y.abs() {
                                        -x
                                    } else {
                                        0.0
                                    }
                                }
                            };
                            if dx.abs() > 0.1 {
                                let max_s = (table_mat.total_w - bounds.width).max(0.0);
                                let cur = state.table_scroll_x.get(&block_idx).copied().unwrap_or(0.0);
                                let next = (cur + dx).clamp(0.0, max_s);
                                if (next - cur).abs() > 0.1 {
                                    state.table_scroll_x.insert(block_idx, next);
                                    shell.request_redraw();
                                    shell.capture_event();
                                }
                            }
                        }
                    }
                }
            }
            mouse::Event::CursorMoved { .. } => {
                if let Some(drag) = &state.table_drag {
                    if let Some(cursor_pos) = cursor.position() {
                        let dx = cursor_pos.x - drag.start_cursor_x;
                        let ratio = if drag.thumb_travel > 0.1 {
                            dx / drag.thumb_travel
                        } else {
                            0.0
                        };
                        let next = (drag.start_scroll_x + ratio * drag.max_scroll)
                            .clamp(0.0, drag.max_scroll);
                        state.table_scroll_x.insert(drag.block, next);
                        shell.request_redraw();
                        return;
                    }
                }

                if let Some(drag_mode) = state.dragging {
                    if let Some(pos) = cursor.position() {
                        let local = Point::new(pos.x - bounds.x, pos.y - bounds.y);
                        if let Some(hit_pos) = hit_doc_pos(state, self, local) {
                            match drag_mode {
                                DragMode::Char => {
                                    self.set_selection(state, hit_pos, true, shell);
                                }
                                DragMode::Word(w_start_block, w_cell) => {
                                    let _ = (w_start_block, w_cell);
                                    self.set_selection(state, hit_pos, true, shell);
                                }
                                DragMode::Block(b_idx) => {
                                    let target = if hit_pos.block >= b_idx {
                                        editor::end_of_block_pos(&self.doc, hit_pos.block)
                                    } else {
                                        DocPos::new(hit_pos.block, 0, 0)
                                    };
                                    self.set_selection(state, target, true, shell);
                                }
                            }
                        }
                    }
                    return;
                }

                let prev_table = state.hovered_table;
                let prev_link = state.hovered_link;
                if let Some(local) = cursor.position_in(bounds) {
                    if viewport.contains(cursor.position().unwrap_or(Point::ORIGIN)) {
                        state.hovered_table = hit_table_row(state, local);
                        state.hovered_link = hit_link_at(state, local);
                    } else {
                        state.hovered_table = None;
                        state.hovered_link = None;
                    }
                } else {
                    state.hovered_table = None;
                    state.hovered_link = None;
                }
                if state.hovered_table != prev_table || state.hovered_link != prev_link {
                    shell.request_redraw();
                }
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let Some(local) = cursor.position_in(bounds) else {
                    if !self.overlay_open && self.editing {
                        state.focused = false;
                        shell.request_redraw();
                    }
                    return;
                };
                if !viewport.contains(cursor.position().unwrap_or(Point::ORIGIN)) {
                    return;
                }
                state.focused = true;
                state.reset_blink();
                state.goal_x = None;

                if let Some((block_idx, table_w, scroll_x)) = hit_table_scrollbar(state, self, local) {
                    let max_scroll = (table_w - bounds.width).max(1.0);
                    let thumb_w = (bounds.width / table_w * bounds.width).clamp(28.0, bounds.width);
                    let thumb_travel = (bounds.width - thumb_w).max(1.0);
                    let thumb_x = (scroll_x / max_scroll) * thumb_travel;
                    if local.x >= thumb_x && local.x <= thumb_x + thumb_w {
                        state.table_drag = Some(TableScrollDrag {
                            block: block_idx,
                            start_cursor_x: cursor.position().unwrap_or(Point::ORIGIN).x,
                            start_scroll_x: scroll_x,
                            max_scroll,
                            thumb_travel,
                        });
                    } else {
                        let click_ratio = ((local.x - thumb_w * 0.5) / thumb_travel).clamp(0.0, 1.0);
                        state.table_scroll_x.insert(block_idx, click_ratio * max_scroll);
                        shell.request_redraw();
                    }
                    shell.capture_event();
                    return;
                }

                if let Some(cb_block) = hit_checkbox(state, self, local) {
                    shell.publish((self.on_action)(DocAction::ToggleCheckbox(cb_block)));
                    shell.capture_event();
                    return;
                }

                let abs_pos = cursor.position().unwrap_or(Point::ORIGIN);
                let now = Instant::now();
                let click_count = match &state.last_click {
                    Some(prev)
                        if now.duration_since(prev.at).as_millis() <= DOUBLE_CLICK_MS
                            && prev.pos.distance(abs_pos) <= 6.0 =>
                    {
                        (prev.count % 3) + 1
                    }
                    _ => 1,
                };
                state.last_click = Some(ClickRecord {
                    at: now,
                    pos: abs_pos,
                    count: click_count,
                });

                if !self.editing && click_count == 1 && !state.modifiers.shift() {
                    if let Some((unit, span_idx)) = hit_link_at(state, local) {
                        if let Some(link) = extract_link(state, unit, span_idx) {
                            shell.publish((self.on_action)(DocAction::FollowLink(link)));
                            shell.capture_event();
                            return;
                        }
                    }
                }

                if let Some(hit) = hit_doc_pos(state, self, local) {
                    match click_count {
                        1 => {
                            let extend = state.modifiers.shift();
                            self.set_selection(state, hit, extend, shell);
                            state.dragging = Some(DragMode::Char);
                        }
                        2 => {
                            let (ws, we) = word_bounds_at(&self.doc, hit);
                            self.set_exact_selection(state, Some(ws), we, shell);
                            state.dragging = Some(DragMode::Word(hit.block, hit.cell));
                        }
                        _ => {
                            let bs = DocPos::new(hit.block, hit.cell, 0);
                            let be = DocPos::new(
                                hit.block,
                                hit.cell,
                                unit_plain(&self.doc, hit).len(),
                            );
                            self.set_exact_selection(state, Some(bs), be, shell);
                            state.dragging = Some(DragMode::Block(hit.block));
                        }
                    }
                    shell.capture_event();
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if state.table_drag.take().is_some() || state.dragging.take().is_some() {
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn set_selection(
        &self,
        state: &mut DocWidgetState,
        pos: DocPos,
        extend: bool,
        shell: &mut Shell<'_, crate::app::Message>,
    ) {
        let clamped = clamp_pos(&self.doc, pos);
        if self.editing {
            let new_anchor = if extend {
                Some(self.anchor.unwrap_or(self.caret))
            } else {
                None
            };
            if clamped != self.caret || new_anchor != self.anchor {
                shell.publish((self.on_action)(DocAction::SetSelection {
                    caret: clamped,
                    anchor: new_anchor,
                }));
            }
        } else {
            if extend {
                if state.read_anchor.is_none() {
                    state.read_anchor = Some(state.read_caret.unwrap_or(clamped));
                }
            } else {
                state.read_anchor = Some(clamped);
            }
            state.read_caret = Some(clamped);
            shell.request_redraw();
        }
    }

    fn set_exact_selection(
        &self,
        state: &mut DocWidgetState,
        anchor: Option<DocPos>,
        caret: DocPos,
        shell: &mut Shell<'_, crate::app::Message>,
    ) {
        let caret = clamp_pos(&self.doc, caret);
        let anchor = anchor.map(|a| clamp_pos(&self.doc, a));
        if self.editing {
            shell.publish((self.on_action)(DocAction::SetSelection { caret, anchor }));
        } else {
            state.read_anchor = anchor;
            state.read_caret = Some(caret);
            shell.request_redraw();
        }
    }

    fn handle_read_key(
        &self,
        state: &mut DocWidgetState,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, crate::app::Message>,
    ) -> bool {
        let cmd = modifiers.command();
        match key.as_ref() {
            keyboard::Key::Character("c") | keyboard::Key::Character("C") if cmd => {
                if let Some((start, end)) = state.active_selection(self) {
                    let copied = editor::extract_selected_text(&self.doc, start, end);
                    if !copied.is_empty() {
                        clipboard.write(clipboard::Kind::Standard, copied);
                        return true;
                    }
                }
                false
            }
            keyboard::Key::Character("a") | keyboard::Key::Character("A") if cmd => {
                if !self.doc.blocks.is_empty() {
                    let start = editor::first_editable_pos(&self.doc);
                    let end = editor::last_editable_pos(&self.doc);
                    state.read_anchor = Some(start);
                    state.read_caret = Some(end);
                    shell.request_redraw();
                    return true;
                }
                false
            }
            keyboard::Key::Named(keyboard::key::Named::Escape) => {
                if state.active_selection(self).is_some() {
                    state.read_anchor = None;
                    state.read_caret = None;
                    shell.request_redraw();
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn handle_edit_key(
        &self,
        state: &mut DocWidgetState,
        key: &keyboard::Key,
        text: Option<&str>,
        modifiers: keyboard::Modifiers,
        bounds: Rectangle,
        viewport: &Rectangle,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, crate::app::Message>,
    ) -> bool {
        let cmd = modifiers.command();
        let shift = modifiers.shift();
        let alt = modifiers.alt();

        if state
            .preedit
            .as_ref()
            .is_some_and(|p| !p.content.is_empty())
        {
            return false;
        }

        if cmd {
            if let keyboard::Key::Character(ch) = key.as_ref() {
                let lower = ch.to_ascii_lowercase();
                match lower.as_str() {
                    "c" => {
                        if let Some((start, end)) = state.active_selection(self) {
                            let copied = editor::extract_selected_text(&self.doc, start, end);
                            clipboard.write(clipboard::Kind::Standard, copied);
                        }
                        return true;
                    }
                    "x" if !shift => {
                        if let Some((start, end)) = state.active_selection(self) {
                            let copied = editor::extract_selected_text(&self.doc, start, end);
                            clipboard.write(clipboard::Kind::Standard, copied);
                            shell.publish((self.on_action)(DocAction::DeleteSelection));
                        }
                        return true;
                    }
                    "v" => {
                        if let Some(pasted) = clipboard.read(clipboard::Kind::Standard) {
                            if !pasted.is_empty() {
                                state.reset_blink();
                                state.goal_x = None;
                                shell.publish((self.on_action)(DocAction::InsertText(pasted)));
                            }
                        }
                        return true;
                    }
                    "a" => {
                        shell.publish((self.on_action)(DocAction::SelectAll));
                        return true;
                    }
                    "z" if !shift => {
                        state.reset_blink();
                        shell.publish((self.on_action)(DocAction::Undo));
                        return true;
                    }
                    "z" | "y" => {
                        state.reset_blink();
                        shell.publish((self.on_action)(DocAction::Redo));
                        return true;
                    }
                    "b" if shift => {
                        shell.publish((self.on_action)(DocAction::ToggleFormat(FormatKind::Bold)));
                        return true;
                    }
                    "i" if shift => {
                        shell.publish((self.on_action)(DocAction::ToggleFormat(FormatKind::Italic)));
                        return true;
                    }
                    "k" if shift => {
                        shell.publish((self.on_action)(DocAction::ToggleFormat(FormatKind::Code)));
                        return true;
                    }
                    "x" if shift => {
                        shell.publish((self.on_action)(DocAction::ToggleFormat(FormatKind::Strike)));
                        return true;
                    }
                    _ => {}
                }
            }
        }

        if alt && matches!(self.doc.blocks.get(self.caret.block).map(|b| &b.kind), Some(BlockKind::Table(_))) {
            match key.as_ref() {
                keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                    let op = if shift { TableOp::InsertRowAbove } else { TableOp::InsertRowBelow };
                    shell.publish((self.on_action)(DocAction::TableOp(op)));
                    return true;
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                    let op = if shift { TableOp::DeleteRow } else { TableOp::InsertRowAbove };
                    shell.publish((self.on_action)(DocAction::TableOp(op)));
                    return true;
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                    shell.publish((self.on_action)(DocAction::TableOp(TableOp::InsertColRight)));
                    return true;
                }
                keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                    let op = if shift { TableOp::DeleteCol } else { TableOp::InsertColLeft };
                    shell.publish((self.on_action)(DocAction::TableOp(op)));
                    return true;
                }
                _ => {}
            }
        }

        match key.as_ref() {
            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                state.reset_blink();
                state.goal_x = None;
                if cmd {
                    shell.publish((self.on_action)(DocAction::MoveWord {
                        left: true,
                        extend: shift,
                    }));
                } else {
                    let next = if !shift && self.anchor.is_some() {
                        state
                            .active_selection(self)
                            .map(|(s, _)| s)
                            .unwrap_or(self.caret)
                    } else {
                        editor::step_pos_left(&self.doc, self.caret)
                    };
                    self.set_selection(state, next, shift, shell);
                    ensure_caret_visible(state, self, next, bounds, viewport, shell);
                }
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                state.reset_blink();
                state.goal_x = None;
                if cmd {
                    shell.publish((self.on_action)(DocAction::MoveWord {
                        left: false,
                        extend: shift,
                    }));
                } else {
                    let next = if !shift && self.anchor.is_some() {
                        state
                            .active_selection(self)
                            .map(|(_, e)| e)
                            .unwrap_or(self.caret)
                    } else {
                        editor::step_pos_right(&self.doc, self.caret)
                    };
                    self.set_selection(state, next, shift, shell);
                    ensure_caret_visible(state, self, next, bounds, viewport, shell);
                }
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                state.reset_blink();
                let next = move_vertical(state, self, -1);
                self.set_selection(state, next, shift, shell);
                ensure_caret_visible(state, self, next, bounds, viewport, shell);
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                state.reset_blink();
                let next = move_vertical(state, self, 1);
                self.set_selection(state, next, shift, shell);
                ensure_caret_visible(state, self, next, bounds, viewport, shell);
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::Home) => {
                state.reset_blink();
                state.goal_x = None;
                if cmd {
                    shell.publish((self.on_action)(DocAction::MoveDocEdge {
                        end: false,
                        extend: shift,
                    }));
                } else {
                    let next = move_line_edge(state, self, false);
                    self.set_selection(state, next, shift, shell);
                }
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::End) => {
                state.reset_blink();
                state.goal_x = None;
                if cmd {
                    shell.publish((self.on_action)(DocAction::MoveDocEdge {
                        end: true,
                        extend: shift,
                    }));
                } else {
                    let next = move_line_edge(state, self, true);
                    self.set_selection(state, next, shift, shell);
                }
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::Enter) => {
                state.reset_blink();
                state.goal_x = None;
                shell.publish((self.on_action)(DocAction::Enter { shift }));
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::Tab) => {
                state.reset_blink();
                state.goal_x = None;
                shell.publish((self.on_action)(DocAction::Tab { shift }));
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::Backspace) => {
                state.reset_blink();
                state.goal_x = None;
                shell.publish((self.on_action)(DocAction::Backspace));
                return true;
            }
            keyboard::Key::Named(keyboard::key::Named::Delete) => {
                state.reset_blink();
                state.goal_x = None;
                shell.publish((self.on_action)(DocAction::Delete));
                return true;
            }
            _ => {}
        }

        if !cmd && !alt {
            if let Some(txt) = text {
                let filtered: String = txt
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                if !filtered.is_empty() {
                    state.reset_blink();
                    state.goal_x = None;
                    shell.publish((self.on_action)(DocAction::InsertText(filtered)));
                    return true;
                }
            }
            if matches!(key.as_ref(), keyboard::Key::Named(keyboard::key::Named::Space)) {
                state.reset_blink();
                state.goal_x = None;
                shell.publish((self.on_action)(DocAction::InsertText(String::from(" "))));
                return true;
            }
        }

        false
    }
}

fn block_font_size(base_size: f32, kind: &BlockKind) -> f32 {
    match kind {
        BlockKind::Heading(1) => base_size * 1.7,
        BlockKind::Heading(2) => base_size * 1.35,
        BlockKind::Heading(3) => base_size * 1.15,
        BlockKind::Heading(4) => base_size * 1.05,
        BlockKind::Heading(_) => base_size,
        BlockKind::Code { .. } => base_size * 0.9,
        _ => base_size,
    }
}

fn block_line_height(base_line: f32, kind: &BlockKind) -> f32 {
    match kind {
        BlockKind::Code { .. } => 1.55,
        BlockKind::Heading(_) => 1.35,
        _ => base_line,
    }
}

fn block_insets(kind: &BlockKind) -> (f32, f32, f32, f32) {
    match kind {
        BlockKind::Heading(1) => (0.0, 14.0, 0.0, 8.0),
        BlockKind::Heading(2) => (0.0, 12.0, 0.0, 4.0),
        BlockKind::Heading(3) => (0.0, 10.0, 0.0, 2.0),
        BlockKind::Heading(_) => (0.0, 8.0, 0.0, 2.0),
        BlockKind::Code { .. } => (12.0, 10.0, 12.0, 10.0),
        BlockKind::Quote => (14.0, 4.0, 12.0, 4.0),
        BlockKind::ListItem { indent, .. } => (8.0 + *indent as f32 * 18.0, 2.0, 0.0, 2.0),
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

pub fn estimate_block_height(
    block: &Block,
    width: f32,
    base_size: f32,
    base_line: f32,
    editing: bool,
) -> f32 {
    let size = block_font_size(base_size, &block.kind);
    let line = block_line_height(base_line, &block.kind);
    let line_px = size * line;
    match &block.kind {
        BlockKind::Rule => 21.0,
        BlockKind::Image { alt, .. } => {
            if alt.is_empty() && !editing {
                260.0
            } else {
                288.0
            }
        }
        BlockKind::Table(table) => {
            let pad_y = (base_size * 0.46).clamp(7.0, 12.0);
            let tline = (base_line * 0.88).clamp(1.45, 1.72);
            let min_row = (base_size * tline + pad_y * 2.0).ceil();
            let est_cols = table.cols.max(1) as f32;
            let avg_chars = (table.plain.chars().count() as f32) / table.rows.max(1) as f32 / est_cols;
            let col_chars = ((width / est_cols).max(70.0) / (base_size * 0.72)).max(6.0);
            let lines_per_row = (avg_chars / col_chars).ceil().clamp(1.0, 4.0);
            let est_row = (lines_per_row * base_size * tline + pad_y * 2.0).max(min_row);
            (table.rows.max(1) as f32 * (est_row + 1.0) + 10.0).ceil()
        }
        _ => {
            let (l, t, r, b) = block_insets(&block.kind);
            let gutter = if matches!(block.kind, BlockKind::ListItem { .. }) {
                (base_size * 1.6).max(24.0)
            } else {
                0.0
            };
            let avail_w = (width - l - r - gutter).max(80.0);
            let cpl = (avail_w / (size * 0.68)).max(10.0);
            let logical_lines: f32 = block
                .plain
                .split('\n')
                .map(|seg| {
                    let units: f32 = seg
                        .chars()
                        .map(|c| if c.is_ascii() { 0.65 } else { 1.3 })
                        .sum();
                    (units / cpl).ceil().max(1.0)
                })
                .sum();
            let code_badge = if matches!(block.kind, BlockKind::Code { .. }) && editing {
                8.0
            } else {
                0.0
            };
            (logical_lines.max(1.0) * line_px + t + b + code_badge).ceil()
        }
    }
}

fn compute_line_starts(plain: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (idx, b) in plain.bytes().enumerate() {
        if b == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn build_preedit_runs(runs: &[Run], ins_off: usize, pre_txt: &str) -> Vec<(Run, bool)> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut cursor = 0usize;
    let mut inserted = false;
    for run in runs {
        let r_end = cursor + run.text.len();
        if !inserted && ins_off >= cursor && ins_off <= r_end {
            let local = ins_off - cursor;
            let left = &run.text[..local];
            let right = &run.text[local..];
            if !left.is_empty() {
                let mut l = run.clone();
                l.text = left.to_string();
                out.push((l, false));
            }
            out.push((
                Run {
                    text: pre_txt.to_string(),
                    bold: run.bold,
                    italic: run.italic,
                    code: run.code,
                    strike: run.strike,
                    link: None,
                },
                true,
            ));
            if !right.is_empty() {
                let mut r = run.clone();
                r.text = right.to_string();
                out.push((r, false));
            }
            inserted = true;
        } else {
            out.push((run.clone(), false));
        }
        cursor = r_end;
    }
    if !inserted {
        out.push((
            Run {
                text: pre_txt.to_string(),
                bold: false,
                italic: false,
                code: false,
                strike: false,
                link: None,
            },
            true,
        ));
    }
    out
}

fn shape_runs(
    runs: &[Run],
    size: f32,
    line_height_rel: f32,
    pal: Palette,
    body_font: Font,
    mono_font: Font,
    force_bold: bool,
    quote: bool,
    code_block: bool,
    search: Option<&Arc<SearchView>>,
    block_index: usize,
    base_offset: usize,
    align_x: TextAlignment,
    wrapping: Wrapping,
    max_width: f32,
    preedit: Option<(usize, &str)>,
) -> ShapedText {
    let mut combined_plain = String::new();
    let tagged_runs: Vec<(Run, bool)> = if let Some((ins_off, pre_txt)) = preedit {
        build_preedit_runs(runs, ins_off, pre_txt)
    } else {
        runs.iter().cloned().map(|r| (r, false)).collect()
    };

    let mut owned_spans: Vec<Span<'static, Link, Font>> = Vec::new();
    let mut meta: Vec<SpanVisualMeta> = Vec::new();
    let mut offset = base_offset;

    for (run, is_preedit) in &tagged_runs {
        combined_plain.push_str(&run.text);
        let run_start = offset;
        let run_end = offset + run.text.len();
        let mut cuts = vec![run_start];
        let mut matches_here: Vec<(usize, &TextMatch)> = Vec::new();
        if preedit.is_none() {
            if let Some(view) = search {
                for (m_idx, found) in view.matches.iter().enumerate() {
                    if found.block == block_index && found.start < run_end && found.end > run_start
                    {
                        cuts.push(found.start.max(run_start));
                        cuts.push(found.end.min(run_end));
                        matches_here.push((m_idx, found));
                    }
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
            let txt = &run.text[start - run_start..end - run_start];
            if txt.is_empty() {
                continue;
            }
            let mut font = if run.code || code_block {
                mono_font
            } else {
                body_font
            };
            if force_bold || run.bold {
                font.weight = iced::font::Weight::Bold;
            } else if !run.code && !code_block {
                font.weight = iced::font::Weight::Medium;
            }
            if run.italic {
                font.style = iced::font::Style::Italic;
            }
            let color = if *is_preedit || run.link.is_some() {
                pal.accent
            } else if quote {
                pal.muted
            } else {
                pal.text
            };
            let mut highlight_bg = None;
            let mut highlight_border = Border::default();
            if run.code && !code_block {
                highlight_bg = Some(pal.code_bg);
                highlight_border = Border {
                    color: pal.border,
                    width: 1.0,
                    radius: 3.0.into(),
                };
            }
            if let Some((m_idx, _)) = matches_here
                .iter()
                .find(|(_, f)| f.start <= start && f.end >= end)
            {
                let current = search.and_then(|v| v.current) == Some(*m_idx);
                highlight_bg = Some(if current {
                    pal.highlight_current
                } else {
                    pal.highlight
                });
            }

            let mut sp = Span::new(txt.to_string())
                .size(size)
                .font(font)
                .color(color);
            if run.strike {
                sp = sp.strikethrough(true);
            }
            if let Some(link) = &run.link {
                sp = sp.link(link.clone()).underline(true);
            } else if *is_preedit {
                sp = sp.underline(true);
            }
            owned_spans.push(sp);
            meta.push(SpanVisualMeta {
                highlight_bg,
                highlight_border,
                padding: iced::Padding::ZERO,
                underline: run.link.is_some() || *is_preedit,
                strikethrough: run.strike,
                link: run.link.clone(),
                color,
                size,
                line_height: line_height_rel,
            });
        }
        offset = run_end;
    }

    if owned_spans.is_empty() {
        let mut font = if code_block { mono_font } else { body_font };
        if force_bold {
            font.weight = iced::font::Weight::Bold;
        }
        owned_spans.push(
            Span::new(String::from(" "))
                .size(size)
                .font(font)
                .color(pal.text),
        );
        meta.push(SpanVisualMeta {
            highlight_bg: None,
            highlight_border: Border::default(),
            padding: iced::Padding::ZERO,
            underline: false,
            strikethrough: false,
            link: None,
            color: pal.text,
            size,
            line_height: line_height_rel,
        });
    }

    let span_refs: Vec<Span<'_, Link, Font>> = owned_spans.iter().cloned().collect();
    let base_font = if code_block { mono_font } else { body_font };
    let paragraph = Paragraph::with_spans(iced::advanced::Text {
        content: span_refs.as_slice(),
        bounds: Size::new(max_width.max(12.0), 100_000.0),
        size: iced::Pixels(size),
        line_height: LineHeight::Relative(line_height_rel),
        font: base_font,
        align_x,
        align_y: iced::alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping,
    });

    let line_starts = compute_line_starts(&combined_plain);
    let plain_len = combined_plain.len();
    ShapedText {
        paragraph,
        meta,
        line_starts,
        plain_len,
        align_x,
    }
}

fn materialize_block(
    widget: &DocWidget<'_>,
    block_idx: usize,
    block: &Block,
    width: f32,
    preedit: Option<(usize, usize, &str)>,
) -> (BlockContent, f32) {
    match &block.kind {
        BlockKind::Rule => (BlockContent::Rule { y_offset: 10.0 }, 21.0),
        BlockKind::Image { alt, src } => {
            let cached = widget
                .images
                .get_or_load(src, widget.doc.path.as_deref(), alt);
            let max_w = (width - 8.0).max(120.0);
            let (disp_w, disp_h) = match &cached {
                CachedImage::Raster { width: iw, height: ih, .. }
                | CachedImage::Svg { width: iw, height: ih, .. } => {
                    let iw = (*iw).max(1) as f32;
                    let ih = (*ih).max(1) as f32;
                    let scale = (max_w / iw).min(1.0).min(MAX_IMAGE_HEIGHT / ih);
                    ((iw * scale).max(24.0), (ih * scale).max(24.0))
                }
                CachedImage::Placeholder(_) => (max_w.min(460.0), 72.0),
            };
            let draw_x = ((width - disp_w) * 0.5).max(0.0);
            let draw_rect = Rectangle::new(Point::new(draw_x, 6.0), Size::new(disp_w, disp_h));
            let caption_str = if !alt.is_empty() && !matches!(cached, CachedImage::Placeholder(_)) {
                Some(alt.clone())
            } else if widget.editing {
                Some(format!("![{}]({})", alt, src))
            } else {
                None
            };
            let mut total_h = 6.0 + disp_h + 6.0;
            let (caption, caption_pos, caption_text) = if let Some(cap_text) = caption_str {
                let cap_size = (widget.size * 0.85).max(12.0);
                let par = Paragraph::with_text(iced::advanced::Text {
                    content: cap_text.as_str(),
                    bounds: Size::new(max_w, 200.0),
                    size: iced::Pixels(cap_size),
                    line_height: LineHeight::Relative(1.35),
                    font: widget.body_font,
                    align_x: TextAlignment::Center,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: Shaping::Auto,
                    wrapping: Wrapping::WordOrGlyph,
                });
                let bounds_cap = par.min_bounds();
                let ch = bounds_cap.height.ceil();
                let cw = bounds_cap.width;
                let cy = 6.0 + disp_h + 6.0;
                total_h = cy + ch + 4.0;
                (Some(par), Point::new(((width - cw) * 0.5).max(0.0), cy), cap_text)
            } else {
                (None, Point::ORIGIN, String::new())
            };
            (
                BlockContent::Image {
                    image: cached,
                    draw_rect,
                    caption,
                    caption_pos,
                    caption_text,
                },
                total_h.ceil(),
            )
        }
        BlockKind::Table(table) => {
            let mat = materialize_table(widget, block_idx, table, width, preedit);
            let has_scroll = mat.total_w > width + 1.0;
            let extra = if has_scroll { SCROLLBAR_HEIGHT + 4.0 } else { 0.0 };
            let total_h = 4.0 + mat.total_h + 4.0 + extra;
            (BlockContent::Table(mat), total_h.ceil())
        }
        _ => {
            let size = block_font_size(widget.size, &block.kind);
            let line = block_line_height(widget.line, &block.kind);
            let (pad_l, pad_t, pad_r, pad_b) = block_insets(&block.kind);
            let force_bold = matches!(block.kind, BlockKind::Heading(_));
            let quote = matches!(block.kind, BlockKind::Quote);
            let code_block = matches!(block.kind, BlockKind::Code { .. });
            let is_task = matches!(block.kind, BlockKind::ListItem { checked: Some(_), .. });

            let mut marker_str = None;
            let mut marker_w = 0.0f32;
            let mut checkbox_checked = None;
            if let BlockKind::ListItem { marker, checked, .. } = &block.kind {
                if let Some(c) = checked {
                    checkbox_checked = Some(*c);
                    marker_w = (widget.size * 1.45).max(24.0);
                } else {
                    marker_str = Some(marker.clone());
                    let m_par = Paragraph::with_text(iced::advanced::Text {
                        content: marker.as_str(),
                        bounds: Size::new(80.0, size * 2.0),
                        size: iced::Pixels(size),
                        line_height: LineHeight::Relative(line),
                        font: widget.body_font,
                        align_x: TextAlignment::Left,
                        align_y: iced::alignment::Vertical::Top,
                        shaping: Shaping::Auto,
                        wrapping: Wrapping::None,
                    });
                    marker_w = m_par.min_bounds().width.ceil().max(18.0);
                }
            }

            let text_x = pad_l + marker_w;
            let avail_w = (width - text_x - pad_r).max(40.0);
            let pre = preedit.and_then(|(c, off, s)| (c == 0).then_some((off, s)));
            let shaped = shape_runs(
                &block.runs,
                size,
                line,
                widget.pal,
                widget.body_font,
                widget.mono_font,
                force_bold,
                quote,
                code_block,
                widget.search.as_ref(),
                block_idx,
                0,
                TextAlignment::Left,
                Wrapping::WordOrGlyph,
                avail_w,
                pre,
            );
            let par_h = shaped.paragraph.min_bounds().height.ceil().max(size * line);
            let total_h = (pad_t + par_h + pad_b).ceil();
            let text_rect = Rectangle::new(Point::new(text_x, pad_t), Size::new(avail_w, par_h));
            let checkbox_rect = if is_task {
                let box_sz = (size * 0.85).clamp(13.0, 17.0);
                let top_off = pad_t + ((size * line - box_sz) * 0.5).max(1.0);
                Some(Rectangle::new(
                    Point::new(pad_l + 1.0, top_off),
                    Size::new(box_sz, box_sz),
                ))
            } else {
                None
            };
            (
                BlockContent::Text {
                    shaped,
                    text_rect,
                    marker: marker_str,
                    marker_origin: Point::new(pad_l, pad_t),
                    checkbox_rect,
                    checked: checkbox_checked,
                },
                total_h,
            )
        }
    }
}

fn materialize_table(
    widget: &DocWidget<'_>,
    block_idx: usize,
    table: &Table,
    max_width: f32,
    preedit: Option<(usize, usize, &str)>,
) -> MaterializedTable {
    let rows = table.rows.max(1);
    let cols = table.cols.max(1);
    let size = widget.size;
    let table_line = (widget.line * 0.88).clamp(1.45, 1.72);
    let pad_x = (size * 0.72).clamp(10.0, 16.0);
    let pad_y = (size * 0.46).clamp(7.0, 12.0);
    let min_row_h = (size * table_line + pad_y * 2.0).ceil();
    let min_empty_w = (size * 2.0).max(28.0);

    let mut pref_w = vec![min_empty_w; cols];
    let mut header_w = vec![min_empty_w; cols];
    let mut unb_widths = Vec::with_capacity(table.cells.len());

    for (ci, cell) in table.cells.iter().enumerate() {
        let align_x = match cell.align {
            CellAlign::Left => TextAlignment::Left,
            CellAlign::Center => TextAlignment::Center,
            CellAlign::Right => TextAlignment::Right,
        };
        let pre = preedit.and_then(|(c, off, s)| (c == ci).then_some((off, s)));
        let unb = shape_runs(
            &cell.runs,
            size,
            table_line,
            widget.pal,
            widget.body_font,
            widget.mono_font,
            cell.header,
            false,
            false,
            None,
            block_idx,
            cell.plain_range.start,
            align_x,
            Wrapping::None,
            100_000.0,
            pre,
        );
        let need_w = unb.paragraph.min_bounds().width.ceil() + pad_x * 2.0 + 2.0;
        if cell.colspan == 1 && cell.col < cols {
            pref_w[cell.col] = pref_w[cell.col].max(need_w);
            if cell.header {
                header_w[cell.col] = header_w[cell.col].max(need_w);
            }
        }
        unb_widths.push(need_w);
    }

    for (ci, cell) in table.cells.iter().enumerate() {
        if cell.colspan > 1 && cell.col < cols {
            let end_c = (cell.col + cell.colspan).min(cols);
            let span_c = end_c - cell.col;
            let need = unb_widths[ci];
            let cur: f32 = pref_w[cell.col..end_c].iter().sum::<f32>()
                + span_c.saturating_sub(1) as f32;
            if need > cur {
                let extra = (need - cur) / span_c as f32;
                for w in &mut pref_w[cell.col..end_c] {
                    *w += extra;
                }
                if cell.header {
                    for w in &mut header_w[cell.col..end_c] {
                        *w += extra;
                    }
                }
            }
        }
    }

    let col_widths = allocate_column_widths(&pref_w, &header_w, max_width, size);
    let mut col_xs = Vec::with_capacity(cols);
    let mut x = 1.0f32;
    for &w in &col_widths {
        col_xs.push(x);
        x += w + 1.0;
    }
    let total_w = x;

    let mut row_heights = vec![min_row_h; rows];
    let mut row_is_header = vec![false; rows];
    let mut cell_shaped = Vec::with_capacity(table.cells.len());

    for (ci, cell) in table.cells.iter().enumerate() {
        if cell.row < rows && cell.header {
            row_is_header[cell.row] = true;
        }
        let end_c = (cell.col + cell.colspan).min(cols);
        let span_c = end_c.saturating_sub(cell.col).max(1);
        let box_w: f32 = col_widths[cell.col..end_c].iter().sum::<f32>()
            + span_c.saturating_sub(1) as f32;
        let inner_w = (box_w - pad_x * 2.0).max(12.0);
        let align_x = match cell.align {
            CellAlign::Left => TextAlignment::Left,
            CellAlign::Center => TextAlignment::Center,
            CellAlign::Right => TextAlignment::Right,
        };
        let pre = preedit.and_then(|(c, off, s)| (c == ci).then_some((off, s)));
        let shaped = shape_runs(
            &cell.runs,
            size,
            table_line,
            widget.pal,
            widget.body_font,
            widget.mono_font,
            cell.header,
            false,
            false,
            widget.search.as_ref(),
            block_idx,
            cell.plain_range.start,
            align_x,
            Wrapping::WordOrGlyph,
            inner_w,
            pre,
        );
        if cell.rowspan == 1 && cell.row < rows {
            let h = shaped.paragraph.min_bounds().height.ceil() + pad_y * 2.0;
            row_heights[cell.row] = row_heights[cell.row].max(h);
        }
        cell_shaped.push(shaped);
    }

    for (ci, cell) in table.cells.iter().enumerate() {
        if cell.rowspan > 1 && cell.row < rows {
            let end_r = (cell.row + cell.rowspan).min(rows);
            let span_r = end_r - cell.row;
            let need = cell_shaped[ci].paragraph.min_bounds().height.ceil() + pad_y * 2.0;
            let cur: f32 = row_heights[cell.row..end_r].iter().sum::<f32>()
                + span_r.saturating_sub(1) as f32;
            if need > cur {
                let extra = (need - cur) / span_r as f32;
                for h in &mut row_heights[cell.row..end_r] {
                    *h += extra;
                }
            }
        }
    }

    let mut row_ys = Vec::with_capacity(rows);
    let mut y = 1.0f32;
    for &h in &row_heights {
        row_ys.push(y);
        y += h + 1.0;
    }
    let total_h = y;

    let mut cells = Vec::with_capacity(table.cells.len());
    for (ci, (cell, shaped)) in table.cells.iter().zip(cell_shaped).enumerate() {
        if cell.col >= cols || cell.row >= rows {
            continue;
        }
        let end_c = (cell.col + cell.colspan).min(cols);
        let end_r = (cell.row + cell.rowspan).min(rows);
        let span_c = end_c.saturating_sub(cell.col).max(1);
        let span_r = end_r.saturating_sub(cell.row).max(1);
        let cx = col_xs[cell.col];
        let cy = row_ys[cell.row];
        let box_w: f32 = col_widths[cell.col..end_c].iter().sum::<f32>()
            + span_c.saturating_sub(1) as f32;
        let box_h: f32 = row_heights[cell.row..end_r].iter().sum::<f32>()
            + span_r.saturating_sub(1) as f32;
        let inner_w = (box_w - pad_x * 2.0).max(1.0);
        let inner_h = (box_h - pad_y * 2.0).max(1.0);
        let par_bounds = shaped.paragraph.min_bounds();
        let par_w = par_bounds.width;
        let par_h = par_bounds.height;
        let text_x = match cell.align {
            CellAlign::Center => cx + pad_x + ((inner_w - par_w) * 0.5).max(0.0),
            CellAlign::Right => cx + pad_x + (inner_w - par_w).max(0.0),
            CellAlign::Left => cx + pad_x,
        };
        let text_y = if cell.header || cell.rowspan > 1 {
            cy + pad_y + ((inner_h - par_h) * 0.5).max(0.0)
        } else {
            cy + pad_y
        };
        cells.push(MaterializedCell {
            cell_idx: ci,
            row: cell.row,
            col: cell.col,
            colspan: cell.colspan,
            rowspan: cell.rowspan,
            header: cell.header,
            box_rect: Rectangle::new(Point::new(cx, cy), Size::new(box_w, box_h)),
            text_origin: Point::new(text_x, text_y),
            shaped,
        });
    }

    MaterializedTable {
        rows,
        cols,
        total_w,
        total_h,
        col_xs,
        col_widths,
        row_ys,
        row_heights,
        row_is_header,
        has_rowspan: table.has_rowspan,
        cells,
    }
}

fn draw_active_block_cue(
    renderer: &mut iced::Renderer,
    rect: Rectangle,
    pal: Palette,
    kind: &BlockKind,
) {
    use iced::advanced::Renderer as _;
    let accent = Color { a: 0.35, ..pal.accent };
    if matches!(kind, BlockKind::Quote) {
        return;
    }
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle::new(
                Point::new(rect.x - 10.0, rect.y + 2.0),
                Size::new(2.0, (rect.height - 4.0).max(8.0)),
            ),
            border: border::rounded(1.0),
            ..Default::default()
        },
        accent,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_block_chrome(
    renderer: &mut iced::Renderer,
    block: &Block,
    rect: Rectangle,
    pal: Palette,
    active_edit: bool,
    mono_font: Font,
    base_size: f32,
    viewport: &Rectangle,
) {
    use iced::advanced::Renderer as _;
    match &block.kind {
        BlockKind::Code { lang } => {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    border: Border {
                        color: if active_edit { pal.accent } else { pal.border },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    snap: true,
                    ..Default::default()
                },
                pal.code_bg,
            );
            if !lang.is_empty() || active_edit {
                let tag = if lang.is_empty() { "code" } else { lang.as_str() };
                let tag_size = (base_size * 0.72).clamp(10.0, 12.0);
                let par = Paragraph::with_text(iced::advanced::Text {
                    content: tag,
                    bounds: Size::new(140.0, 24.0),
                    size: iced::Pixels(tag_size),
                    line_height: LineHeight::Relative(1.2),
                    font: mono_font,
                    align_x: TextAlignment::Right,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: Shaping::Auto,
                    wrapping: Wrapping::None,
                });
                let tw = par.min_bounds().width;
                use iced::advanced::text::Renderer as _;
                renderer.fill_paragraph(
                    &par,
                    Point::new(rect.x + rect.width - tw - 10.0, rect.y + 5.0),
                    pal.muted,
                    *viewport,
                );
            }
        }
        BlockKind::Quote => {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(rect.x + 2.0, rect.y + 2.0),
                        Size::new(3.0, (rect.height - 4.0).max(8.0)),
                    ),
                    border: border::rounded(1.5),
                    ..Default::default()
                },
                if active_edit { pal.accent } else { pal.border },
            );
        }
        BlockKind::Heading(level) if active_edit => {
            let label = format!("H{}", level);
            let par = Paragraph::with_text(iced::advanced::Text {
                content: label.as_str(),
                bounds: Size::new(30.0, 20.0),
                size: iced::Pixels(10.5),
                line_height: LineHeight::Relative(1.2),
                font: mono_font,
                align_x: TextAlignment::Right,
                align_y: iced::alignment::Vertical::Top,
                shaping: Shaping::Auto,
                wrapping: Wrapping::None,
            });
            use iced::advanced::text::Renderer as _;
            renderer.fill_paragraph(
                &par,
                Point::new(rect.x - 36.0, rect.y + 12.0),
                pal.muted,
                *viewport,
            );
        }
        _ => {}
    }
}

fn draw_checkbox(renderer: &mut iced::Renderer, rect: Rectangle, checked: bool, pal: Palette) {
    use iced::advanced::Renderer as _;
    renderer.fill_quad(
        renderer::Quad {
            bounds: rect,
            border: Border {
                color: if checked { pal.accent } else { pal.muted },
                width: 1.4,
                radius: 3.0.into(),
            },
            snap: true,
            ..Default::default()
        },
        if checked { pal.accent } else { pal.bg },
    );
    if checked {
        let inset = rect.width * 0.28;
        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle::new(
                    Point::new(rect.x + inset, rect.y + inset),
                    Size::new(rect.width - inset * 2.0, rect.height - inset * 2.0),
                ),
                border: border::rounded(1.5),
                ..Default::default()
            },
            pal.bg,
        );
    }
}

fn draw_cached_image(
    renderer: &mut iced::Renderer,
    image: &CachedImage,
    rect: Rectangle,
    pal: Palette,
    viewport: &Rectangle,
) {
    let Some(clip) = rect.intersection(viewport) else {
        return;
    };
    use iced::advanced::Renderer as _;
    match image {
        CachedImage::Raster { handle, .. } => {
        renderer.fill_quad(
            renderer::Quad {
                bounds: rect,
                border: Border {
                    color: pal.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                snap: true,
                ..Default::default()
            },
            pal.panel,
        );
        use iced::advanced::image::Renderer as _;
        let mut img = core_image::Image::new(handle.clone());
        img.border_radius = 4.0.into();
        renderer.draw_image(img, rect, clip);
        }
        CachedImage::Svg { handle, .. } => {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    border: Border {
                        color: pal.border,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    snap: true,
                    ..Default::default()
                },
                pal.panel,
            );
            use iced::advanced::svg::Renderer as _;
            renderer.draw_svg(core_svg::Svg::new(handle.clone()), rect, clip);
        }
        CachedImage::Placeholder(label) => {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    border: Border {
                        color: pal.border,
                        width: 1.0,
                        radius: 6.0.into(),
                    },
                    snap: true,
                    ..Default::default()
                },
                pal.panel,
            );
            let par = Paragraph::with_text(iced::advanced::Text {
                content: label.as_str(),
                bounds: Size::new((rect.width - 20.0).max(40.0), rect.height),
                size: iced::Pixels(13.5),
                line_height: LineHeight::Relative(1.4),
                font: Font::DEFAULT,
                align_x: TextAlignment::Center,
                align_y: iced::alignment::Vertical::Top,
                shaping: Shaping::Auto,
                wrapping: Wrapping::WordOrGlyph,
            });
            let ph = par.min_bounds().height;
            let py = rect.y + ((rect.height - ph) * 0.5).max(8.0);
            use iced::advanced::text::Renderer as _;
            renderer.fill_paragraph(&par, Point::new(rect.x + 10.0, py), pal.muted, clip);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_shaped_text(
    renderer: &mut iced::Renderer,
    shaped: &ShapedText,
    origin: Point,
    pal: Palette,
    defaults: &renderer::Style,
    selection_bytes: Option<(usize, usize)>,
    hovered_span: Option<usize>,
    viewport: &Rectangle,
) {
    use iced::advanced::Renderer as _;
    let translation = origin - Point::ORIGIN;

    for (idx, meta) in shaped.meta.iter().enumerate() {
        let hovered = hovered_span == Some(idx);
        if meta.highlight_bg.is_none() && !meta.underline && !meta.strikethrough && !hovered {
            continue;
        }
        let regions = shaped.paragraph.span_bounds(idx);
        if let Some(bg) = meta.highlight_bg {
            for r in &regions {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            r.position() + translation - Vector::new(meta.padding.left, meta.padding.top),
                            r.size() + Size::new(meta.padding.x(), meta.padding.y()),
                        ),
                        border: meta.highlight_border,
                        ..Default::default()
                    },
                    bg,
                );
            }
        }
        if meta.underline || meta.strikethrough || hovered {
            let line_px = meta.size * meta.line_height;
            let base_y = meta.size + (line_px - meta.size) * 0.5;
            if meta.underline || hovered {
                for r in &regions {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                r.position() + translation + Vector::new(0.0, base_y - meta.size * 0.08),
                                Size::new(r.width, 1.0),
                            ),
                            ..Default::default()
                        },
                        meta.color,
                    );
                }
            }
            if meta.strikethrough {
                for r in &regions {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                r.position() + translation + Vector::new(0.0, base_y - meta.size * 0.5),
                                Size::new(r.width, 1.0),
                            ),
                            ..Default::default()
                        },
                        meta.color,
                    );
                }
            }
        }
    }

    if let Some((from, to)) = selection_bytes {
        let rects = selection_rects(shaped, from, to);
        for r in rects {
            let abs = Rectangle::new(r.position() + translation, r.size());
            if let Some(clipped) = abs.intersection(viewport) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: clipped,
                        ..Default::default()
                    },
                    pal.selection,
                );
            }
        }
    }

    use iced::advanced::text::Renderer as _;
    let fallback_color = if shaped.meta.first().is_some() {
        shaped.meta[0].color
    } else {
        defaults.text_color
    };
    renderer.fill_paragraph(&shaped.paragraph, origin, fallback_color, *viewport);
}

#[allow(clippy::too_many_arguments)]
fn draw_materialized_table(
    renderer: &mut iced::Renderer,
    table: &MaterializedTable,
    origin: Point,
    clip_rect: Rectangle,
    tp: TablePalette,
    pal: Palette,
    defaults: &renderer::Style,
    selection: Option<(DocPos, DocPos)>,
    block_idx: usize,
    hovered_row: Option<usize>,
    active_cell: Option<usize>,
    caret_offset: Option<usize>,
    hovered_link: Option<(UnitId, usize)>,
    _state: &DocWidgetState,
) {
    use iced::advanced::Renderer as _;
    renderer.start_layer(clip_rect);

    let table_bounds = Rectangle::new(origin, Size::new(table.total_w, table.total_h));
    renderer.fill_quad(
        renderer::Quad {
            bounds: table_bounds,
            border: border::rounded(4.0),
            snap: true,
            ..Default::default()
        },
        tp.bg,
    );

    let mut body_row_seq = 0usize;
    let mut row_bg_index = vec![0usize; table.rows];
    for r in 0..table.rows {
        if !table.row_is_header[r] {
            row_bg_index[r] = body_row_seq;
            body_row_seq += 1;
        }
    }

    for cell in &table.cells {
        let abs_box = Rectangle::new(
            Point::new(origin.x + cell.box_rect.x, origin.y + cell.box_rect.y),
            cell.box_rect.size(),
        );
        let is_hovered = !cell.header && cell.rowspan == 1 && hovered_row == Some(cell.row);
        let bg = if cell.header {
            Some(tp.header_bg)
        } else if is_hovered {
            Some(tp.hover_bg)
        } else if tp.zebra && row_bg_index[cell.row] % 2 == 1 {
            Some(tp.zebra_bg)
        } else {
            None
        };
        if let Some(bg) = bg {
            let end_c = (cell.col + cell.colspan).min(table.cols);
            let end_r = (cell.row + cell.rowspan).min(table.rows);
            renderer.fill_quad(
                renderer::Quad {
                    bounds: abs_box,
                    border: Border {
                        radius: border::Radius {
                            top_left: if cell.row == 0 && cell.col == 0 { 3.0 } else { 0.0 },
                            top_right: if cell.row == 0 && end_c == table.cols { 3.0 } else { 0.0 },
                            bottom_right: if end_r == table.rows && end_c == table.cols { 3.0 } else { 0.0 },
                            bottom_left: if end_r == table.rows && cell.col == 0 { 3.0 } else { 0.0 },
                        },
                        ..Default::default()
                    },
                    snap: true,
                    ..Default::default()
                },
                bg,
            );
        }
    }

    let mut spans_v = vec![vec![false; table.cols]; table.rows.saturating_sub(1)];
    let mut spans_h = vec![vec![false; table.cols.saturating_sub(1)]; table.rows];
    for cell in &table.cells {
        let end_c = (cell.col + cell.colspan).min(table.cols);
        let end_r = (cell.row + cell.rowspan).min(table.rows);
        for r in cell.row..end_r.saturating_sub(1) {
            for c in cell.col..end_c {
                spans_v[r][c] = true;
            }
        }
        for r in cell.row..end_r {
            for c in cell.col..end_c.saturating_sub(1) {
                spans_h[r][c] = true;
            }
        }
    }

    for r in 0..table.rows.saturating_sub(1) {
        let line_y = origin.y + table.row_ys[r] + table.row_heights[r];
        let is_hdr_sep = table.row_is_header[r] && !table.row_is_header[r + 1];
        let thickness = if is_hdr_sep { 1.5 } else { 1.0 };
        let color = if is_hdr_sep { tp.header_border } else { tp.border };
        for c in 0..table.cols {
            if spans_v[r][c] {
                continue;
            }
            let sx = if c == 0 { origin.x + 1.0 } else { origin.x + table.col_xs[c] - 1.0 };
            let ex = if c + 1 == table.cols {
                origin.x + table.total_w - 1.0
            } else {
                origin.x + table.col_xs[c] + table.col_widths[c] + 1.0
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(sx, line_y),
                        Size::new((ex - sx).max(0.0), thickness),
                    ),
                    snap: true,
                    ..Default::default()
                },
                color,
            );
        }
    }

    for c in 0..table.cols.saturating_sub(1) {
        let line_x = origin.x + table.col_xs[c] + table.col_widths[c];
        for r in 0..table.rows {
            if spans_h[r][c] {
                continue;
            }
            let sy = if r == 0 { origin.y + 1.0 } else { origin.y + table.row_ys[r] - 1.0 };
            let ey = if r + 1 == table.rows {
                origin.y + table.total_h - 1.0
            } else {
                origin.y + table.row_ys[r] + table.row_heights[r] + 1.0
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(
                        Point::new(line_x, sy),
                        Size::new(1.0, (ey - sy).max(0.0)),
                    ),
                    snap: true,
                    ..Default::default()
                },
                tp.border,
            );
        }
    }

    renderer.fill_quad(
        renderer::Quad {
            bounds: table_bounds,
            border: Border {
                color: tp.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            snap: true,
            ..Default::default()
        },
        Color::TRANSPARENT,
    );

    for cell in &table.cells {
        let cell_origin = Point::new(origin.x + cell.text_origin.x, origin.y + cell.text_origin.y);
        let cell_sel = selection_for_table_cell(selection, block_idx, cell.cell_idx, cell.shaped.plain_len);
        let h_span = hovered_link.and_then(|(u, s)| {
            (u.block == block_idx && u.cell == cell.cell_idx).then_some(s)
        });
        draw_shaped_text(
            renderer,
            &cell.shaped,
            cell_origin,
            pal,
            defaults,
            cell_sel,
            h_span,
            &clip_rect,
        );

        if active_cell == Some(cell.cell_idx) {
            let abs_box = Rectangle::new(
                Point::new(origin.x + cell.box_rect.x, origin.y + cell.box_rect.y),
                cell.box_rect.size(),
            );
            renderer.fill_quad(
                renderer::Quad {
                    bounds: abs_box,
                    border: Border {
                        color: pal.accent,
                        width: 1.6,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                },
                Color::TRANSPARENT,
            );
            if let Some(c_off) = caret_offset {
                draw_caret_in_shaped(renderer, &cell.shaped, cell_origin, c_off, pal.accent);
            }
        }
    }

    renderer.end_layer();
}

fn draw_table_scrollbar(
    renderer: &mut iced::Renderer,
    origin: Point,
    view_w: f32,
    total_w: f32,
    scroll_x: f32,
    pal: Palette,
    dragging: bool,
) {
    use iced::advanced::Renderer as _;
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle::new(origin, Size::new(view_w, 7.0)),
            border: border::rounded(3.5),
            ..Default::default()
        },
        pal.panel,
    );
    let max_s = (total_w - view_w).max(1.0);
    let thumb_w = (view_w / total_w * view_w).clamp(28.0, view_w);
    let travel = (view_w - thumb_w).max(0.0);
    let thumb_x = (scroll_x / max_s).clamp(0.0, 1.0) * travel;
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle::new(Point::new(origin.x + thumb_x, origin.y), Size::new(thumb_w, 7.0)),
            border: border::rounded(3.5),
            ..Default::default()
        },
        if dragging { pal.accent } else { pal.muted },
    );
}

fn draw_caret_in_shaped(
    renderer: &mut iced::Renderer,
    shaped: &ShapedText,
    origin: Point,
    offset: usize,
    color: Color,
) {
    use iced::advanced::Renderer as _;
    let (cx, cy, ch) = caret_local_rect(shaped, offset);
    renderer.fill_quad(
        renderer::Quad {
            bounds: Rectangle::new(Point::new(origin.x + cx, origin.y + cy), Size::new(2.0, ch)),
            snap: true,
            ..Default::default()
        },
        color,
    );
}

fn caret_local_rect(shaped: &ShapedText, offset: usize) -> (f32, f32, f32) {
    let buffer = shaped.paragraph.buffer();
    let line_height = buffer.metrics().line_height;
    let clamped = offset.min(shaped.plain_len);
    let line_idx = shaped
        .line_starts
        .partition_point(|&s| s <= clamped)
        .saturating_sub(1)
        .min(buffer.lines.len().saturating_sub(1));
    let line_start = shaped.line_starts.get(line_idx).copied().unwrap_or(0);
    let local_byte = clamped.saturating_sub(line_start);

    let runs: Vec<cosmic_text::LayoutRun<'_>> = buffer
        .layout_runs()
        .filter(|r| r.line_i == line_idx)
        .collect();

    if runs.is_empty() {
        let w = shaped.paragraph.bounds().width;
        let ax = aligned_empty_x(shaped.align_x, w);
        return (ax, line_idx as f32 * line_height, line_height);
    }

    for (ri, run) in runs.iter().enumerate() {
        let y = run.line_top;
        let h = run.line_height;
        if run.glyphs.is_empty() {
            if ri + 1 == runs.len() {
                let ax = aligned_empty_x(shaped.align_x, shaped.paragraph.bounds().width);
                return (ax, y, h);
            }
            continue;
        }
        let first_start = run.glyphs.iter().map(|g| g.start).min().unwrap_or(0);
        let last_end = run.glyphs.iter().map(|g| g.end).max().unwrap_or(0);
        let is_last_run = ri + 1 == runs.len();
        let next_start = runs
            .get(ri + 1)
            .and_then(|nr| nr.glyphs.iter().map(|g| g.start).min())
            .unwrap_or(usize::MAX);

        if (local_byte >= first_start && local_byte < last_end)
            || (is_last_run && local_byte >= last_end)
            || (!is_last_run && local_byte == last_end && local_byte < next_start)
        {
            if let Some(g) = run
                .glyphs
                .iter()
                .find(|g| local_byte >= g.start && local_byte <= g.end)
            {
                let span = g.end.saturating_sub(g.start).max(1);
                let frac = (local_byte.saturating_sub(g.start)) as f32 / span as f32;
                let gx = if local_byte >= g.end {
                    g.x + g.w
                } else {
                    g.x + g.w * frac
                };
                return (gx, y, h);
            }
            let last_g = &run.glyphs[run.glyphs.len() - 1];
            return (last_g.x + last_g.w, y, h);
        }
    }

    let last = &runs[runs.len() - 1];
    if let Some(g) = last.glyphs.last() {
        (g.x + g.w, last.line_top, last.line_height)
    } else {
        (
            aligned_empty_x(shaped.align_x, shaped.paragraph.bounds().width),
            last.line_top,
            last.line_height,
        )
    }
}

fn aligned_empty_x(align_x: TextAlignment, width: f32) -> f32 {
    match align_x {
        TextAlignment::Center => width * 0.5,
        TextAlignment::Right => width.saturating_sub_f32(2.0),
        _ => 0.0,
    }
}

trait SatF32 {
    fn saturating_sub_f32(self, rhs: f32) -> f32;
}
impl SatF32 for f32 {
    fn saturating_sub_f32(self, rhs: f32) -> f32 {
        (self - rhs).max(0.0)
    }
}

fn selection_rects(shaped: &ShapedText, from: usize, to: usize) -> Vec<Rectangle> {
    if from >= to {
        return Vec::new();
    }
    let buffer = shaped.paragraph.buffer();
    let mut rects = Vec::new();
    for run in buffer.layout_runs() {
        let line_start = shaped.line_starts.get(run.line_i).copied().unwrap_or(0);
        if run.glyphs.is_empty() {
            if line_start >= from && line_start < to {
                rects.push(Rectangle::new(
                    Point::new(0.0, run.line_top),
                    Size::new(7.0, run.line_height),
                ));
            }
            continue;
        }
        let mut run_x0: Option<f32> = None;
        let mut run_x1 = 0.0f32;
        for g in run.glyphs {
            let gs = line_start + g.start;
            let ge = line_start + g.end;
            let ov_s = gs.max(from);
            let ov_e = ge.min(to);
            if ov_s < ov_e {
                let len = (ge - gs).max(1) as f32;
                let x0 = g.x + g.w * ((ov_s - gs) as f32 / len);
                let x1 = g.x + g.w * ((ov_e - gs) as f32 / len);
                run_x0 = Some(match run_x0 {
                    Some(prev) => prev.min(x0),
                    None => x0,
                });
                run_x1 = run_x1.max(x1);
            }
        }
        if let Some(x0) = run_x0 {
            let w = (run_x1 - x0).max(2.0);
            rects.push(Rectangle::new(
                Point::new(x0, run.line_top),
                Size::new(w, run.line_height),
            ));
        }
    }
    rects
}

fn selection_for_unit(
    selection: Option<(DocPos, DocPos)>,
    block_idx: usize,
    cell_idx: usize,
    plain_len: usize,
    block: &Block,
) -> Option<(usize, usize)> {
    let _ = block;
    let (s, e) = selection?;
    if block_idx < s.block || block_idx > e.block {
        return None;
    }
    let from = if block_idx == s.block && cell_idx == s.cell {
        s.offset
    } else if block_idx == s.block && cell_idx < s.cell {
        return None;
    } else {
        0
    };
    let to = if block_idx == e.block && cell_idx == e.cell {
        e.offset
    } else if block_idx == e.block && cell_idx > e.cell {
        return None;
    } else {
        plain_len
    };
    if from < to {
        Some((from.min(plain_len), to.min(plain_len)))
    } else if plain_len == 0 && s.block < block_idx && e.block > block_idx {
        Some((0, 1))
    } else {
        None
    }
}

fn selection_for_table_cell(
    selection: Option<(DocPos, DocPos)>,
    block_idx: usize,
    cell_idx: usize,
    plain_len: usize,
) -> Option<(usize, usize)> {
    let (s, e) = selection?;
    if block_idx < s.block || block_idx > e.block {
        return None;
    }
    if block_idx == s.block && block_idx == e.block {
        if cell_idx < s.cell || cell_idx > e.cell {
            return None;
        }
        let from = if cell_idx == s.cell { s.offset } else { 0 };
        let to = if cell_idx == e.cell { e.offset } else { plain_len };
        return (from < to).then_some((from.min(plain_len), to.min(plain_len)));
    }
    let from = if block_idx == s.block {
        if cell_idx < s.cell {
            return None;
        } else if cell_idx == s.cell {
            s.offset
        } else {
            0
        }
    } else {
        0
    };
    let to = if block_idx == e.block {
        if cell_idx > e.cell {
            return None;
        } else if cell_idx == e.cell {
            e.offset
        } else {
            plain_len
        }
    } else {
        plain_len
    };
    (from < to).then_some((from.min(plain_len), to.min(plain_len)))
}

fn hit_shaped_offset(shaped: &ShapedText, rel: Point, plain: &str) -> usize {
    let buffer = shaped.paragraph.buffer();
    let max_h = shaped.paragraph.min_bounds().height.max(1.0);
    let clamped_y = rel.y.clamp(0.0, (max_h - 0.5).max(0.0));
    if let Some(cursor) = buffer.hit(rel.x.max(0.0), clamped_y) {
        let base = shaped.line_starts.get(cursor.line).copied().unwrap_or(0);
        let line_len = buffer
            .lines
            .get(cursor.line)
            .map(|l| l.text().len())
            .unwrap_or(0);
        let off = (base + cursor.index.min(line_len)).min(plain.len());
        snap_to_char_boundary(plain, off)
    } else if rel.y <= 0.0 {
        0
    } else {
        plain.len()
    }
}

fn snap_to_char_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn hit_doc_pos(
    state: &DocWidgetState,
    widget: &DocWidget<'_>,
    local: Point,
) -> Option<DocPos> {
    if state.blocks.is_empty() {
        return None;
    }
    let y = local.y.max(0.0);
    let idx = state
        .prefix_ys
        .partition_point(|&py| py <= y)
        .saturating_sub(1)
        .min(state.blocks.len().saturating_sub(1));
    let block = widget.doc.blocks.get(idx)?;
    let cached = state.blocks.get(idx)?;
    let block_y = state.prefix_ys[idx];
    let rel_y = y - block_y;

    match &cached.material {
        Some(BlockContent::Text { shaped, text_rect, .. }) => {
            let rel = Point::new(local.x - text_rect.x, rel_y - text_rect.y);
            let off = hit_shaped_offset(shaped, rel, &block.plain);
            Some(DocPos::new(idx, 0, off))
        }
        Some(BlockContent::Table(table_mat)) => {
            let scroll_x = state.table_scroll_x.get(&idx).copied().unwrap_or(0.0);
            let tx = local.x + scroll_x;
            let ty = (rel_y - 4.0).max(0.0);
            let target_cell = table_mat
                .cells
                .iter()
                .find(|c| {
                    tx >= c.box_rect.x - 1.0
                        && tx <= c.box_rect.x + c.box_rect.width + 1.0
                        && ty >= c.box_rect.y - 1.0
                        && ty <= c.box_rect.y + c.box_rect.height + 1.0
                })
                .or_else(|| {
                    table_mat.cells.iter().min_by_key(|c| {
                        let dx = if tx < c.box_rect.x {
                            c.box_rect.x - tx
                        } else if tx > c.box_rect.x + c.box_rect.width {
                            tx - (c.box_rect.x + c.box_rect.width)
                        } else {
                            0.0
                        };
                        let dy = if ty < c.box_rect.y {
                            c.box_rect.y - ty
                        } else if ty > c.box_rect.y + c.box_rect.height {
                            ty - (c.box_rect.y + c.box_rect.height)
                        } else {
                            0.0
                        };
                        ((dx + dy * 4.0) * 10.0) as i64
                    })
                })?;
            let cell_plain = match &block.kind {
                BlockKind::Table(t) => t
                    .cells
                    .get(target_cell.cell_idx)
                    .map(|c| c.plain.as_str())
                    .unwrap_or(""),
                _ => "",
            };
            let rel = Point::new(tx - target_cell.text_origin.x, ty - target_cell.text_origin.y);
            let off = hit_shaped_offset(&target_cell.shaped, rel, cell_plain);
            Some(DocPos::new(idx, target_cell.cell_idx, off))
        }
        _ => Some(DocPos::new(idx, 0, 0)),
    }
}

fn table_under_point<'a>(
    state: &'a DocWidgetState,
    local: Point,
) -> Option<(usize, &'a MaterializedTable)> {
    let idx = state
        .prefix_ys
        .partition_point(|&py| py <= local.y.max(0.0))
        .saturating_sub(1)
        .min(state.blocks.len().saturating_sub(1));
    let cached = state.blocks.get(idx)?;
    if let Some(BlockContent::Table(t)) = &cached.material {
        let block_y = state.prefix_ys[idx];
        if local.y >= block_y && local.y <= block_y + cached.height {
            return Some((idx, t));
        }
    }
    None
}

fn hit_table_row(state: &DocWidgetState, local: Point) -> Option<(usize, usize)> {
    let (block_idx, table) = table_under_point(state, local)?;
    let scroll_x = state.table_scroll_x.get(&block_idx).copied().unwrap_or(0.0);
    let tx = local.x + scroll_x;
    let ty = local.y - state.prefix_ys[block_idx] - 4.0;
    if tx < 0.0 || tx > table.total_w || ty < 0.0 || ty > table.total_h {
        return None;
    }
    for (r, (ry, rh)) in table.row_ys.iter().zip(&table.row_heights).enumerate() {
        if !table.row_is_header.get(r).copied().unwrap_or(false)
            && ty >= *ry
            && ty < *ry + *rh
        {
            return Some((block_idx, r));
        }
    }
    None
}

fn hit_table_scrollbar(
    state: &DocWidgetState,
    widget: &DocWidget<'_>,
    local: Point,
) -> Option<(usize, f32, f32)> {
    let idx = state
        .prefix_ys
        .partition_point(|&py| py <= local.y.max(0.0))
        .saturating_sub(1)
        .min(state.blocks.len().saturating_sub(1));
    let cached = state.blocks.get(idx)?;
    let Some(BlockContent::Table(t)) = &cached.material else {
        return None;
    };
    if t.total_w <= widget.content_width + 1.0 {
        return None;
    }
    let bar_y = state.prefix_ys[idx] + 4.0 + t.total_h;
    if local.y >= bar_y && local.y <= bar_y + SCROLLBAR_HEIGHT + 4.0 {
        let scroll_x = state.table_scroll_x.get(&idx).copied().unwrap_or(0.0);
        return Some((idx, t.total_w, scroll_x));
    }
    None
}

fn hit_checkbox(
    state: &DocWidgetState,
    _widget: &DocWidget<'_>,
    local: Point,
) -> Option<usize> {
    let idx = state
        .prefix_ys
        .partition_point(|&py| py <= local.y.max(0.0))
        .saturating_sub(1)
        .min(state.blocks.len().saturating_sub(1));
    let cached = state.blocks.get(idx)?;
    if let Some(BlockContent::Text {
        checkbox_rect: Some(cb),
        ..
    }) = &cached.material
    {
        let block_y = state.prefix_ys[idx];
        let abs_cb = Rectangle::new(Point::new(cb.x - 4.0, block_y + cb.y - 4.0), Size::new(cb.width + 8.0, cb.height + 8.0));
        if abs_cb.contains(local) {
            return Some(idx);
        }
    }
    None
}

fn hit_link_at(state: &DocWidgetState, local: Point) -> Option<(UnitId, usize)> {
    let idx = state
        .prefix_ys
        .partition_point(|&py| py <= local.y.max(0.0))
        .saturating_sub(1)
        .min(state.blocks.len().saturating_sub(1));
    let cached = state.blocks.get(idx)?;
    let block_y = state.prefix_ys[idx];
    let rel_y = local.y - block_y;
    match &cached.material {
        Some(BlockContent::Text { shaped, text_rect, .. }) => {
            let rel = Point::new(local.x - text_rect.x, rel_y - text_rect.y);
            if rel.x < 0.0 || rel.y < 0.0 || rel.x > text_rect.width || rel.y > text_rect.height {
                return None;
            }
            let span_idx = shaped.paragraph.hit_span(rel)?;
            shaped.meta.get(span_idx)?.link.as_ref()?;
            Some((UnitId { block: idx, cell: 0 }, span_idx))
        }
        Some(BlockContent::Table(table_mat)) => {
            let scroll_x = state.table_scroll_x.get(&idx).copied().unwrap_or(0.0);
            let tx = local.x + scroll_x;
            let ty = rel_y - 4.0;
            let cell = table_mat.cells.iter().find(|c| {
                tx >= c.box_rect.x
                    && tx <= c.box_rect.x + c.box_rect.width
                    && ty >= c.box_rect.y
                    && ty <= c.box_rect.y + c.box_rect.height
            })?;
            let rel = Point::new(tx - cell.text_origin.x, ty - cell.text_origin.y);
            let span_idx = cell.shaped.paragraph.hit_span(rel)?;
            cell.shaped.meta.get(span_idx)?.link.as_ref()?;
            Some((
                UnitId {
                    block: idx,
                    cell: cell.cell_idx,
                },
                span_idx,
            ))
        }
        _ => None,
    }
}

fn extract_link(state: &DocWidgetState, unit: UnitId, span_idx: usize) -> Option<Link> {
    let cached = state.blocks.get(unit.block)?;
    match &cached.material {
        Some(BlockContent::Text { shaped, .. }) => shaped.meta.get(span_idx)?.link.clone(),
        Some(BlockContent::Table(table_mat)) => table_mat
            .cells
            .iter()
            .find(|c| c.cell_idx == unit.cell)?
            .shaped
            .meta
            .get(span_idx)?
            .link
            .clone(),
        _ => None,
    }
}

fn word_bounds_at(doc: &Doc, pos: DocPos) -> (DocPos, DocPos) {
    let plain = unit_plain(doc, pos);
    if plain.is_empty() {
        return (pos, pos);
    }
    let off = pos.offset.min(plain.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let target = plain[off..]
        .chars()
        .next()
        .or_else(|| plain[..off].chars().next_back())
        .is_some_and(is_word);
    let mut start = off;
    while start > 0 {
        let prev = prev_cluster_boundary(plain, start);
        let ch = plain[prev..start].chars().next();
        if ch.is_some_and(is_word) != target {
            break;
        }
        start = prev;
    }
    let mut end = off;
    while end < plain.len() {
        let next = next_cluster_boundary(plain, end);
        let ch = plain[end..next].chars().next();
        if ch.is_some_and(is_word) != target {
            break;
        }
        end = next;
    }
    (
        DocPos::new(pos.block, pos.cell, start),
        DocPos::new(pos.block, pos.cell, end),
    )
}

fn caret_screen_rect(
    state: &DocWidgetState,
    widget: &DocWidget<'_>,
    doc_origin: Point,
) -> Option<Rectangle> {
    let pos = state.active_caret(widget)?;
    let pre_len = state
        .preedit
        .as_ref()
        .map(|p| p.content.len())
        .unwrap_or(0);
    let cached = state.blocks.get(pos.block)?;
    let block_y = doc_origin.y + state.prefix_ys[pos.block];
    match &cached.material {
        Some(BlockContent::Text { shaped, text_rect, .. }) => {
            let (cx, cy, ch) = caret_local_rect(shaped, pos.offset + pre_len);
            Some(Rectangle::new(
                Point::new(doc_origin.x + text_rect.x + cx, block_y + text_rect.y + cy),
                Size::new(2.0, ch),
            ))
        }
        Some(BlockContent::Table(table_mat)) => {
            let cell = table_mat.cells.iter().find(|c| c.cell_idx == pos.cell)?;
            let scroll_x = state.table_scroll_x.get(&pos.block).copied().unwrap_or(0.0);
            let (cx, cy, ch) = caret_local_rect(&cell.shaped, pos.offset + pre_len);
            Some(Rectangle::new(
                Point::new(
                    doc_origin.x - scroll_x + cell.text_origin.x + cx,
                    block_y + 4.0 + cell.text_origin.y + cy,
                ),
                Size::new(2.0, ch),
            ))
        }
        _ => Some(Rectangle::new(
            Point::new(doc_origin.x, block_y),
            Size::new(2.0, widget.size * widget.line),
        )),
    }
}

fn ensure_caret_visible(
    state: &mut DocWidgetState,
    widget: &DocWidget<'_>,
    pos: DocPos,
    bounds: Rectangle,
    viewport: &Rectangle,
    shell: &mut Shell<'_, crate::app::Message>,
) {
    let Some(block_top) = state.prefix_ys.get(pos.block).copied() else {
        return;
    };
    let mut local_caret_y = block_top;
    let mut caret_h = widget.size * widget.line;
    if let Some(cached) = state.blocks.get(pos.block) {
        match &cached.material {
            Some(BlockContent::Text { shaped, text_rect, .. }) => {
                let (_, cy, ch) = caret_local_rect(shaped, pos.offset);
                local_caret_y = block_top + text_rect.y + cy;
                caret_h = ch;
            }
            Some(BlockContent::Table(table_mat)) => {
                if let Some(cell) = table_mat.cells.iter().find(|c| c.cell_idx == pos.cell) {
                    let (cx, cy, ch) = caret_local_rect(&cell.shaped, pos.offset);
                    local_caret_y = block_top + 4.0 + cell.text_origin.y + cy;
                    caret_h = ch;
                    if table_mat.total_w > bounds.width + 1.0 {
                        let abs_cx = cell.text_origin.x + cx;
                        let scroll_x = state.table_scroll_x.get(&pos.block).copied().unwrap_or(0.0);
                        let max_s = (table_mat.total_w - bounds.width).max(0.0);
                        if abs_cx < scroll_x + 24.0 {
                            state
                                .table_scroll_x
                                .insert(pos.block, (abs_cx - 28.0).clamp(0.0, max_s));
                        } else if abs_cx > scroll_x + bounds.width - 24.0 {
                            state.table_scroll_x.insert(
                                pos.block,
                                (abs_cx - bounds.width + 32.0).clamp(0.0, max_s),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let local_top = (viewport.y - bounds.y).max(0.0);
    let view_h = viewport.height.max(100.0);
    let local_bottom = local_top + view_h;
    if local_caret_y < local_top + 24.0 {
        shell.publish((widget.on_action)(DocAction::ScrollToY(
            (local_caret_y - 24.0).max(0.0),
        )));
    } else if local_caret_y + caret_h > local_bottom - 24.0 {
        shell.publish((widget.on_action)(DocAction::ScrollToY(
            (local_caret_y + caret_h + 28.0 - view_h).max(0.0),
        )));
    }
}

fn move_line_edge(state: &DocWidgetState, widget: &DocWidget<'_>, to_end: bool) -> DocPos {
    let pos = widget.caret;
    let plain = unit_plain(&widget.doc, pos);
    let Some(cached) = state.blocks.get(pos.block) else {
        return pos;
    };
    let shaped = match &cached.material {
        Some(BlockContent::Text { shaped, .. }) => shaped,
        Some(BlockContent::Table(t)) => {
            let Some(cell) = t.cells.iter().find(|c| c.cell_idx == pos.cell) else {
                return pos;
            };
            &cell.shaped
        }
        _ => return pos,
    };
    let (_, cy, ch) = caret_local_rect(shaped, pos.offset);
    let target_y = cy + ch * 0.5;
    let runs: Vec<cosmic_text::LayoutRun<'_>> = shaped.paragraph.buffer().layout_runs().collect();
    if let Some(run) = runs
        .iter()
        .find(|r| target_y >= r.line_top && target_y <= r.line_top + r.line_height)
    {
        let base = shaped.line_starts.get(run.line_i).copied().unwrap_or(0);
        let new_off = if to_end {
            base + run.glyphs.iter().map(|g| g.end).max().unwrap_or(0)
        } else {
            base + run.glyphs.iter().map(|g| g.start).min().unwrap_or(0)
        };
        return DocPos::new(pos.block, pos.cell, snap_to_char_boundary(plain, new_off));
    }
    DocPos::new(pos.block, pos.cell, if to_end { plain.len() } else { 0 })
}

fn move_vertical(state: &mut DocWidgetState, widget: &DocWidget<'_>, dir: isize) -> DocPos {
    let pos = widget.caret;
    let Some(cached) = state.blocks.get(pos.block) else {
        return pos;
    };

    let (current_goal_x, current_y_rel, shaped_opt, origin_x) = match &cached.material {
        Some(BlockContent::Text { shaped, text_rect, .. }) => {
            let (cx, cy, ch) = caret_local_rect(shaped, pos.offset);
            let gx = *state.goal_x.get_or_insert(text_rect.x + cx);
            (gx, cy + ch * 0.5, Some(shaped), text_rect.x)
        }
        Some(BlockContent::Table(t)) => {
            if let Some(cell) = t.cells.iter().find(|c| c.cell_idx == pos.cell) {
                let (cx, cy, ch) = caret_local_rect(&cell.shaped, pos.offset);
                let scroll_x = state.table_scroll_x.get(&pos.block).copied().unwrap_or(0.0);
                let gx = *state.goal_x.get_or_insert(cell.text_origin.x + cx - scroll_x);
                (gx, cy + ch * 0.5, Some(&cell.shaped), cell.text_origin.x - scroll_x)
            } else {
                (*state.goal_x.get_or_insert(0.0), 0.0, None, 0.0)
            }
        }
        _ => (*state.goal_x.get_or_insert(0.0), 0.0, None, 0.0),
    };

    if let Some(shaped) = shaped_opt {
        let runs: Vec<(usize, f32, f32)> = shaped
            .paragraph
            .buffer()
            .layout_runs()
            .enumerate()
            .map(|(i, r)| (i, r.line_top, r.line_height))
            .collect();
        if let Some((cur_vi, _, _)) = runs
            .iter()
            .find(|(_, top, h)| current_y_rel >= *top && current_y_rel <= *top + *h)
            .copied()
        {
            let next_vi = cur_vi as isize + dir;
            if next_vi >= 0 && (next_vi as usize) < runs.len() {
                let (_, top, h) = runs[next_vi as usize];
                let rel_pt = Point::new((current_goal_x - origin_x).max(0.0), top + h * 0.5);
                let off = hit_shaped_offset(shaped, rel_pt, unit_plain(&widget.doc, pos));
                return DocPos::new(pos.block, pos.cell, off);
            }
        }
    }

    if let BlockKind::Table(table) = &widget.doc.blocks[pos.block].kind {
        if let Some(cur_cell) = table.cells.get(pos.cell) {
            let target_row = cur_cell.row as isize + dir;
            if target_row >= 0 && (target_row as usize) < table.rows {
                let tr = target_row as usize;
                let next_ci = table
                    .cells
                    .iter()
                    .enumerate()
                    .find(|(_, c)| c.row == tr && cur_cell.col >= c.col && cur_cell.col < c.col + c.colspan)
                    .or_else(|| table.cells.iter().enumerate().find(|(_, c)| c.row == tr))
                    .map(|(i, _)| i);
                if let Some(nci) = next_ci {
                    let target_pos = DocPos::new(pos.block, nci, 0);
                    return place_at_unit_edge(state, widget, target_pos, current_goal_x, dir < 0);
                }
            }
        }
    }

    let next_block = pos.block as isize + dir;
    if next_block >= 0 && (next_block as usize) < widget.doc.blocks.len() {
        let nb = next_block as usize;
        state.materialize_range(widget, nb..nb + 1, state.last_local_top, false);
        if let Some(BlockContent::Table(t)) = state.blocks.get(nb).and_then(|c| c.material.as_ref()) {
            let target_row = if dir < 0 { t.rows.saturating_sub(1) } else { 0 };
            let scroll_x = state.table_scroll_x.get(&nb).copied().unwrap_or(0.0);
            let tx = current_goal_x + scroll_x;
            let best_cell = t
                .cells
                .iter()
                .filter(|c| c.row == target_row)
                .min_by_key(|c| {
                    let center = c.box_rect.x + c.box_rect.width * 0.5;
                    ((center - tx).abs() * 10.0) as i64
                })
                .map(|c| c.cell_idx)
                .unwrap_or(0);
            return place_at_unit_edge(state, widget, DocPos::new(nb, best_cell, 0), current_goal_x, dir < 0);
        }
        return place_at_unit_edge(state, widget, DocPos::new(nb, 0, 0), current_goal_x, dir < 0);
    }

    pos
}

fn place_at_unit_edge(
    state: &mut DocWidgetState,
    widget: &DocWidget<'_>,
    pos: DocPos,
    goal_x: f32,
    from_bottom: bool,
) -> DocPos {
    state.materialize_range(widget, pos.block..pos.block + 1, state.last_local_top, false);
    let plain = unit_plain(&widget.doc, pos);
    let Some(cached) = state.blocks.get(pos.block) else {
        return pos;
    };
    let (shaped, origin_x) = match &cached.material {
        Some(BlockContent::Text { shaped, text_rect, .. }) => (shaped, text_rect.x),
        Some(BlockContent::Table(t)) => {
            let Some(cell) = t.cells.iter().find(|c| c.cell_idx == pos.cell) else {
                return pos;
            };
            let scroll_x = state.table_scroll_x.get(&pos.block).copied().unwrap_or(0.0);
            (&cell.shaped, cell.text_origin.x - scroll_x)
        }
        _ => return pos,
    };
    let runs: Vec<(f32, f32)> = shaped
        .paragraph
        .buffer()
        .layout_runs()
        .map(|r| (r.line_top, r.line_height))
        .collect();
    if let Some((top, h)) = if from_bottom { runs.last() } else { runs.first() } {
        let off = hit_shaped_offset(
            shaped,
            Point::new((goal_x - origin_x).max(0.0), top + h * 0.5),
            plain,
        );
        DocPos::new(pos.block, pos.cell, off)
    } else {
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc;

    #[test]
    fn estimate_height_scales_with_content_and_type() {
        let d = doc::parse("# 标题\n\n第一段普通文字。\n\n---\n\n| A | B |\n| :--- | :--- |\n| 1 | 2 |\n| 3 | 4 |\n", None);
        let h_h1 = estimate_block_height(&d.blocks[0], 740.0, 16.0, 1.75, false);
        let h_p = estimate_block_height(&d.blocks[1], 740.0, 16.0, 1.75, false);
        let h_rule = estimate_block_height(&d.blocks[2], 740.0, 16.0, 1.75, false);
        let h_tbl = estimate_block_height(&d.blocks[3], 740.0, 16.0, 1.75, false);
        assert!(h_h1 > h_p);
        assert_eq!(h_rule, 21.0);
        assert!(h_tbl > h_h1);
    }

    #[test]
    fn height_cache_sync_incremental_update() {
        let d1 = Arc::new(doc::parse("第一段\n\n第二段\n\n第三段", None));
        let mut d2_val = (*d1).clone();
        d2_val.blocks[1].plain = "第二段\n换了很多行\n第三行\n第四行".into();
        d2_val.blocks[1].runs = vec![Run {
            text: d2_val.blocks[1].plain.clone(),
            bold: false,
            italic: false,
            code: false,
            strike: false,
            link: None,
        }];
        let d2 = Arc::new(d2_val);

        let mut state = DocWidgetState::default();
        let pal = crate::theme::all()[0].pal;
        let w1 = DocWidget {
            id: iced::widget::Id::new("doc"),
            doc: d1.clone(),
            doc_rev: 1,
            edited_block: None,
            materialize_hint: None,
            images: Arc::new(ImageStore::new()),
            pal,
            theme_idx: 0,
            body_font: Font::DEFAULT,
            mono_font: Font::MONOSPACE,
            size: 16.0,
            line: 1.75,
            content_width: 740.0,
            editing: true,
            overlay_open: false,
            caret: DocPos::new(0, 0, 0),
            anchor: None,
            search: None,
            search_rev: 0,
            renders: Arc::new(AtomicU32::new(0)),
            on_action: Box::new(|_| crate::app::Message::Noop),
        };
        state.sync(&w1);
        state.blocks[0].measured_width = 740.0;
        state.blocks[2].measured_width = 740.0;

        let w2 = DocWidget {
            doc: d2,
            doc_rev: 2,
            edited_block: Some(1),
            ..w1
        };
        state.sync(&w2);
        assert_eq!(state.blocks[0].measured_width, 740.0);
        assert_eq!(state.blocks[1].measured_width, 0.0);
        assert_eq!(state.blocks[2].measured_width, 740.0);
        assert!(state.prefix_ys[2] > state.prefix_ys[1] + 50.0);
    }
}
