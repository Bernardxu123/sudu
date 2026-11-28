use crate::theme::Palette;
use iced::advanced::layout;
use iced::advanced::mouse;
use iced::advanced::overlay;
use iced::advanced::renderer;
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{Clipboard, Layout, Shell};
use iced::{border, Alignment, Border, Color, Element, Event, Length, Rectangle, Size, Vector};

pub struct TableCellSlot<'a, Message> {
    pub row: usize,
    pub col: usize,
    pub colspan: usize,
    pub rowspan: usize,
    pub header: bool,
    pub align_x: Alignment,
    pub element: Element<'a, Message>,
}

#[derive(Clone, Copy)]
pub struct TablePalette {
    pub bg: Color,
    pub header_bg: Color,
    pub zebra_bg: Color,
    pub hover_bg: Color,
    pub border: Color,
    pub header_border: Color,
    pub zebra: bool,
}

impl TablePalette {
    pub fn from_palette(pal: Palette, has_rowspan: bool) -> Self {
        Self {
            bg: pal.bg,
            header_bg: pal.panel,
            zebra_bg: mix_color(pal.bg, pal.panel, 0.48),
            hover_bg: mix_color(pal.bg, pal.selection, 0.32),
            border: pal.border,
            header_border: mix_color(pal.border, pal.muted, 0.36),
            zebra: !has_rowspan,
        }
    }
}

fn mix_color(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

pub struct TableView<'a, Message> {
    rows: usize,
    cols: usize,
    max_width: f32,
    pad_x: f32,
    pad_y: f32,
    min_row_height: f32,
    font_size: f32,
    palette: TablePalette,
    cells: Vec<TableCellSlot<'a, Message>>,
}

impl<'a, Message> TableView<'a, Message> {
    pub fn new(
        rows: usize,
        cols: usize,
        max_width: f32,
        font_size: f32,
        line_height: f32,
        palette: TablePalette,
        cells: Vec<TableCellSlot<'a, Message>>,
    ) -> Self {
        let pad_x = (font_size * 0.72).clamp(10.0, 16.0);
        let pad_y = (font_size * 0.46).clamp(7.0, 12.0);
        let min_row_height = (font_size * line_height + pad_y * 2.0).ceil();
        Self {
            rows,
            cols,
            max_width,
            pad_x,
            pad_y,
            min_row_height,
            font_size,
            palette,
            cells,
        }
    }
}

#[derive(Default)]
struct TableLayoutState {
    col_widths: Vec<f32>,
    row_heights: Vec<f32>,
    col_xs: Vec<f32>,
    row_ys: Vec<f32>,
    row_is_header: Vec<bool>,
    hovered_row: Option<usize>,
}

pub fn allocate_column_widths(
    pref_w: &[f32],
    header_w: &[f32],
    viewport_width: f32,
    font_size: f32,
) -> Vec<f32> {
    let cols = pref_w.len();
    if cols == 0 {
        return Vec::new();
    }
    let borders = (cols + 1) as f32;
    let avail = viewport_width.max(240.0);
    let budget = (avail - borders).max(60.0);
    let natural_sum: f32 = pref_w.iter().sum();

    if natural_sum <= budget {
        return pref_w.to_vec();
    }

    let base_floor = (font_size * 5.2).clamp(72.0, 118.0);
    let min_w: Vec<f32> = (0..cols)
        .map(|c| {
            let hdr_min = header_w[c].min(base_floor * 1.25);
            let floor = base_floor.max(hdr_min);
            pref_w[c].min(floor)
        })
        .collect();
    let min_sum: f32 = min_w.iter().sum();

    if min_sum <= budget {
        let mut widths = min_w;
        let mut rem = budget - min_sum;
        for _ in 0..cols {
            if rem <= 0.1 {
                break;
            }
            let hungry: Vec<usize> = (0..cols)
                .filter(|&c| widths[c] + 0.1 < pref_w[c])
                .collect();
            if hungry.is_empty() {
                break;
            }
            let share = rem / hungry.len() as f32;
            let mut spent = 0.0f32;
            for c in hungry {
                let need = pref_w[c] - widths[c];
                let add = need.min(share);
                widths[c] += add;
                spent += add;
            }
            if spent <= 0.01 {
                break;
            }
            rem = (rem - spent).max(0.0);
        }
        widths
    } else {
        let scroll_cap = (avail * 0.44).clamp(220.0, 340.0);
        (0..cols)
            .map(|c| pref_w[c].min(scroll_cap).max(min_w[c]))
            .collect()
    }
}

