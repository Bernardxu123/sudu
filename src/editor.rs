use crate::doc::{self, Block, BlockKind, Doc, Run};
use crate::table::{self, CellAlign, Table};
use iced::widget::Id as WidgetId;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocPos {
    pub block: usize,
    pub cell: usize,
    pub offset: usize,
}

impl DocPos {
    pub fn new(block: usize, cell: usize, offset: usize) -> Self {
        Self { block, cell, offset }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatKind {
    Bold,
    Italic,
    Code,
    Strike,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableOp {
    InsertRowBelow,
    InsertRowAbove,
    DeleteRow,
    InsertColRight,
    InsertColLeft,
    DeleteCol,
    Align(CellAlign),
}

#[derive(Clone)]
struct Snapshot {
    blocks: Vec<Block>,
    caret: DocPos,
    anchor: Option<DocPos>,
}

#[derive(Clone)]
pub struct EditorState {
    pub caret: DocPos,
    pub anchor: Option<DocPos>,
    pub dirty: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            caret: DocPos::new(0, 0, 0),
            anchor: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl EditorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, doc: &Doc) {
        self.caret = first_editable_pos(doc);
        self.anchor = None;
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
    }

    pub fn selection_range(&self) -> Option<(DocPos, DocPos)> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            None
        } else if anchor < self.caret {
            Some((anchor, self.caret))
        } else {
            Some((self.caret, anchor))
        }
    }

    pub fn clamp(&mut self, doc: &Doc) {
        if doc.blocks.is_empty() {
            self.caret = DocPos::new(0, 0, 0);
            self.anchor = None;
            return;
        }
        self.caret = clamp_pos(doc, self.caret);
        if let Some(a) = self.anchor {
            self.anchor = Some(clamp_pos(doc, a));
        }
    }