impl<'a, Message> Widget<Message, iced::Theme, iced::Renderer> for TableView<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<TableLayoutState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(TableLayoutState::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.cells
            .iter()
            .map(|cell| Tree::new(&cell.element))
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let widgets: Vec<&Element<'a, Message>> =
            self.cells.iter().map(|cell| &cell.element).collect();
        let diff_len = widgets.len();
        if tree.children.len() > diff_len {
            tree.children.truncate(diff_len);
        }
        for (index, element) in widgets.into_iter().enumerate() {
            if index < tree.children.len() {
                tree.children[index].diff(element.as_widget());
            } else {
                tree.children.push(Tree::new(element.as_widget()));
            }
        }
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        if self.rows == 0 || self.cols == 0 || self.cells.is_empty() {
            return layout::Node::new(Size::ZERO);
        }

        let state = tree.state.downcast_mut::<TableLayoutState>();
        let min_empty_w = (self.font_size * 2.0).max(28.0);
        let mut pref_w = vec![min_empty_w; self.cols];
        let mut header_w = vec![min_empty_w; self.cols];
        let unbounded = layout::Limits::new(Size::ZERO, Size::new(100_000.0, 100_000.0));

        for (i, (cell, child_tree)) in self
            .cells
            .iter_mut()
            .zip(&mut tree.children)
            .enumerate()
        {
            let _ = i;
            let node = cell
                .element
                .as_widget_mut()
                .layout(child_tree, renderer, &unbounded);
            if cell.colspan == 1 && cell.col < self.cols {
                let w = node.size().width.ceil() + self.pad_x * 2.0 + 2.0;
                pref_w[cell.col] = pref_w[cell.col].max(w);
                if cell.header {
                    header_w[cell.col] = header_w[cell.col].max(w);
                }
            }
        }

        for (cell, child_tree) in self.cells.iter_mut().zip(&mut tree.children) {
            if cell.colspan > 1 && cell.col < self.cols {
                let node = cell
                    .element
                    .as_widget_mut()
                    .layout(child_tree, renderer, &unbounded);
                let end_c = (cell.col + cell.colspan).min(self.cols);
                let span_c = end_c - cell.col;
                let need = node.size().width.ceil() + self.pad_x * 2.0 + 2.0;
                let current: f32 = pref_w[cell.col..end_c].iter().sum::<f32>()
                    + span_c.saturating_sub(1) as f32;
                if need > current {
                    let extra = (need - current) / span_c as f32;
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

        let col_widths = allocate_column_widths(&pref_w, &header_w, self.max_width, self.font_size);

        let mut col_xs = Vec::with_capacity(self.cols);
        let mut x = 1.0f32;
        for &w in &col_widths {
            col_xs.push(x);
            x += w + 1.0;
        }
        let total_w = x;

        let mut row_heights = vec![self.min_row_height; self.rows];
        let mut row_is_header = vec![false; self.rows];
        let mut nodes = vec![layout::Node::default(); self.cells.len()];

        for (i, (cell, child_tree)) in self
            .cells
            .iter_mut()
            .zip(&mut tree.children)
            .enumerate()
        {
            if cell.row < self.rows && cell.header {
                row_is_header[cell.row] = true;
            }
            let end_c = (cell.col + cell.colspan).min(self.cols);
            let span_c = end_c.saturating_sub(cell.col).max(1);
            let box_w: f32 = col_widths[cell.col..end_c].iter().sum::<f32>()
                + span_c.saturating_sub(1) as f32;
            let inner_w = (box_w - self.pad_x * 2.0).max(12.0);
            let cell_limits = layout::Limits::new(Size::ZERO, Size::new(inner_w, 100_000.0));
            let node = cell
                .element
                .as_widget_mut()
                .layout(child_tree, renderer, &cell_limits);
            if cell.rowspan == 1 && cell.row < self.rows {
                let h = node.size().height.ceil() + self.pad_y * 2.0;
                row_heights[cell.row] = row_heights[cell.row].max(h);
            }
            nodes[i] = node;
        }

        for (i, cell) in self.cells.iter().enumerate() {
            if cell.rowspan > 1 && cell.row < self.rows {
                let end_r = (cell.row + cell.rowspan).min(self.rows);
                let span_r = end_r - cell.row;
                let need = nodes[i].size().height.ceil() + self.pad_y * 2.0;
                let current: f32 = row_heights[cell.row..end_r].iter().sum::<f32>()
                    + span_r.saturating_sub(1) as f32;
                if need > current {
                    let extra = (need - current) / span_r as f32;
                    for h in &mut row_heights[cell.row..end_r] {
                        *h += extra;
                    }
                }
            }
        }

        let mut row_ys = Vec::with_capacity(self.rows);
        let mut y = 1.0f32;
        for &h in &row_heights {
            row_ys.push(y);
            y += h + 1.0;
        }
        let total_h = y;

        for (i, cell) in self.cells.iter().enumerate() {
            let end_c = (cell.col + cell.colspan).min(self.cols);
            let end_r = (cell.row + cell.rowspan).min(self.rows);
            let span_c = end_c.saturating_sub(cell.col).max(1);
            let span_r = end_r.saturating_sub(cell.row).max(1);
            let cell_x = col_xs[cell.col];
            let cell_y = row_ys[cell.row];
            let box_w: f32 = col_widths[cell.col..end_c].iter().sum::<f32>()
                + span_c.saturating_sub(1) as f32;
            let box_h: f32 = row_heights[cell.row..end_r].iter().sum::<f32>()
                + span_r.saturating_sub(1) as f32;
            let inner_w = (box_w - self.pad_x * 2.0).max(1.0);
            let inner_h = (box_h - self.pad_y * 2.0).max(1.0);
            nodes[i].move_to_mut((cell_x + self.pad_x, cell_y + self.pad_y));
            let align_y = if cell.header || cell.rowspan > 1 {
                Alignment::Center
            } else {
                Alignment::Start
            };
            nodes[i].align_mut(cell.align_x, align_y, Size::new(inner_w, inner_h));
        }

        state.col_widths = col_widths;
        state.row_heights = row_heights;
        state.col_xs = col_xs;
        state.row_ys = row_ys;
        state.row_is_header = row_is_header;

        let resolved = limits.resolve(
            Length::Fixed(total_w),
            Length::Fixed(total_h),
            Size::new(total_w, total_h),
        );
        layout::Node::with_children(resolved, nodes)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        for ((cell, state), child_layout) in self
            .cells
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            cell.element
                .as_widget_mut()
                .operate(state, child_layout, renderer, operation);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<TableLayoutState>();
        let prev_hover = state.hovered_row;
        let next_hover = match cursor.position_in(layout.bounds()) {
            Some(local) => state
                .row_ys
                .iter()
                .zip(&state.row_heights)
                .enumerate()
                .find(|(r, (y, h))| {
                    !state.row_is_header.get(*r).copied().unwrap_or(false)
                        && local.y >= **y
                        && local.y < **y + **h
                })
                .map(|(r, _)| r),
            None => None,
        };
        if next_hover != prev_hover {
            state.hovered_row = next_hover;
            shell.request_redraw();
        }

        for ((cell, child_tree), child_layout) in self
            .cells
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            cell.element.as_widget_mut().update(
                child_tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.cells
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((cell, child_tree), child_layout)| {
                cell.element.as_widget().mouse_interaction(
                    child_tree,
                    child_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if !bounds.intersects(viewport) || self.rows == 0 || self.cols == 0 {
            return;
        }
        let state = tree.state.downcast_ref::<TableLayoutState>();
        if state.col_widths.len() != self.cols || state.row_heights.len() != self.rows {
            return;
        }

        use iced::advanced::Renderer as _;

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: border::rounded(4.0),
                snap: true,
                ..renderer::Quad::default()
            },
            self.palette.bg,
        );

        let mut body_row_seq = 0usize;
        let mut row_bg_index = vec![0usize; self.rows];
        for r in 0..self.rows {
            if !state.row_is_header[r] {
                row_bg_index[r] = body_row_seq;
                body_row_seq += 1;
            }
        }

        for cell in &self.cells {
            let end_c = (cell.col + cell.colspan).min(self.cols);
            let end_r = (cell.row + cell.rowspan).min(self.rows);
            let span_c = end_c.saturating_sub(cell.col).max(1);
            let span_r = end_r.saturating_sub(cell.row).max(1);
            let cell_x = bounds.x + state.col_xs[cell.col];
            let cell_y = bounds.y + state.row_ys[cell.row];
            let box_w: f32 = state.col_widths[cell.col..end_c].iter().sum::<f32>()
                + span_c.saturating_sub(1) as f32;
            let box_h: f32 = state.row_heights[cell.row..end_r].iter().sum::<f32>()
                + span_r.saturating_sub(1) as f32;

            let is_hovered = !cell.header
                && cell.rowspan == 1
                && state.hovered_row == Some(cell.row);
            let bg = if cell.header {
                Some(self.palette.header_bg)
            } else if is_hovered {
                Some(self.palette.hover_bg)
            } else if self.palette.zebra && row_bg_index[cell.row] % 2 == 1 {
                Some(self.palette.zebra_bg)
            } else {
                None
            };

            if let Some(bg) = bg {
                let top_left = if cell.row == 0 && cell.col == 0 { 3.0 } else { 0.0 };
                let top_right = if cell.row == 0 && end_c == self.cols { 3.0 } else { 0.0 };
                let bottom_left = if end_r == self.rows && cell.col == 0 { 3.0 } else { 0.0 };
                let bottom_right = if end_r == self.rows && end_c == self.cols { 3.0 } else { 0.0 };
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            iced::Point::new(cell_x, cell_y),
                            Size::new(box_w, box_h),
                        ),
                        border: Border {
                            radius: border::Radius {
                                top_left,
                                top_right,
                                bottom_right,
                                bottom_left,
                            },
                            ..Border::default()
                        },
                        snap: true,
                        ..renderer::Quad::default()
                    },
                    bg,
                );
            }
        }

        let mut spans_vertical = vec![vec![false; self.cols]; self.rows.saturating_sub(1)];
        let mut spans_horizontal = vec![vec![false; self.cols.saturating_sub(1)]; self.rows];
        for cell in &self.cells {
            let end_c = (cell.col + cell.colspan).min(self.cols);
            let end_r = (cell.row + cell.rowspan).min(self.rows);
            for r in cell.row..end_r.saturating_sub(1) {
                for c in cell.col..end_c {
                    spans_vertical[r][c] = true;
                }
            }
            for r in cell.row..end_r {
                for c in cell.col..end_c.saturating_sub(1) {
                    spans_horizontal[r][c] = true;
                }
            }
        }

        for r in 0..self.rows.saturating_sub(1) {
            let line_y = bounds.y + state.row_ys[r] + state.row_heights[r];
            let is_header_sep = state.row_is_header[r] && !state.row_is_header[r + 1];
            let thickness = if is_header_sep { 1.5 } else { 1.0 };
            let color = if is_header_sep {
                self.palette.header_border
            } else {
                self.palette.border
            };
            for c in 0..self.cols {
                if spans_vertical[r][c] {
                    continue;
                }
                let start_x = if c == 0 {
                    bounds.x + 1.0
                } else {
                    bounds.x + state.col_xs[c] - 1.0
                };
                let end_x = if c + 1 == self.cols {
                    bounds.x + bounds.width - 1.0
                } else {
                    bounds.x + state.col_xs[c] + state.col_widths[c] + 1.0
                };
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            iced::Point::new(start_x, line_y),
                            Size::new((end_x - start_x).max(0.0), thickness),
                        ),
                        snap: true,
                        ..renderer::Quad::default()
                    },
                    color,
                );
            }
        }

        for c in 0..self.cols.saturating_sub(1) {
            let line_x = bounds.x + state.col_xs[c] + state.col_widths[c];
            for r in 0..self.rows {
                if spans_horizontal[r][c] {
                    continue;
                }
                let start_y = if r == 0 {
                    bounds.y + 1.0
                } else {
                    bounds.y + state.row_ys[r] - 1.0
                };
                let end_y = if r + 1 == self.rows {
                    bounds.y + bounds.height - 1.0
                } else {
                    bounds.y + state.row_ys[r] + state.row_heights[r] + 1.0
                };
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            iced::Point::new(line_x, start_y),
                            Size::new(1.0, (end_y - start_y).max(0.0)),
                        ),
                        snap: true,
                        ..renderer::Quad::default()
                    },
                    self.palette.border,
                );
            }
        }

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: self.palette.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                snap: true,
                ..renderer::Quad::default()
            },
            Color::TRANSPARENT,
        );

        for ((cell, child_tree), child_layout) in self
            .cells
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            if child_layout.bounds().intersects(viewport) {
                cell.element.as_widget().draw(
                    child_tree,
                    renderer,
                    theme,
                    style,
                    child_layout,
                    cursor,
                    viewport,
                );
            }
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let children = self
            .cells
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((cell, state), layout)| {
                cell.element
                    .as_widget_mut()
                    .overlay(state, layout, renderer, viewport, translation)
            })
            .collect::<Vec<_>>();
        (!children.is_empty()).then(|| overlay::Group::with_children(children).overlay())
    }
}