    fn push_undo(&mut self, doc: &Doc) {
        self.undo.push(Snapshot {
            blocks: doc.blocks.clone(),
            caret: self.caret,
            anchor: self.anchor,
        });
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, doc: &mut Doc) -> bool {
        let Some(snap) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            blocks: doc.blocks.clone(),
            caret: self.caret,
            anchor: self.anchor,
        });
        doc.blocks = snap.blocks;
        self.caret = snap.caret;
        self.anchor = snap.anchor;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn redo(&mut self, doc: &mut Doc) -> bool {
        let Some(snap) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            blocks: doc.blocks.clone(),
            caret: self.caret,
            anchor: self.anchor,
        });
        doc.blocks = snap.blocks;
        self.caret = snap.caret;
        self.anchor = snap.anchor;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn select_all(&mut self, doc: &Doc) {
        if doc.blocks.is_empty() {
            return;
        }
        self.anchor = Some(first_editable_pos(doc));
        self.caret = last_editable_pos(doc);
    }

    #[cfg(test)]
    pub fn move_left(&mut self, doc: &Doc, extend: bool) {
        self.prepare_move(extend);
        self.caret = step_pos_left(doc, self.caret);
        if !extend {
            self.anchor = None;
        }
    }

    #[cfg(test)]
    pub fn move_right(&mut self, doc: &Doc, extend: bool) {
        self.prepare_move(extend);
        self.caret = step_pos_right(doc, self.caret);
        if !extend {
            self.anchor = None;
        }
    }

    pub fn move_word_left(&mut self, doc: &Doc, extend: bool) {
        self.prepare_move(extend);
        let text = unit_plain(doc, self.caret);
        if self.caret.offset == 0 {
            self.caret = step_pos_left(doc, self.caret);
        } else {
            self.caret.offset = prev_word_boundary(text, self.caret.offset);
        }
        if !extend {
            self.anchor = None;
        }
    }

    pub fn move_word_right(&mut self, doc: &Doc, extend: bool) {
        self.prepare_move(extend);
        let text = unit_plain(doc, self.caret);
        if self.caret.offset >= text.len() {
            self.caret = step_pos_right(doc, self.caret);
        } else {
            self.caret.offset = next_word_boundary(text, self.caret.offset);
        }
        if !extend {
            self.anchor = None;
        }
    }

    pub fn move_doc_start(&mut self, doc: &Doc, extend: bool) {
        self.prepare_move(extend);
        self.caret = first_editable_pos(doc);
        if !extend {
            self.anchor = None;
        }
    }

    pub fn move_doc_end(&mut self, doc: &Doc, extend: bool) {
        self.prepare_move(extend);
        self.caret = last_editable_pos(doc);
        if !extend {
            self.anchor = None;
        }
    }

    fn prepare_move(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else if let Some((start, end)) = self.selection_range() {
            self.caret = if self.caret == start { start } else { end };
        }
    }

    pub fn insert_text(&mut self, doc: &mut Doc, text: &str, doc_path: Option<&Path>) -> bool {
        if text.is_empty() {
            return false;
        }
        self.ensure_non_empty(doc);
        self.push_undo(doc);
        self.delete_selection_internal(doc);
        self.clamp(doc);

        if matches!(
            doc.blocks[self.caret.block].kind,
            BlockKind::Rule | BlockKind::Image { .. }
        ) {
            let new_idx = self.caret.block + 1;
            doc.blocks.insert(new_idx, empty_paragraph());
            self.caret = DocPos::new(new_idx, 0, 0);
        }

        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.contains('\n') && !is_code_block(doc, self.caret.block) && !is_table_block(doc, self.caret.block) {
            let parts: Vec<&str> = normalized.split('\n').collect();
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    self.split_block_internal(doc);
                }
                if !part.is_empty() {
                    self.insert_into_current_unit(doc, part, doc_path);
                }
            }
        } else {
            self.insert_into_current_unit(doc, &normalized, doc_path);
        }

        self.anchor = None;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    fn insert_into_current_unit(&mut self, doc: &mut Doc, text: &str, doc_path: Option<&Path>) {
        let pos = self.caret;
        let Some(block) = doc.blocks.get_mut(pos.block) else {
            return;
        };
        block.raw = None;
        match &mut block.kind {
            BlockKind::Table(table) => {
                if let Some(cell) = table.cells.get_mut(pos.cell) {
                    let off = pos.offset.min(cell.plain.len());
                    insert_into_runs(&mut cell.runs, off, text);
                    self.caret.offset = off + text.len();
                    if should_try_inline_trigger(text) {
                        try_apply_inline_markdown(&mut cell.runs, &mut self.caret.offset, doc_path);
                    }
                    table.refresh_plain();
                    block.plain = table.plain.clone();
                }
            }
            BlockKind::Code { .. } => {
                if block.runs.is_empty() {
                    block.runs.push(Run {
                        text: text.to_string(),
                        code: true,
                        ..Run::default()
                    });
                    self.caret.offset = text.len();
                } else {
                    let off = pos.offset.min(block.plain.len());
                    insert_into_runs_preserve_code(&mut block.runs, off, text);
                    self.caret.offset = off + text.len();
                }
                block.plain = runs_plain(&block.runs);
            }
            _ => {
                let off = pos.offset.min(block.plain.len());
                insert_into_runs(&mut block.runs, off, text);
                self.caret.offset = off + text.len();
                block.plain = runs_plain(&block.runs);
                if !try_block_markdown_prefix(block, &mut self.caret.offset)
                    && should_try_inline_trigger(text)
                {
                    try_apply_inline_markdown(&mut block.runs, &mut self.caret.offset, doc_path);
                    block.plain = runs_plain(&block.runs);
                }
            }
        }
    }

    pub fn enter(&mut self, doc: &mut Doc, shift: bool, doc_path: Option<&Path>) -> bool {
        self.ensure_non_empty(doc);
        self.push_undo(doc);
        self.delete_selection_internal(doc);
        self.clamp(doc);

        let pos = self.caret;
        let is_table = is_table_block(doc, pos.block);
        if is_table {
            if shift {
                self.insert_into_current_unit(doc, "\n", doc_path);
            } else {
                self.table_enter_next_row(doc);
            }
            self.anchor = None;
            doc::rebuild_metadata(doc);
            self.dirty = true;
            return true;
        }

        if is_code_block(doc, pos.block) {
            if shift {
                let next_idx = pos.block + 1;
                doc.blocks.insert(next_idx, empty_paragraph());
                self.caret = DocPos::new(next_idx, 0, 0);
            } else {
                self.insert_into_current_unit(doc, "\n", doc_path);
            }
            self.anchor = None;
            doc::rebuild_metadata(doc);
            self.dirty = true;
            return true;
        }

        if shift {
            self.insert_into_current_unit(doc, "\n", doc_path);
            self.anchor = None;
            doc::rebuild_metadata(doc);
            self.dirty = true;
            return true;
        }

        if try_block_enter_transform(doc, pos, &mut self.caret, doc_path) {
            self.anchor = None;
            doc::rebuild_metadata(doc);
            self.dirty = true;
            return true;
        }

        self.split_block_internal(doc);
        self.anchor = None;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    fn table_enter_next_row(&mut self, doc: &mut Doc) {
        let pos = self.caret;
        let Some(block) = doc.blocks.get_mut(pos.block) else {
            return;
        };
        let BlockKind::Table(table) = &mut block.kind else {
            return;
        };
        let Some(cur) = table.cells.get(pos.cell).cloned() else {
            return;
        };
        let target_row = cur.row + cur.rowspan;
        if target_row >= table.rows {
            block.raw = None;
            table.insert_row(table.rows);
        }
        if let Some(next_idx) = find_cell_at(table, target_row, cur.col) {
            self.caret = DocPos::new(pos.block, next_idx, 0);
        }
    }

    pub fn tab(&mut self, doc: &mut Doc, shift: bool) -> bool {
        let pos = self.caret;
        if let Some(block) = doc.blocks.get_mut(pos.block) {
            if let BlockKind::Table(table) = &mut block.kind {
                if table.cells.is_empty() {
                    return false;
                }
                if shift {
                    if pos.cell > 0 {
                        let prev = pos.cell - 1;
                        let len = table.cells[prev].plain.len();
                        self.caret = DocPos::new(pos.block, prev, len);
                    }
                } else if pos.cell + 1 < table.cells.len() {
                    let next = pos.cell + 1;
                    let len = table.cells[next].plain.len();
                    self.caret = DocPos::new(pos.block, next, len);
                } else {
                    self.push_undo(doc);
                    let target = {
                        let block = &mut doc.blocks[pos.block];
                        block.raw = None;
                        let BlockKind::Table(table) = &mut block.kind else {
                            return false;
                        };
                        let new_row = table.rows;
                        table.insert_row(new_row);
                        find_cell_at(table, new_row, 0).unwrap_or(pos.cell)
                    };
                    self.caret = DocPos::new(pos.block, target, 0);
                    doc::rebuild_metadata(doc);
                    self.dirty = true;
                }
                self.anchor = None;
                return true;
            }
            if let BlockKind::ListItem { indent: _, .. } = &mut block.kind {
                self.push_undo(doc);
                let BlockKind::ListItem { indent, .. } = &mut doc.blocks[pos.block].kind else {
                    return false;
                };
                if shift {
                    *indent = indent.saturating_sub(1);
                } else {
                    *indent = (*indent + 1).min(6);
                }
                doc.blocks[pos.block].raw = None;
                self.dirty = true;
                return true;
            }
        }
        false
    }

    pub fn backspace(&mut self, doc: &mut Doc) -> bool {
        self.ensure_non_empty(doc);
        self.push_undo(doc);
        if self.delete_selection_internal(doc) {
            self.anchor = None;
            doc::rebuild_metadata(doc);
            self.dirty = true;
            return true;
        }
        self.clamp(doc);
        let pos = self.caret;
        let block = &mut doc.blocks[pos.block];
        match &mut block.kind {
            BlockKind::Table(table) => {
                if pos.offset == 0 {
                    self.undo.pop();
                    return false;
                }
                if let Some(cell) = table.cells.get_mut(pos.cell) {
                    let prev = prev_cluster_boundary(&cell.plain, pos.offset);
                    delete_from_runs(&mut cell.runs, prev, pos.offset);
                    self.caret.offset = prev;
                    block.raw = None;
                    table.refresh_plain();
                    block.plain = table.plain.clone();
                }
            }
            BlockKind::Rule | BlockKind::Image { .. } => {
                doc.blocks.remove(pos.block);
                if doc.blocks.is_empty() {
                    doc.blocks.push(empty_paragraph());
                    self.caret = DocPos::new(0, 0, 0);
                } else {
                    let prev_idx = pos.block.saturating_sub(1);
                    self.caret = end_of_block_pos(doc, prev_idx);
                }
            }
            _ if pos.offset > 0 => {
                let prev = prev_cluster_boundary(&block.plain, pos.offset);
                delete_from_runs(&mut block.runs, prev, pos.offset);
                block.plain = runs_plain(&block.runs);
                block.raw = None;
                self.caret.offset = prev;
            }
            BlockKind::ListItem { indent, checked, .. } => {
                if *indent > 0 {
                    *indent -= 1;
                } else if checked.is_some() {
                    *checked = None;
                } else {
                    block.kind = BlockKind::Paragraph;
                }
                block.raw = None;
            }
            BlockKind::Heading(_) | BlockKind::Quote => {
                block.kind = BlockKind::Paragraph;
                block.raw = None;
            }
            BlockKind::Code { .. } => {
                if block.plain.is_empty() {
                    block.kind = BlockKind::Paragraph;
                    block.runs.clear();
                    block.raw = None;
                } else if pos.block > 0 {
                    self.undo.pop();
                    return false;
                } else {
                    self.undo.pop();
                    return false;
                }
            }
            BlockKind::Paragraph => {
                if pos.block == 0 {
                    self.undo.pop();
                    return false;
                }
                let prev_idx = pos.block - 1;
                match &doc.blocks[prev_idx].kind {
                    BlockKind::Rule | BlockKind::Image { .. } => {
                        doc.blocks.remove(prev_idx);
                        self.caret = DocPos::new(prev_idx, 0, 0);
                    }
                    BlockKind::Table(_) => {
                        if doc.blocks[pos.block].plain.is_empty() {
                            doc.blocks.remove(pos.block);
                        }
                        self.caret = end_of_block_pos(doc, prev_idx);
                    }
                    _ => {
                        let cur_block = doc.blocks.remove(pos.block);
                        let prev = &mut doc.blocks[prev_idx];
                        let join_offset = prev.plain.len();
                        let was_code = matches!(prev.kind, BlockKind::Code { .. });
                        for mut run in cur_block.runs {
                            if was_code {
                                run.code = true;
                                run.bold = false;
                                run.italic = false;
                                run.strike = false;
                                run.link = None;
                            }
                            push_merged_run(&mut prev.runs, run);
                        }
                        prev.plain = runs_plain(&prev.runs);
                        prev.raw = None;
                        self.caret = DocPos::new(prev_idx, 0, join_offset);
                    }
                }
            }
        }
        self.anchor = None;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn delete_forward(&mut self, doc: &mut Doc) -> bool {
        self.ensure_non_empty(doc);
        self.push_undo(doc);
        if self.delete_selection_internal(doc) {
            self.anchor = None;
            doc::rebuild_metadata(doc);
            self.dirty = true;
            return true;
        }
        self.clamp(doc);
        let pos = self.caret;
        let unit_len = unit_plain(doc, pos).len();
        if pos.offset < unit_len {
            let block = &mut doc.blocks[pos.block];
            match &mut block.kind {
                BlockKind::Table(table) => {
                    if let Some(cell) = table.cells.get_mut(pos.cell) {
                        let next = next_cluster_boundary(&cell.plain, pos.offset);
                        delete_from_runs(&mut cell.runs, pos.offset, next);
                        block.raw = None;
                        table.refresh_plain();
                        block.plain = table.plain.clone();
                    }
                }
                BlockKind::Rule | BlockKind::Image { .. } => {}
                _ => {
                    let next = next_cluster_boundary(&block.plain, pos.offset);
                    delete_from_runs(&mut block.runs, pos.offset, next);
                    block.plain = runs_plain(&block.runs);
                    block.raw = None;
                }
            }
        } else if !is_table_block(doc, pos.block) && pos.block + 1 < doc.blocks.len() {
            let next_idx = pos.block + 1;
            match &doc.blocks[next_idx].kind {
                BlockKind::Table(_) => {
                    self.undo.pop();
                    return false;
                }
                BlockKind::Rule | BlockKind::Image { .. } => {
                    doc.blocks.remove(next_idx);
                }
                _ => {
                    let next_block = doc.blocks.remove(next_idx);
                    let cur = &mut doc.blocks[pos.block];
                    let was_code = matches!(cur.kind, BlockKind::Code { .. });
                    for mut run in next_block.runs {
                        if was_code {
                            run.code = true;
                            run.bold = false;
                            run.italic = false;
                            run.strike = false;
                            run.link = None;
                        }
                        push_merged_run(&mut cur.runs, run);
                    }
                    cur.plain = runs_plain(&cur.runs);
                    cur.raw = None;
                }
            }
        } else {
            self.undo.pop();
            return false;
        }
        self.anchor = None;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn table_op(&mut self, doc: &mut Doc, op: TableOp) -> bool {
        let pos = self.caret;
        let Some(block) = doc.blocks.get(pos.block) else {
            return false;
        };
        let BlockKind::Table(table) = &block.kind else {
            return false;
        };
        let Some(cur_cell) = table.cells.get(pos.cell).cloned() else {
            return false;
        };
        self.push_undo(doc);
        let block = &mut doc.blocks[pos.block];
        let BlockKind::Table(table) = &mut block.kind else {
            return false;
        };
        let mut target_row = cur_cell.row;
        let mut target_col = cur_cell.col;
        let mut changed = true;
        match op {
            TableOp::InsertRowBelow => {
                target_row = cur_cell.row + cur_cell.rowspan;
                table.insert_row(target_row);
            }
            TableOp::InsertRowAbove => {
                target_row = cur_cell.row;
                table.insert_row(target_row);
            }
            TableOp::DeleteRow => {
                changed = table.delete_row(cur_cell.row);
                target_row = cur_cell.row.min(table.rows.saturating_sub(1));
            }
            TableOp::InsertColRight => {
                target_col = cur_cell.col + cur_cell.colspan;
                table.insert_col(target_col);
            }
            TableOp::InsertColLeft => {
                target_col = cur_cell.col;
                table.insert_col(target_col);
            }
            TableOp::DeleteCol => {
                changed = table.delete_col(cur_cell.col);
                target_col = cur_cell.col.min(table.cols.saturating_sub(1));
            }
            TableOp::Align(align) => {
                table.set_col_align(cur_cell.col, align);
            }
        }
        if !changed {
            self.undo.pop();
            return false;
        }
        block.raw = None;
        block.plain = table.plain.clone();
        if let Some(cell_idx) = find_cell_at(table, target_row, target_col) {
            let off = self.caret.offset.min(table.cells[cell_idx].plain.len());
            self.caret = DocPos::new(pos.block, cell_idx, off);
        } else {
            self.caret = DocPos::new(pos.block, 0, 0);
        }
        self.anchor = None;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn insert_table(&mut self, doc: &mut Doc, rows: usize, cols: usize) -> bool {
        self.push_undo(doc);
        if doc.blocks.is_empty() {
            doc.blocks.push(empty_paragraph());
        }
        let table = Table::new_empty(rows, cols);
        let plain = table.plain.clone();
        let insert_idx = (self.caret.block + 1).min(doc.blocks.len());
        doc.blocks.insert(
            insert_idx,
            Block {
                id: WidgetId::unique(),
                kind: BlockKind::Table(Box::new(table)),
                runs: Vec::new(),
                plain,
                span: 0..0,
                raw: None,
            },
        );
        self.caret = DocPos::new(insert_idx, 0, 0);
        self.anchor = None;
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn toggle_format(&mut self, doc: &mut Doc, fmt: FormatKind) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        if start.block != end.block || start.cell != end.cell {
            return false;
        }
        let Some(block) = doc.blocks.get(start.block) else {
            return false;
        };
        if matches!(block.kind, BlockKind::Code { .. } | BlockKind::Rule | BlockKind::Image { .. }) {
            return false;
        }
        self.push_undo(doc);
        let block = &mut doc.blocks[start.block];
        block.raw = None;
        match &mut block.kind {
            BlockKind::Table(table) => {
                if let Some(cell) = table.cells.get_mut(start.cell) {
                    toggle_runs_format(&mut cell.runs, start.offset, end.offset, fmt);
                    table.refresh_plain();
                    block.plain = table.plain.clone();
                }
            }
            _ => {
                toggle_runs_format(&mut block.runs, start.offset, end.offset, fmt);
                block.plain = runs_plain(&block.runs);
            }
        }
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn toggle_task_checkbox(&mut self, doc: &mut Doc, block_idx: usize) -> bool {
        let Some(block) = doc.blocks.get(block_idx) else {
            return false;
        };
        if !matches!(
            block.kind,
            BlockKind::ListItem {
                checked: Some(_),
                ..
            }
        ) {
            return false;
        }
        self.push_undo(doc);
        if let BlockKind::ListItem {
            checked: Some(flag),
            ..
        } = &mut doc.blocks[block_idx].kind
        {
            *flag = !*flag;
            doc.blocks[block_idx].raw = None;
        }
        doc::rebuild_metadata(doc);
        self.dirty = true;
        true
    }

    pub fn delete_selection(&mut self, doc: &mut Doc) -> bool {
        if self.selection_range().is_none() {
            return false;
        }
        self.push_undo(doc);
        let deleted = self.delete_selection_internal(doc);
        if deleted {
            self.anchor = None;
            doc::rebuild_metadata(doc);
            self.dirty = true;
        } else {
            self.undo.pop();
        }
        deleted
    }

    fn delete_selection_internal(&mut self, doc: &mut Doc) -> bool {
        let Some((start, end)) = self.selection_range() else {
            return false;
        };
        if start.block == end.block {
            let Some(block) = doc.blocks.get_mut(start.block) else {
                return false;
            };
            block.raw = None;
            match &mut block.kind {
                BlockKind::Table(table) => {
                    for c in start.cell..=end.cell.min(table.cells.len().saturating_sub(1)) {
                        let cell = &mut table.cells[c];
                        let from = if c == start.cell { start.offset } else { 0 };
                        let to = if c == end.cell {
                            end.offset
                        } else {
                            cell.plain.len()
                        };
                        let from = from.min(cell.plain.len());
                        let to = to.max(from).min(cell.plain.len());
                        if from < to {
                            delete_from_runs(&mut cell.runs, from, to);
                        }
                    }
                    table.refresh_plain();
                    block.plain = table.plain.clone();
                }
                BlockKind::Rule | BlockKind::Image { .. } => {}
                _ => {
                    let from = start.offset.min(block.plain.len());
                    let to = end.offset.max(from).min(block.plain.len());
                    delete_from_runs(&mut block.runs, from, to);
                    block.plain = runs_plain(&block.runs);
                }
            }
            self.caret = start;
            return true;
        }

        let end_tail_runs = if let Some(end_blk) = doc.blocks.get_mut(end.block) {
            end_blk.raw = None;
            match &mut end_blk.kind {
                BlockKind::Table(table) => {
                    for c in 0..=end.cell.min(table.cells.len().saturating_sub(1)) {
                        let cell = &mut table.cells[c];
                        let to = if c == end.cell {
                            end.offset.min(cell.plain.len())
                        } else {
                            cell.plain.len()
                        };
                        if to > 0 {
                            delete_from_runs(&mut cell.runs, 0, to);
                        }
                    }
                    table.refresh_plain();
                    end_blk.plain = table.plain.clone();
                    None
                }
                BlockKind::Rule | BlockKind::Image { .. } => Some(Vec::new()),
                _ => {
                    let to = end.offset.min(end_blk.plain.len());
                    delete_from_runs(&mut end_blk.runs, 0, to);
                    Some(std::mem::take(&mut end_blk.runs))
                }
            }
        } else {
            None
        };

        let start_is_table = is_table_block(doc, start.block);
        if start_is_table {
            if let Some(start_blk) = doc.blocks.get_mut(start.block) {
                start_blk.raw = None;
                if let BlockKind::Table(table) = &mut start_blk.kind {
                    for c in start.cell..table.cells.len() {
                        let cell = &mut table.cells[c];
                        let from = if c == start.cell {
                            start.offset.min(cell.plain.len())
                        } else {
                            0
                        };
                        let len = cell.plain.len();
                        if from < len {
                            delete_from_runs(&mut cell.runs, from, len);
                        }
                    }
                    table.refresh_plain();
                    start_blk.plain = table.plain.clone();
                }
            }
            let remove_end = if end_tail_runs.is_some() {
                end.block + 1
            } else {
                end.block
            };
            if start.block + 1 < remove_end && remove_end <= doc.blocks.len() {
                doc.blocks.drain(start.block + 1..remove_end);
            }
            if let Some(tail) = end_tail_runs {
                if !tail.is_empty() {
                    let mut para = empty_paragraph();
                    para.runs = tail;
                    para.plain = runs_plain(&para.runs);
                    doc.blocks.insert(start.block + 1, para);
                }
            }
            self.caret = start;
            return true;
        }

        let remove_from = start.block + 1;
        let remove_to = if end_tail_runs.is_some() {
            end.block + 1
        } else {
            end.block
        };
        if remove_from < remove_to && remove_to <= doc.blocks.len() {
            doc.blocks.drain(remove_from..remove_to);
        }
        if let Some(start_blk) = doc.blocks.get_mut(start.block) {
            start_blk.raw = None;
            if matches!(start_blk.kind, BlockKind::Rule | BlockKind::Image { .. }) {
                start_blk.kind = BlockKind::Paragraph;
                start_blk.runs.clear();
                start_blk.plain.clear();
            } else {
                let from = start.offset.min(start_blk.plain.len());
                let len = start_blk.plain.len();
                if from < len {
                    delete_from_runs(&mut start_blk.runs, from, len);
                }
            }
            let was_code = matches!(start_blk.kind, BlockKind::Code { .. });
            if let Some(tail) = end_tail_runs {
                for mut run in tail {
                    if was_code {
                        run.code = true;
                        run.bold = false;
                        run.italic = false;
                        run.strike = false;
                        run.link = None;
                    }
                    push_merged_run(&mut start_blk.runs, run);
                }
            }
            start_blk.plain = runs_plain(&start_blk.runs);
        }
        self.caret = start;
        true
    }

    fn split_block_internal(&mut self, doc: &mut Doc) {
        let pos = self.caret;
        let Some(block) = doc.blocks.get_mut(pos.block) else {
            return;
        };
        block.raw = None;
        if matches!(block.kind, BlockKind::Rule | BlockKind::Image { .. }) {
            let next_idx = pos.block + 1;
            doc.blocks.insert(next_idx, empty_paragraph());
            self.caret = DocPos::new(next_idx, 0, 0);
            return;
        }
        if let BlockKind::ListItem {
            indent,
            ordered,
            number,
            checked,
            ..
        } = &block.kind
        {
            if block.plain.trim().is_empty() && pos.offset == 0 {
                block.kind = BlockKind::Paragraph;
                block.runs.clear();
                block.plain.clear();
                self.caret = DocPos::new(pos.block, 0, 0);
                return;
            }
            let next_num = number + 1;
            let next_marker = if *ordered {
                format!("{next_num}. ")
            } else {
                String::from("• ")
            };
            let next_kind = BlockKind::ListItem {
                indent: *indent,
                marker: next_marker,
                ordered: *ordered,
                number: next_num,
                checked: checked.map(|_| false),
            };
            let (left, right) = split_runs_at(&block.runs, pos.offset);
            block.runs = left;
            block.plain = runs_plain(&block.runs);
            let mut next_block = Block {
                id: WidgetId::unique(),
                kind: next_kind,
                runs: right,
                plain: String::new(),
                span: 0..0,
                raw: None,
            };
            next_block.plain = runs_plain(&next_block.runs);
            let next_idx = pos.block + 1;
            doc.blocks.insert(next_idx, next_block);
            self.caret = DocPos::new(next_idx, 0, 0);
            return;
        }

        let (left, right) = split_runs_at(&block.runs, pos.offset);
        let next_kind = match &block.kind {
            BlockKind::Quote if !block.plain.is_empty() && pos.offset > 0 => BlockKind::Quote,
            _ => BlockKind::Paragraph,
        };
        if matches!(block.kind, BlockKind::Quote) && block.plain.is_empty() {
            block.kind = BlockKind::Paragraph;
            self.caret = DocPos::new(pos.block, 0, 0);
            return;
        }
        block.runs = left;
        block.plain = runs_plain(&block.runs);
        let mut next_block = Block {
            id: WidgetId::unique(),
            kind: next_kind,
            runs: right,
            plain: String::new(),
            span: 0..0,
            raw: None,
        };
        next_block.plain = runs_plain(&next_block.runs);
        let next_idx = pos.block + 1;
        doc.blocks.insert(next_idx, next_block);
        self.caret = DocPos::new(next_idx, 0, 0);
    }

    fn ensure_non_empty(&mut self, doc: &mut Doc) {
        if doc.blocks.is_empty() {
            doc.blocks.push(empty_paragraph());
            self.caret = DocPos::new(0, 0, 0);
            self.anchor = None;
        }
    }
}

pub fn extract_selected_text(doc: &Doc, start: DocPos, end: DocPos) -> String {
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    if doc.blocks.is_empty() || start == end {
        return String::new();
    }
    let mut out = String::new();
    let end_b = end.block.min(doc.blocks.len() - 1);
    for b in start.block..=end_b {
        if b > start.block {
            let prev_list = matches!(doc.blocks[b - 1].kind, BlockKind::ListItem { .. });
            let curr_list = matches!(doc.blocks[b].kind, BlockKind::ListItem { .. });
            out.push_str(if prev_list && curr_list { "\n" } else { "\n\n" });
        }
        let block = &doc.blocks[b];
        match &block.kind {
            BlockKind::Table(table) => {
                if table.cells.is_empty() {
                    continue;
                }
                let c_start = if b == start.block {
                    start.cell.min(table.cells.len() - 1)
                } else {
                    0
                };
                let c_end = if b == end.block {
                    end.cell.min(table.cells.len() - 1)
                } else {
                    table.cells.len() - 1
                };
                let mut prev_row = table.cells[c_start].row;
                for c in c_start..=c_end {
                    let cell = &table.cells[c];
                    if c > c_start {
                        if cell.row != prev_row {
                            out.push('\n');
                        } else {
                            out.push('\t');
                        }
                    }
                    prev_row = cell.row;
                    let from = if b == start.block && c == start.cell {
                        start.offset.min(cell.plain.len())
                    } else {
                        0
                    };
                    let to = if b == end.block && c == end.cell {
                        end.offset.min(cell.plain.len())
                    } else {
                        cell.plain.len()
                    };
                    if from < to {
                        out.push_str(&cell.plain[from..to]);
                    }
                }
            }
            BlockKind::Rule => {}
            BlockKind::Image { alt, .. } => {
                if alt.is_empty() {
                    out.push_str("[图片]");
                } else {
                    out.push_str(&format!("[图片：{alt}]"));
                }
            }
            _ => {
                let from = if b == start.block {
                    start.offset.min(block.plain.len())
                } else {
                    0
                };
                let to = if b == end.block {
                    end.offset.min(block.plain.len())
                } else {
                    block.plain.len()
                };
                if from < to {
                    out.push_str(&block.plain[from..to]);
                }
            }
        }
    }
    out
}

pub fn empty_paragraph() -> Block {
    Block {
        id: WidgetId::unique(),
        kind: BlockKind::Paragraph,
        runs: Vec::new(),
        plain: String::new(),
        span: 0..0,
        raw: None,
    }
}

fn is_code_block(doc: &Doc, block: usize) -> bool {
    doc.blocks
        .get(block)
        .is_some_and(|b| matches!(b.kind, BlockKind::Code { .. }))
}

fn is_table_block(doc: &Doc, block: usize) -> bool {
    doc.blocks
        .get(block)
        .is_some_and(|b| matches!(b.kind, BlockKind::Table(_)))
}

fn find_cell_at(table: &Table, row: usize, col: usize) -> Option<usize> {
    table
        .cells
        .iter()
        .position(|c| c.row <= row && row < c.row + c.rowspan && c.col <= col && col < c.col + c.colspan)
        .or_else(|| {
            table
                .cells
                .iter()
                .position(|c| c.row == row)
        })
}

pub fn unit_plain<'a>(doc: &'a Doc, pos: DocPos) -> &'a str {
    let Some(block) = doc.blocks.get(pos.block) else {
        return "";
    };
    match &block.kind {
        BlockKind::Table(table) => table
            .cells
            .get(pos.cell)
            .map(|c| c.plain.as_str())
            .unwrap_or(""),
        BlockKind::Rule | BlockKind::Image { .. } => "",
        _ => block.plain.as_str(),
    }
}

pub fn clamp_pos(doc: &Doc, mut pos: DocPos) -> DocPos {
    if doc.blocks.is_empty() {
        return DocPos::new(0, 0, 0);
    }
    pos.block = pos.block.min(doc.blocks.len() - 1);
    let block = &doc.blocks[pos.block];
    match &block.kind {
        BlockKind::Table(table) => {
            if table.cells.is_empty() {
                pos.cell = 0;
                pos.offset = 0;
            } else {
                pos.cell = pos.cell.min(table.cells.len() - 1);
                let len = table.cells[pos.cell].plain.len();
                pos.offset = snap_char_boundary(&table.cells[pos.cell].plain, pos.offset.min(len));
            }
        }
        BlockKind::Rule | BlockKind::Image { .. } => {
            pos.cell = 0;
            pos.offset = 0;
        }
        _ => {
            pos.cell = 0;
            let len = block.plain.len();
            pos.offset = snap_char_boundary(&block.plain, pos.offset.min(len));
        }
    }
    pos
}

fn snap_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

pub fn first_editable_pos(_doc: &Doc) -> DocPos {
    DocPos::new(0, 0, 0)
}

pub fn last_editable_pos(doc: &Doc) -> DocPos {
    if doc.blocks.is_empty() {
        return DocPos::new(0, 0, 0);
    }
    end_of_block_pos(doc, doc.blocks.len() - 1)
}

pub fn end_of_block_pos(doc: &Doc, block_idx: usize) -> DocPos {
    let Some(block) = doc.blocks.get(block_idx) else {
        return DocPos::new(0, 0, 0);
    };
    match &block.kind {
        BlockKind::Table(table) => {
            if table.cells.is_empty() {
                DocPos::new(block_idx, 0, 0)
            } else {
                let last = table.cells.len() - 1;
                DocPos::new(block_idx, last, table.cells[last].plain.len())
            }
        }
        BlockKind::Rule | BlockKind::Image { .. } => DocPos::new(block_idx, 0, 0),
        _ => DocPos::new(block_idx, 0, block.plain.len()),
    }
}

pub fn step_pos_left(doc: &Doc, pos: DocPos) -> DocPos {
    let pos = clamp_pos(doc, pos);
    if pos.offset > 0 {
        let text = unit_plain(doc, pos);
        return DocPos::new(pos.block, pos.cell, prev_cluster_boundary(text, pos.offset));
    }
    if pos.cell > 0 {
        let prev_cell = pos.cell - 1;
        let len = unit_plain(doc, DocPos::new(pos.block, prev_cell, 0)).len();
        return DocPos::new(pos.block, prev_cell, len);
    }
    if pos.block > 0 {
        return end_of_block_pos(doc, pos.block - 1);
    }
    pos
}

pub fn step_pos_right(doc: &Doc, pos: DocPos) -> DocPos {
    let pos = clamp_pos(doc, pos);
    let text = unit_plain(doc, pos);
    if pos.offset < text.len() {
        return DocPos::new(pos.block, pos.cell, next_cluster_boundary(text, pos.offset));
    }
    if let Some(block) = doc.blocks.get(pos.block) {
        if let BlockKind::Table(table) = &block.kind {
            if pos.cell + 1 < table.cells.len() {
                return DocPos::new(pos.block, pos.cell + 1, 0);
            }
        }
    }
    if pos.block + 1 < doc.blocks.len() {
        return DocPos::new(pos.block + 1, 0, 0);
    }
    pos
}

pub fn prev_cluster_boundary(text: &str, offset: usize) -> usize {
    let mut pos = snap_char_boundary(text, offset);
    if pos == 0 {
        return 0;
    }
    let Some(ch) = text[..pos].chars().next_back() else {
        return 0;
    };
    pos -= ch.len_utf8();
    if ch == '\n' && pos > 0 && text.as_bytes()[pos - 1] == b'\r' {
        return pos - 1;
    }
    while pos > 0 {
        let Some(prev) = text[..pos].chars().next_back() else {
            break;
        };
        if is_cluster_extender(ch) || prev == '\u{200D}' {
            pos -= prev.len_utf8();
            if !is_cluster_extender(prev) && prev != '\u{200D}' {
                break;
            }
        } else {
            break;
        }
    }
    pos
}

pub fn next_cluster_boundary(text: &str, offset: usize) -> usize {
    let mut pos = snap_char_boundary(text, offset);
    if pos >= text.len() {
        return text.len();
    }
    let Some(first) = text[pos..].chars().next() else {
        return text.len();
    };
    pos += first.len_utf8();
    if first == '\r' && text.as_bytes().get(pos) == Some(&b'\n') {
        return pos + 1;
    }
    while pos < text.len() {
        let Some(next) = text[pos..].chars().next() else {
            break;
        };
        if is_cluster_extender(next) {
            pos += next.len_utf8();
        } else if next == '\u{200D}' {
            pos += next.len_utf8();
            if let Some(joined) = text[pos..].chars().next() {
                pos += joined.len_utf8();
            }
        } else {
            break;
        }
    }
    pos
}

fn is_cluster_extender(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
            | 0x1F3FB..=0x1F3FF
            | 0xE0100..=0xE01EF
    )
}

fn prev_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = snap_char_boundary(text, offset);
    while pos > 0 {
        let Some(ch) = text[..pos].chars().next_back() else {
            break;
        };
        if ch.is_whitespace() {
            pos -= ch.len_utf8();
        } else {
            break;
        }
    }
    while pos > 0 {
        let Some(ch) = text[..pos].chars().next_back() else {
            break;
        };
        if ch.is_alphanumeric() || ch == '_' {
            pos -= ch.len_utf8();
        } else {
            break;
        }
    }
    if pos == offset && offset > 0 {
        prev_cluster_boundary(text, offset)
    } else {
        pos
    }
}