impl<'a, Message: 'a> From<TableView<'a, Message>> for Element<'a, Message> {
    fn from(table: TableView<'a, Message>) -> Self {
        Element::new(table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_columns_without_crushing_wide_tables() {
        let narrow_pref = vec![50.0, 90.0, 180.0];
        let narrow_hdr = vec![50.0, 70.0, 80.0];
        let fit = allocate_column_widths(&narrow_pref, &narrow_hdr, 760.0, 17.0);
        assert_eq!(fit, narrow_pref);

        let long_pref = vec![56.0, 110.0, 1400.0];
        let long_hdr = vec![56.0, 88.0, 96.0];
        let wrapped = allocate_column_widths(&long_pref, &long_hdr, 760.0, 17.0);
        assert!((wrapped.iter().sum::<f32>() + 4.0 - 760.0).abs() < 1.0);
        assert!((wrapped[0] - 56.0).abs() < 0.1);
        assert!((wrapped[1] - 110.0).abs() < 0.1);
        assert!(wrapped[2] > 500.0);

        let wide_pref = vec![60.0, 140.0, 160.0, 220.0, 90.0, 80.0, 480.0, 150.0, 120.0, 130.0];
        let wide_hdr = vec![60.0; 10];
        let scroll = allocate_column_widths(&wide_pref, &wide_hdr, 760.0, 17.0);
        let total = scroll.iter().sum::<f32>() + 11.0;
        assert!(total > 760.0);
        assert!(scroll[1] >= 139.0);
        assert!(scroll[6] <= 340.0 && scroll[6] >= 220.0);
    }
}