fn next_word_boundary(text: &str, offset: usize) -> usize {
    let mut pos = snap_char_boundary(text, offset);
    while pos < text.len() {
        let Some(ch) = text[pos..].chars().next() else {
            break;
        };
        if ch.is_alphanumeric() || ch == '_' {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    while pos < text.len() {
        let Some(ch) = text[pos..].chars().next() else {
            break;
        };
        if ch.is_whitespace() {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    if pos == offset && offset < text.len() {
        next_cluster_boundary(text, offset)
    } else {
        pos
    }
}

fn runs_plain(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

fn push_merged_run(runs: &mut Vec<Run>, run: Run) {
    if run.text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut() {
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
    runs.push(run);
}

fn insert_into_runs(runs: &mut Vec<Run>, offset: usize, text: &str) {
    if text.is_empty() {
        return;
    }
    if runs.is_empty() {
        runs.push(Run {
            text: text.to_string(),
            ..Run::default()
        });
        return;
    }
    let break_inline = text.starts_with(char::is_whitespace);
    let mut cursor = 0usize;
    let mut idx = 0usize;
    while idx < runs.len() {
        let len = runs[idx].text.len();
        if offset <= cursor + len {
            let local = offset - cursor;
            if local == len && (runs[idx].code || runs[idx].link.is_some()) && break_inline {
                let new_run = Run {
                    text: text.to_string(),
                    ..Run::default()
                };
                runs.insert(idx + 1, new_run);
                merge_adjacent_runs(runs);
                return;
            }
            if local == 0 && (runs[idx].code || runs[idx].link.is_some()) && break_inline {
                let new_run = Run {
                    text: text.to_string(),
                    ..Run::default()
                };
                runs.insert(idx, new_run);
                merge_adjacent_runs(runs);
                return;
            }
            runs[idx].text.insert_str(local, text);
            return;
        }
        cursor += len;
        idx += 1;
    }
    if let Some(last) = runs.last_mut() {
        if (last.code || last.link.is_some()) && break_inline {
            runs.push(Run {
                text: text.to_string(),
                ..Run::default()
            });
        } else {
            last.text.push_str(text);
        }
    }
}

fn insert_into_runs_preserve_code(runs: &mut Vec<Run>, offset: usize, text: &str) {
    if runs.is_empty() {
        runs.push(Run {
            text: text.to_string(),
            code: true,
            ..Run::default()
        });
        return;
    }
    let mut cursor = 0usize;
    for run in runs.iter_mut() {
        let len = run.text.len();
        if offset <= cursor + len {
            run.text.insert_str(offset - cursor, text);
            return;
        }
        cursor += len;
    }
    if let Some(last) = runs.last_mut() {
        last.text.push_str(text);
    }
}

fn delete_from_runs(runs: &mut Vec<Run>, start: usize, end: usize) {
    if start >= end {
        return;
    }
    let mut out = Vec::with_capacity(runs.len());
    let mut cursor = 0usize;
    for mut run in std::mem::take(runs) {
        let r_start = cursor;
        let r_end = cursor + run.text.len();
        cursor = r_end;
        if r_end <= start || r_start >= end {
            push_merged_run(&mut out, run);
            continue;
        }
        let keep_left = start.saturating_sub(r_start).min(run.text.len());
        let keep_right = end.saturating_sub(r_start).min(run.text.len());
        if keep_left > 0 || keep_right < run.text.len() {
            let mut next = String::with_capacity(keep_left + (run.text.len() - keep_right));
            next.push_str(&run.text[..keep_left]);
            next.push_str(&run.text[keep_right..]);
            run.text = next;
            push_merged_run(&mut out, run);
        }
    }
    *runs = out;
}

fn split_runs_at(runs: &[Run], offset: usize) -> (Vec<Run>, Vec<Run>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut cursor = 0usize;
    for run in runs {
        let r_start = cursor;
        let r_end = cursor + run.text.len();
        cursor = r_end;
        if r_end <= offset {
            push_merged_run(&mut left, run.clone());
        } else if r_start >= offset {
            push_merged_run(&mut right, run.clone());
        } else {
            let mid = offset - r_start;
            let mut l = run.clone();
            l.text = run.text[..mid].to_string();
            let mut r = run.clone();
            r.text = run.text[mid..].to_string();
            push_merged_run(&mut left, l);
            push_merged_run(&mut right, r);
        }
    }
    (left, right)
}

fn toggle_runs_format(runs: &mut Vec<Run>, start: usize, end: usize, fmt: FormatKind) {
    if start >= end {
        return;
    }
    let mut pieces = Vec::new();
    let mut cursor = 0usize;
    let mut all_active = true;
    for run in runs.iter() {
        let r_start = cursor;
        let r_end = cursor + run.text.len();
        cursor = r_end;
        if r_end > start && r_start < end {
            let active = match fmt {
                FormatKind::Bold => run.bold,
                FormatKind::Italic => run.italic,
                FormatKind::Code => run.code,
                FormatKind::Strike => run.strike,
            };
            if !active {
                all_active = false;
                break;
            }
        }
    }
    let enable = !all_active;
    let mut cursor = 0usize;
    for run in std::mem::take(runs) {
        let r_start = cursor;
        let r_end = cursor + run.text.len();
        cursor = r_end;
        if r_end <= start || r_start >= end {
            push_merged_run(&mut pieces, run);
            continue;
        }
        let s = start.saturating_sub(r_start);
        let e = (end - r_start).min(run.text.len());
        if s > 0 {
            let mut head = run.clone();
            head.text = run.text[..s].to_string();
            push_merged_run(&mut pieces, head);
        }
        if s < e {
            let mut mid = run.clone();
            mid.text = run.text[s..e].to_string();
            match fmt {
                FormatKind::Bold => mid.bold = enable,
                FormatKind::Italic => mid.italic = enable,
                FormatKind::Code => mid.code = enable,
                FormatKind::Strike => mid.strike = enable,
            }
            push_merged_run(&mut pieces, mid);
        }
        if e < run.text.len() {
            let mut tail = run.clone();
            tail.text = run.text[e..].to_string();
            push_merged_run(&mut pieces, tail);
        }
    }
    *runs = pieces;
}

fn merge_adjacent_runs(runs: &mut Vec<Run>) {
    let mut out = Vec::with_capacity(runs.len());
    for run in std::mem::take(runs) {
        push_merged_run(&mut out, run);
    }
    *runs = out;
}

fn try_block_markdown_prefix(block: &mut Block, caret_offset: &mut usize) -> bool {
    if matches!(
        block.kind,
        BlockKind::ListItem {
            ordered: false,
            checked: None,
            ..
        }
    ) {
        if let Some(prefix_len) = match_task_prefix(&block.plain) {
            if *caret_offset == prefix_len {
                let checked = block.plain[..prefix_len].contains(['x', 'X']);
                delete_from_runs(&mut block.runs, 0, prefix_len);
                block.plain = runs_plain(&block.runs);
                if let BlockKind::ListItem { checked: slot, .. } = &mut block.kind {
                    *slot = Some(checked);
                }
                *caret_offset = 0;
                return true;
            }
        }
        return false;
    }
    if !matches!(block.kind, BlockKind::Paragraph) {
        return false;
    }
    let plain = &block.plain;
    let hashes = plain.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes)
        && plain.as_bytes().get(hashes) == Some(&b' ')
        && *caret_offset == hashes + 1
    {
        delete_from_runs(&mut block.runs, 0, hashes + 1);
        block.plain = runs_plain(&block.runs);
        block.kind = BlockKind::Heading(hashes as u8);
        *caret_offset = 0;
        return true;
    }
    if (plain.starts_with("- ") || plain.starts_with("* ") || plain.starts_with("+ "))
        && *caret_offset == 2
    {
        delete_from_runs(&mut block.runs, 0, 2);
        block.plain = runs_plain(&block.runs);
        block.kind = BlockKind::ListItem {
            indent: 0,
            marker: String::from("• "),
            ordered: false,
            number: 1,
            checked: None,
        };
        *caret_offset = 0;
        return true;
    }
    if plain.starts_with("> ") && *caret_offset == 2 {
        delete_from_runs(&mut block.runs, 0, 2);
        block.plain = runs_plain(&block.runs);
        block.kind = BlockKind::Quote;
        *caret_offset = 0;
        return true;
    }
    let digits = plain.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0
        && plain.as_bytes().get(digits) == Some(&b'.')
        && plain.as_bytes().get(digits + 1) == Some(&b' ')
        && *caret_offset == digits + 2
    {
        if let Ok(number) = plain[..digits].parse::<u64>() {
            delete_from_runs(&mut block.runs, 0, digits + 2);
            block.plain = runs_plain(&block.runs);
            block.kind = BlockKind::ListItem {
                indent: 0,
                marker: format!("{number}. "),
                ordered: true,
                number,
                checked: None,
            };
            *caret_offset = 0;
            return true;
        }
    }
    false
}

fn match_task_prefix(plain: &str) -> Option<usize> {
    if plain.starts_with("[ ] ") || plain.starts_with("[x] ") || plain.starts_with("[X] ") {
        Some(4)
    } else {
        None
    }
}

fn try_block_enter_transform(
    doc: &mut Doc,
    pos: DocPos,
    caret: &mut DocPos,
    doc_path: Option<&Path>,
) -> bool {
    let Some(block) = doc.blocks.get(pos.block) else {
        return false;
    };
    if !matches!(block.kind, BlockKind::Paragraph) || pos.offset != block.plain.len() {
        return false;
    }
    let trimmed = block.plain.trim();
    if matches!(trimmed, "---" | "***" | "___") {
        doc.blocks[pos.block].kind = BlockKind::Rule;
        doc.blocks[pos.block].runs.clear();
        doc.blocks[pos.block].plain.clear();
        doc.blocks[pos.block].raw = None;
        doc.blocks.insert(pos.block + 1, empty_paragraph());
        *caret = DocPos::new(pos.block + 1, 0, 0);
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if !rest.contains('`') {
            let lang = rest.split_whitespace().next().unwrap_or("").to_string();
            doc.blocks[pos.block].kind = BlockKind::Code { lang };
            doc.blocks[pos.block].runs.clear();
            doc.blocks[pos.block].plain.clear();
            doc.blocks[pos.block].raw = None;
            *caret = DocPos::new(pos.block, 0, 0);
            return true;
        }
    }
    if trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 3 {
        let cells = table::split_table_row(trimmed);
        if cells.len() >= 1 {
            let sep = format!("|{}|", vec![" --- "; cells.len()].join("|"));
            let md = format!("{trimmed}\n{sep}");
            if let Some(mut tbl) = table::parse_gfm_table(&md, 0, doc_path) {
                tbl.insert_row(1);
                let plain = tbl.plain.clone();
                doc.blocks[pos.block].kind = BlockKind::Table(Box::new(tbl));
                doc.blocks[pos.block].runs.clear();
                doc.blocks[pos.block].plain = plain;
                doc.blocks[pos.block].raw = None;
                let target_cell = doc.blocks[pos.block]
                    .kind
                    .as_table()
                    .and_then(|t| find_cell_at(t, 1, 0))
                    .unwrap_or(0);
                *caret = DocPos::new(pos.block, target_cell, 0);
                return true;
            }
        }
    }
    false
}

trait BlockTableExt {
    fn as_table(&self) -> Option<&Table>;
}

impl BlockTableExt for BlockKind {
    fn as_table(&self) -> Option<&Table> {
        match self {
            BlockKind::Table(t) => Some(t.as_ref()),
            _ => None,
        }
    }
}

fn should_try_inline_trigger(inserted: &str) -> bool {
    matches!(
        inserted.as_bytes().last().copied(),
        Some(b'*' | b'~' | b'`' | b')')
    )
}

fn try_apply_inline_markdown(
    runs: &mut Vec<Run>,
    caret_offset: &mut usize,
    doc_path: Option<&Path>,
) {
    let target = *caret_offset;
    let mut cursor = 0usize;
    let mut found_idx = None;
    for (i, run) in runs.iter().enumerate() {
        let end = cursor + run.text.len();
        if target <= end && !run.code && run.link.is_none() {
            found_idx = Some((i, cursor, target - cursor));
            break;
        }
        cursor = end;
    }
    let Some((run_idx, run_base, local_end)) = found_idx else {
        return;
    };
    let text = runs[run_idx].text.clone();
    let prefix = &text[..local_end];

    let replacement: Option<(usize, Run)> = if let Some(inner_end) = prefix.strip_suffix("**") {
        inner_end.rfind("**").and_then(|open| {
            let inner = &inner_end[open + 2..];
            (!inner.is_empty()).then(|| {
                (
                    open,
                    Run {
                        text: inner.to_string(),
                        bold: true,
                        ..runs[run_idx].clone()
                    },
                )
            })
        })
    } else if let Some(inner_end) = prefix.strip_suffix("~~") {
        inner_end.rfind("~~").and_then(|open| {
            let inner = &inner_end[open + 2..];
            (!inner.is_empty()).then(|| {
                (
                    open,
                    Run {
                        text: inner.to_string(),
                        strike: true,
                        ..runs[run_idx].clone()
                    },
                )
            })
        })
    } else if let Some(inner_end) = prefix.strip_suffix('`') {
        inner_end.rfind('`').and_then(|open| {
            let inner = &inner_end[open + 1..];
            (!inner.is_empty()).then(|| {
                (
                    open,
                    Run {
                        text: inner.to_string(),
                        code: true,
                        bold: false,
                        italic: false,
                        strike: false,
                        link: None,
                    },
                )
            })
        })
    } else if let Some(inner_end) = prefix.strip_suffix('*') {
        if !inner_end.ends_with('*') {
            inner_end.rfind('*').and_then(|open| {
                if open > 0 && inner_end.as_bytes()[open - 1] == b'*' {
                    return None;
                }
                let inner = &inner_end[open + 1..];
                (!inner.is_empty()).then(|| {
                    (
                        open,
                        Run {
                            text: inner.to_string(),
                            italic: true,
                            ..runs[run_idx].clone()
                        },
                    )
                })
            })
        } else {
            None
        }
    } else if let Some(before_paren) = prefix.strip_suffix(')') {
        parse_inline_link_suffix(before_paren).and_then(|(open, label, url)| {
            let link = doc::resolve_link(url, doc_path)?;
            Some((
                open,
                Run {
                    text: label.to_string(),
                    link: Some(link),
                    ..runs[run_idx].clone()
                },
            ))
        })
    } else {
        None
    };

    let Some((open_local, new_run)) = replacement else {
        return;
    };
    let new_len = new_run.text.len();
    let orig_run = runs.remove(run_idx);
    let mut insert_at = run_idx;
    if open_local > 0 {
        let mut head = orig_run.clone();
        head.text = text[..open_local].to_string();
        runs.insert(insert_at, head);
        insert_at += 1;
    }
    runs.insert(insert_at, new_run);
    insert_at += 1;
    if local_end < text.len() {
        let mut tail = orig_run;
        tail.text = text[local_end..].to_string();
        runs.insert(insert_at, tail);
    }
    merge_adjacent_runs(runs);
    *caret_offset = run_base + open_local + new_len;
}

fn parse_inline_link_suffix(before_paren: &str) -> Option<(usize, &str, &str)> {
    let mid = before_paren.rfind("](")?;
    let url = &before_paren[mid + 2..];
    if url.is_empty() || url.contains(['\n', ' ']) {
        return None;
    }
    let before_mid = &before_paren[..mid];
    let open = before_mid.rfind('[')?;
    let label = &before_mid[open + 1..];
    if label.is_empty() {
        return None;
    }
    Some((open, label, url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_block_selection_copy_and_delete() {
        let mut doc = doc::parse("# 标题\n\n第一段文字\n\n第二段末尾\n", None);
        let mut ed = EditorState::new();
        ed.anchor = Some(DocPos::new(0, 0, "标".len()));
        ed.caret = DocPos::new(2, 0, "第二段".len());
        let (s, e) = ed.selection_range().unwrap();
        let copied = extract_selected_text(&doc, s, e);
        assert_eq!(copied, "题\n\n第一段文字\n\n第二段");

        assert!(ed.delete_selection(&mut doc));
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc.blocks[0].plain, "标末尾");
        assert!(matches!(doc.blocks[0].kind, BlockKind::Heading(1)));

        assert!(ed.undo(&mut doc));
        assert_eq!(doc.blocks.len(), 3);
        assert_eq!(doc.blocks[1].plain, "第一段文字");
    }

    #[test]
    fn live_markdown_prefix_and_inline_triggers() {
        let mut doc = doc::parse("初稿", None);
        let mut ed = EditorState::new();
        ed.caret = DocPos::new(0, 0, 0);
        ed.insert_text(&mut doc, "## ", None);
        assert!(matches!(doc.blocks[0].kind, BlockKind::Heading(2)));
        assert_eq!(doc.blocks[0].plain, "初稿");

        ed.caret = DocPos::new(0, 0, doc.blocks[0].plain.len());
        ed.move_left(&doc, false);
        ed.move_right(&doc, false);
        ed.insert_text(&mut doc, " **粗体**", None);
        assert_eq!(doc.blocks[0].plain, "初稿 粗体");
        assert!(doc.blocks[0].runs.iter().any(|r| r.bold && r.text == "粗体"));
    }

    #[test]
    fn table_cell_tab_and_row_col_operations() {
        let mut doc = doc::parse("| 列1 | 列2 |\n| --- | --- |\n| A | B |\n", None);
        let mut ed = EditorState::new();
        ed.caret = DocPos::new(0, 2, 1);
        assert!(ed.tab(&mut doc, false));
        assert_eq!(ed.caret, DocPos::new(0, 3, 1));
        assert!(ed.tab(&mut doc, false));
        let BlockKind::Table(tbl) = &doc.blocks[0].kind else {
            panic!("expected table");
        };
        assert_eq!(tbl.rows, 3);
        assert_eq!(ed.caret, DocPos::new(0, 4, 0));

        assert!(ed.table_op(&mut doc, TableOp::InsertColRight));
        let BlockKind::Table(tbl) = &doc.blocks[0].kind else {
            panic!("expected table");
        };
        assert_eq!(tbl.cols, 3);
        assert!(ed.table_op(&mut doc, TableOp::Align(CellAlign::Center)));
        let BlockKind::Table(tbl) = &doc.blocks[0].kind else {
            panic!("expected table");
        };
        assert_eq!(tbl.alignments[1], CellAlign::Center);
        let md = doc::serialize(&doc);
        assert!(md.contains(":---:"));
    }
}
