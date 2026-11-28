use crate::doc::{resolve_link, Run};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCell {
    pub row: usize,
    pub col: usize,
    pub colspan: usize,
    pub rowspan: usize,
    pub header: bool,
    pub align: CellAlign,
    pub runs: Vec<Run>,
    pub plain: String,
    pub plain_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub rows: usize,
    pub cols: usize,
    pub alignments: Vec<CellAlign>,
    pub cells: Vec<TableCell>,
    pub plain: String,
    pub has_rowspan: bool,
    pub is_html: bool,
}

struct RawRow {
    header: bool,
    cells: Vec<String>,
}

pub fn parse_gfm_table(source: &str, quote_depth: usize, path: Option<&Path>) -> Option<Table> {
    let lines: Vec<String> = source
        .lines()
        .map(|line| strip_container_prefix(line, quote_depth))
        .collect();
    if lines.len() < 2 {
        return None;
    }
    let header_cells = split_table_row(&lines[0]);
    let mut alignments = parse_delimiter_row(&lines[1])?;
    if header_cells.len() != alignments.len() {
        return None;
    }

    let mut raw_rows = Vec::with_capacity(lines.len().saturating_sub(1));
    raw_rows.push(RawRow {
        header: true,
        cells: header_cells,
    });
    for line in &lines[2..] {
        if line.trim().is_empty() {
            break;
        }
        raw_rows.push(RawRow {
            header: false,
            cells: split_table_row(line),
        });
    }

    let col_count = raw_rows
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(alignments.len())
        .max(alignments.len())
        .max(1);
    while alignments.len() < col_count {
        alignments.push(CellAlign::Left);
    }

    let row_count = raw_rows.len();
    let mut cells = Vec::with_capacity(row_count * col_count);
    for (row_idx, mut row) in raw_rows.into_iter().enumerate() {
        while row.cells.len() < col_count {
            row.cells.push(String::new());
        }
        for (col_idx, raw_cell) in row.cells.into_iter().enumerate() {
            let runs = parse_cell_inline(&raw_cell, path);
            let cell_plain = runs_plain(&runs);
            cells.push(TableCell {
                row: row_idx,
                col: col_idx,
                colspan: 1,
                rowspan: 1,
                header: row.header,
                align: alignments[col_idx],
                runs,
                plain: cell_plain,
                plain_range: 0..0,
            });
        }
    }

    Some(finalize_table(row_count, col_count, alignments, cells, false))
}

pub fn find_gfm_table_in_lines(
    source: &str,
    quote_depth: usize,
) -> Option<(usize, usize)> {
    let mut line_offsets = Vec::new();
    let mut pos = 0usize;
    for segment in source.split('\n') {
        let len = segment.len();
        let trimmed_cr = segment.strip_suffix('\r').unwrap_or(segment);
        line_offsets.push((pos, pos + trimmed_cr.len(), trimmed_cr));
        pos += len + 1;
    }
    if line_offsets.len() < 2 {
        return None;
    }
    for index in 0..line_offsets.len() - 1 {
        let sep_clean = strip_container_prefix(line_offsets[index + 1].2, quote_depth);
        let Some(alignments) = parse_delimiter_row(&sep_clean) else {
            continue;
        };
        let hdr_clean = strip_container_prefix(line_offsets[index].2, quote_depth);
        if !hdr_clean.contains('|') {
            continue;
        }
        let header_cells = split_table_row(&hdr_clean);
        if header_cells.len() != alignments.len() {
            continue;
        }
        let table_start = line_offsets[index].0;
        let mut end_idx = index + 1;
        for next in (index + 2)..line_offsets.len() {
            let clean = strip_container_prefix(line_offsets[next].2, quote_depth);
            let trimmed = clean.trim();
            if trimmed.is_empty() || is_block_interrupt(trimmed) || !trimmed.contains('|') {
                break;
            }
            end_idx = next;
        }
        return Some((table_start, line_offsets[end_idx].1));
    }
    None
}

fn is_block_interrupt(trimmed: &str) -> bool {
    trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with("~~~")
        || trimmed.starts_with('>')
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
}

pub fn try_extract_html_table(
    source: &str,
    search_from: usize,
    limit: usize,
    path: Option<&Path>,
) -> Option<(usize, usize, Table)> {
    let window_end = limit.min(source.len());
    let slice = &source[search_from..window_end];
    let rel_start = find_html_tag_ci(slice, "table")?;
    let abs_start = search_from + rel_start;
    let abs_end = find_matching_table_end(source, abs_start)?;
    let table = parse_html_table(&source[abs_start..abs_end], path)?;
    Some((abs_start, abs_end, table))
}

fn find_html_tag_ci(haystack: &str, tag: &str) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let tag_bytes = tag.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<!--") {
            if let Some(end) = haystack[i + 4..].find("-->") {
                i += 4 + end + 3;
                continue;
            } else {
                return None;
            }
        }
        if bytes[i] == b'<'
            && i + 1 + tag_bytes.len() <= bytes.len()
            && bytes[i + 1..i + 1 + tag_bytes.len()].eq_ignore_ascii_case(tag_bytes)
        {
            let next = bytes.get(i + 1 + tag_bytes.len()).copied();
            if matches!(
                next,
                Some(b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/') | None
            ) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn find_matching_table_end(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"<!--") {
            if let Some(end) = source[i + 4..].find("-->") {
                i += 4 + end + 3;
                continue;
            } else {
                return Some(bytes.len());
            }
        }
        if bytes[i] == b'<' {
            if bytes[i..].len() >= 6
                && bytes[i + 1..i + 6].eq_ignore_ascii_case(b"table")
                && matches!(
                    bytes.get(i + 6).copied(),
                    Some(b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/')
                )
            {
                if let Some(gt) = find_tag_close(bytes, i + 6) {
                    depth += 1;
                    i = gt + 1;
                    continue;
                }
            } else if bytes[i..].len() >= 7
                && bytes[i + 1..i + 7].eq_ignore_ascii_case(b"/table")
                && matches!(
                    bytes.get(i + 7).copied(),
                    Some(b'>' | b' ' | b'\t' | b'\r' | b'\n')
                )
            {
                if let Some(gt) = find_tag_close(bytes, i + 7) {
                    depth = depth.saturating_sub(1);
                    i = gt + 1;
                    if depth == 0 {
                        return Some(i);
                    }
                    continue;
                }
            }
        }
        i += 1;
    }
    None
}

fn find_tag_close(bytes: &[u8], mut i: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => {
                if b == b'"' || b == b'\'' {
                    quote = Some(b);
                } else if b == b'>' {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

#[derive(Clone, Default)]
struct HtmlInlineState {
    bold: usize,
    italic: usize,
    strike: usize,
    code: usize,
    link: Option<crate::doc::Link>,
}

struct HtmlCellDraft {
    header: bool,
    align: Option<CellAlign>,
    colspan: usize,
    rowspan: usize,
    runs: Vec<Run>,
}

struct HtmlRowDraft {
    cells: Vec<HtmlCellDraft>,
}

pub fn parse_html_table(html: &str, path: Option<&Path>) -> Option<Table> {
    let mut rows: Vec<HtmlRowDraft> = Vec::new();
    let mut in_thead = false;
    let mut current_row: Option<(Option<CellAlign>, Vec<HtmlCellDraft>)> = None;
    let mut current_cell: Option<(HtmlCellDraft, HtmlInlineState)> = None;
    let mut table_depth = 0usize;
    let mut i = 0usize;
    let bytes = html.as_bytes();

    while i < bytes.len() {
        if bytes[i..].starts_with(b"<!--") {
            if let Some(end) = html[i + 4..].find("-->") {
                i += 4 + end + 3;
                continue;
            } else {
                break;
            }
        }
        if bytes[i] == b'<' {
            if let Some(gt) = find_tag_close(bytes, i + 1) {
                let raw_tag = &html[i + 1..gt];
                let trimmed = raw_tag.trim();
                let is_closing = trimmed.starts_with('/');
                let body = trimmed.strip_prefix('/').unwrap_or(trimmed).trim();
                let self_closing = body.ends_with('/');
                let body = body.strip_suffix('/').unwrap_or(body).trim();
                let (name, attrs) = split_tag_name_attrs(body);
                let lower = name.to_ascii_lowercase();

                match lower.as_str() {
                    "table" => {
                        if is_closing {
                            table_depth = table_depth.saturating_sub(1);
                            if table_depth == 0 {
                                break;
                            }
                        } else {
                            table_depth += 1;
                            if table_depth > 1 {
                                if let Some((cell, state)) = current_cell.as_mut() {
                                    push_html_text(&mut cell.runs, " ", state);
                                }
                            }
                        }
                    }
                    "thead" if table_depth == 1 => {
                        in_thead = !is_closing;
                    }
                    "tbody" | "tfoot" if table_depth == 1 => {
                        if !is_closing {
                            in_thead = false;
                        }
                    }
                    "tr" if table_depth == 1 => {
                        if is_closing {
                            if let Some((cell, _)) = current_cell.take() {
                                if let Some((_, ref mut row_cells)) = current_row {
                                    row_cells.push(cell);
                                }
                            }
                            if let Some((_, row_cells)) = current_row.take() {
                                if !row_cells.is_empty() {
                                    rows.push(HtmlRowDraft { cells: row_cells });
                                }
                            }
                        } else {
                            if let Some((cell, _)) = current_cell.take() {
                                if let Some((_, ref mut row_cells)) = current_row {
                                    row_cells.push(cell);
                                }
                            }
                            if let Some((_, row_cells)) = current_row.take() {
                                if !row_cells.is_empty() {
                                    rows.push(HtmlRowDraft { cells: row_cells });
                                }
                            }
                            current_row = Some((parse_html_align(attrs), Vec::new()));
                        }
                    }
                    "th" | "td" if table_depth == 1 => {
                        if is_closing {
                            if let Some((mut cell, _)) = current_cell.take() {
                                trim_runs_whitespace(&mut cell.runs);
                                if let Some((_, ref mut row_cells)) = current_row {
                                    row_cells.push(cell);
                                }
                            }
                        } else {
                            if let Some((mut cell, _)) = current_cell.take() {
                                trim_runs_whitespace(&mut cell.runs);
                                if let Some((_, ref mut row_cells)) = current_row {
                                    row_cells.push(cell);
                                }
                            }
                            if current_row.is_none() {
                                current_row = Some((None, Vec::new()));
                            }
                            let row_align = current_row.as_ref().and_then(|(a, _)| *a);
                            let header = lower == "th" || in_thead;
                            let colspan = parse_html_span(attrs, "colspan", 64);
                            let rowspan = parse_html_span(attrs, "rowspan", 256);
                            let align = parse_html_align(attrs).or(row_align);
                            let draft = HtmlCellDraft {
                                header,
                                align,
                                colspan,
                                rowspan,
                                runs: Vec::new(),
                            };
                            if self_closing {
                                if let Some((_, ref mut row_cells)) = current_row {
                                    row_cells.push(draft);
                                }
                            } else {
                                current_cell = Some((draft, HtmlInlineState::default()));
                            }
                        }
                    }
                    _ if table_depth >= 1 => {
                        if let Some((cell, state)) = current_cell.as_mut() {
                            match lower.as_str() {
                                "br" => push_html_text(&mut cell.runs, "\n", state),
                                "p" | "div" => {
                                    if is_closing && !cell.runs.is_empty() {
                                        push_html_text(&mut cell.runs, "\n", state);
                                    }
                                }
                                "li" => {
                                    if !is_closing {
                                        if !cell.runs.is_empty()
                                            && !cell.runs.last().is_some_and(|r| r.text.ends_with('\n'))
                                        {
                                            push_html_text(&mut cell.runs, "\n", state);
                                        }
                                        push_html_text(&mut cell.runs, "• ", state);
                                    }
                                }
                                "b" | "strong" => {
                                    if is_closing {
                                        state.bold = state.bold.saturating_sub(1);
                                    } else if !self_closing {
                                        state.bold += 1;
                                    }
                                }
                                "i" | "em" => {
                                    if is_closing {
                                        state.italic = state.italic.saturating_sub(1);
                                    } else if !self_closing {
                                        state.italic += 1;
                                    }
                                }
                                "s" | "del" | "strike" => {
                                    if is_closing {
                                        state.strike = state.strike.saturating_sub(1);
                                    } else if !self_closing {
                                        state.strike += 1;
                                    }
                                }
                                "code" | "kbd" | "samp" => {
                                    if is_closing {
                                        state.code = state.code.saturating_sub(1);
                                    } else if !self_closing {
                                        state.code += 1;
                                    }
                                }
                                "a" => {
                                    if is_closing {
                                        state.link = None;
                                    } else if !self_closing {
                                        if let Some(href) = get_attr(attrs, "href") {
                                            let decoded = decode_html_entities(&href);
                                            state.link = resolve_link(&decoded, path);
                                        }
                                    }
                                }
                                "img" => {
                                    let label = get_attr(attrs, "alt")
                                        .map(|a| decode_html_entities(&a))
                                        .unwrap_or_default();
                                    let label = label.trim();
                                    if label.is_empty() {
                                        push_html_text(&mut cell.runs, "[图片]", state);
                                    } else {
                                        push_html_text(
                                            &mut cell.runs,
                                            &format!("[图片：{label}]"),
                                            state,
                                        );
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                i = gt + 1;
                continue;
            }
        }
        let next_tag = html[i..].find('<').map(|offset| i + offset).unwrap_or(bytes.len());
        if next_tag > i {
            if let Some((cell, state)) = current_cell.as_mut() {
                let raw_text = &html[i..next_tag];
                let decoded = decode_html_entities(raw_text);
                let normalized = collapse_html_whitespace(&decoded);
                let after_newline = cell.runs.last().is_some_and(|r| r.text.ends_with('\n'));
                let text = if cell.runs.is_empty() || after_newline {
                    normalized.trim_start_matches(' ')
                } else {
                    normalized.as_str()
                };
                if !text.is_empty() {
                    push_html_text(&mut cell.runs, text, state);
                }
            }
            i = next_tag;
        } else {
            i += 1;
        }
    }

    if let Some((mut cell, _)) = current_cell.take() {
        trim_runs_whitespace(&mut cell.runs);
        if let Some((_, ref mut row_cells)) = current_row {
            row_cells.push(cell);
        }
    }
    if let Some((_, row_cells)) = current_row.take() {
        if !row_cells.is_empty() {
            rows.push(HtmlRowDraft { cells: row_cells });
        }
    }
    if rows.is_empty() {
        return None;
    }

    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut placed: Vec<TableCell> = Vec::new();
    let mut max_cols = 1usize;

    for (row_idx, row) in rows.into_iter().enumerate() {
        if occupied.len() <= row_idx {
            occupied.resize_with(row_idx + 1, Vec::new);
        }
        let mut col_cursor = 0usize;
        for cell in row.cells {
            while occupied[row_idx].get(col_cursor).copied().unwrap_or(false) {
                col_cursor += 1;
            }
            let colspan = cell.colspan.max(1);
            let rowspan = cell.rowspan.max(1);
            max_cols = max_cols.max(col_cursor + colspan);
            if occupied.len() < row_idx + rowspan {
                occupied.resize_with(row_idx + rowspan, Vec::new);
            }
            for r in row_idx..row_idx + rowspan {
                if occupied[r].len() < col_cursor + colspan {
                    occupied[r].resize(col_cursor + colspan, false);
                }
                for c in col_cursor..col_cursor + colspan {
                    occupied[r][c] = true;
                }
            }
            let align = cell.align.unwrap_or(CellAlign::Left);
            let plain = runs_plain(&cell.runs);
            placed.push(TableCell {
                row: row_idx,
                col: col_cursor,
                colspan,
                rowspan,
                header: cell.header,
                align,
                runs: cell.runs,
                plain,
                plain_range: 0..0,
            });
            col_cursor += colspan;
        }
    }

    if placed.is_empty() {
        return None;
    }
    let row_count = occupied.len().max(1);
    for cell in &mut placed {
        if cell.row + cell.rowspan > row_count {
            cell.rowspan = (row_count - cell.row).max(1);
        }
        if cell.col + cell.colspan > max_cols {
            cell.colspan = (max_cols - cell.col).max(1);
        }
    }

    for r in 0..row_count {
        if occupied[r].len() < max_cols {
            occupied[r].resize(max_cols, false);
        }
        let row_is_header = placed
            .iter()
            .any(|c| c.row == r && c.header);
        for c in 0..max_cols {
            if !occupied[r][c] {
                placed.push(TableCell {
                    row: r,
                    col: c,
                    colspan: 1,
                    rowspan: 1,
                    header: row_is_header,
                    align: CellAlign::Left,
                    runs: Vec::new(),
                    plain: String::new(),
                    plain_range: 0..0,
                });
            }
        }
    }

    placed.sort_by_key(|c| (c.row, c.col));
    let alignments = vec![CellAlign::Left; max_cols];
    Some(finalize_table(row_count, max_cols, alignments, placed, true))
}

fn finalize_table(
    rows: usize,
    cols: usize,
    alignments: Vec<CellAlign>,
    mut cells: Vec<TableCell>,
    is_html: bool,
) -> Table {
    cells.sort_by_key(|c| (c.row, c.col));
    let mut plain = String::new();
    let mut has_rowspan = false;
    for cell in &mut cells {
        if cell.rowspan > 1 {
            has_rowspan = true;
        }
        cell.plain = runs_plain(&cell.runs);
        if !plain.is_empty() {
            plain.push('\n');
        }
        let start = plain.len();
        plain.push_str(&cell.plain);
        let end = plain.len();
        cell.plain_range = start..end;
    }
    Table {
        rows,
        cols,
        alignments,
        cells,
        plain,
        has_rowspan,
        is_html,
    }
}

impl Table {
    pub fn refresh_plain(&mut self) {
        self.cells.sort_by_key(|c| (c.row, c.col));
        let mut plain = String::new();
        let mut has_rowspan = false;
        for cell in &mut self.cells {
            if cell.rowspan > 1 {
                has_rowspan = true;
            }
            cell.plain = runs_plain(&cell.runs);
            if !plain.is_empty() {
                plain.push('\n');
            }
            let start = plain.len();
            plain.push_str(&cell.plain);
            let end = plain.len();
            cell.plain_range = start..end;
        }
        self.plain = plain;
        self.has_rowspan = has_rowspan;
    }

    pub fn has_spans(&self) -> bool {
        self.cells.iter().any(|c| c.colspan > 1 || c.rowspan > 1)
    }

    pub fn new_empty(rows: usize, cols: usize) -> Self {
        let rows = rows.max(2);
        let cols = cols.max(1);
        let alignments = vec![CellAlign::Left; cols];
        let mut cells = Vec::with_capacity(rows * cols);
        for r in 0..rows {
            for c in 0..cols {
                cells.push(TableCell {
                    row: r,
                    col: c,
                    colspan: 1,
                    rowspan: 1,
                    header: r == 0,
                    align: CellAlign::Left,
                    runs: Vec::new(),
                    plain: String::new(),
                    plain_range: 0..0,
                });
            }
        }
        finalize_table(rows, cols, alignments, cells, false)
    }

    pub fn insert_row(&mut self, at_row: usize) {
        let at_row = at_row.min(self.rows);
        for cell in &mut self.cells {
            if cell.row >= at_row {
                cell.row += 1;
            } else if cell.row + cell.rowspan > at_row {
                cell.rowspan += 1;
            }
        }
        let mut occupied = vec![false; self.cols];
        for cell in &self.cells {
            if cell.row <= at_row && at_row < cell.row + cell.rowspan {
                for c in cell.col..(cell.col + cell.colspan).min(self.cols) {
                    occupied[c] = true;
                }
            }
        }
        let is_header = at_row == 0 && self.cells.iter().any(|c| c.row == 1 && c.header);
        for c in 0..self.cols {
            if !occupied[c] {
                self.cells.push(TableCell {
                    row: at_row,
                    col: c,
                    colspan: 1,
                    rowspan: 1,
                    header: is_header,
                    align: self.alignments.get(c).copied().unwrap_or(CellAlign::Left),
                    runs: Vec::new(),
                    plain: String::new(),
                    plain_range: 0..0,
                });
            }
        }
        self.rows += 1;
        self.refresh_plain();
    }

    pub fn delete_row(&mut self, target_row: usize) -> bool {
        if self.rows <= 1 || target_row >= self.rows {
            return false;
        }
        let is_header_only = self.cells.iter().filter(|c| c.header).all(|c| c.row == target_row)
            && self.cells.iter().any(|c| c.row == target_row && c.header);
        if is_header_only && self.rows <= 1 {
            return false;
        }
        let mut next_cells = Vec::with_capacity(self.cells.len());
        for mut cell in std::mem::take(&mut self.cells) {
            if cell.row == target_row {
                if cell.rowspan > 1 {
                    cell.rowspan -= 1;
                    next_cells.push(cell);
                }
            } else if cell.row > target_row {
                cell.row -= 1;
                next_cells.push(cell);
            } else {
                if cell.row + cell.rowspan > target_row {
                    cell.rowspan -= 1;
                }
                next_cells.push(cell);
            }
        }
        self.cells = next_cells;
        self.rows -= 1;
        if is_header_only {
            for cell in &mut self.cells {
                if cell.row == 0 {
                    cell.header = true;
                }
            }
        }
        self.refresh_plain();
        true
    }

    pub fn insert_col(&mut self, at_col: usize) {
        let at_col = at_col.min(self.cols);
        self.alignments.insert(at_col, CellAlign::Left);
        for cell in &mut self.cells {
            if cell.col >= at_col {
                cell.col += 1;
            } else if cell.col + cell.colspan > at_col {
                cell.colspan += 1;
            }
        }
        let mut occupied = vec![false; self.rows];
        for cell in &self.cells {
            if cell.col <= at_col && at_col < cell.col + cell.colspan {
                for r in cell.row..(cell.row + cell.rowspan).min(self.rows) {
                    occupied[r] = true;
                }
            }
        }
        let mut row_is_header = vec![false; self.rows];
        for cell in &self.cells {
            if cell.row < self.rows && cell.header {
                row_is_header[cell.row] = true;
            }
        }
        for r in 0..self.rows {
            if !occupied[r] {
                self.cells.push(TableCell {
                    row: r,
                    col: at_col,
                    colspan: 1,
                    rowspan: 1,
                    header: row_is_header[r],
                    align: CellAlign::Left,
                    runs: Vec::new(),
                    plain: String::new(),
                    plain_range: 0..0,
                });
            }
        }
        self.cols += 1;
        self.refresh_plain();
    }

    pub fn delete_col(&mut self, target_col: usize) -> bool {
        if self.cols <= 1 || target_col >= self.cols {
            return false;
        }
        self.alignments.remove(target_col);
        let mut next_cells = Vec::with_capacity(self.cells.len());
        for mut cell in std::mem::take(&mut self.cells) {
            if cell.col == target_col {
                if cell.colspan > 1 {
                    cell.colspan -= 1;
                    next_cells.push(cell);
                }
            } else if cell.col > target_col {
                cell.col -= 1;
                next_cells.push(cell);
            } else {
                if cell.col + cell.colspan > target_col {
                    cell.colspan -= 1;
                }
                next_cells.push(cell);
            }
        }
        self.cells = next_cells;
        self.cols -= 1;
        self.refresh_plain();
        true
    }

    pub fn set_col_align(&mut self, col: usize, align: CellAlign) {
        if col >= self.cols {
            return;
        }
        if let Some(slot) = self.alignments.get_mut(col) {
            *slot = align;
        }
        for cell in &mut self.cells {
            if cell.col == col && cell.colspan == 1 {
                cell.align = align;
            }
        }
    }

    pub fn to_markdown_or_html(&self) -> String {
        if self.is_html || self.has_spans() {
            return self.to_html();
        }
        self.to_markdown()
    }

    pub fn to_markdown(&self) -> String {
        let mut grid = vec![vec![String::new(); self.cols]; self.rows];
        for cell in &self.cells {
            if cell.row < self.rows && cell.col < self.cols {
                grid[cell.row][cell.col] = serialize_cell_runs(&cell.runs);
            }
        }
        let mut out = String::new();
        if let Some(header) = grid.first() {
            out.push('|');
            for cell in header {
                out.push(' ');
                out.push_str(cell);
                out.push_str(" |");
            }
            out.push('\n');
        }
        out.push('|');
        for c in 0..self.cols {
            let sep = match self.alignments.get(c).copied().unwrap_or(CellAlign::Left) {
                CellAlign::Left => " :--- |",
                CellAlign::Center => " :---: |",
                CellAlign::Right => " ---: |",
            };
            out.push_str(sep);
        }
        for row in grid.iter().skip(1) {
            out.push('\n');
            out.push('|');
            for cell in row {
                out.push(' ');
                out.push_str(cell);
                out.push_str(" |");
            }
        }
        out
    }

    pub fn to_html(&self) -> String {
        let mut out = String::from("<table>\n");
        let mut header_rows = Vec::new();
        let mut body_rows = Vec::new();
        for r in 0..self.rows {
            let row_cells: Vec<&TableCell> = self.cells.iter().filter(|c| c.row == r).collect();
            if row_cells.iter().any(|c| c.header) {
                header_rows.push(row_cells);
            } else {
                body_rows.push(row_cells);
            }
        }
        if !header_rows.is_empty() {
            out.push_str("  <thead>\n");
            for row in header_rows {
                write_html_row(&mut out, &row, true);
            }
            out.push_str("  </thead>\n");
        }
        if !body_rows.is_empty() {
            out.push_str("  <tbody>\n");
            for row in body_rows {
                write_html_row(&mut out, &row, false);
            }
            out.push_str("  </tbody>\n");
        }
        out.push_str("</table>");
        out
    }
}

fn write_html_row(out: &mut String, row: &[&TableCell], in_head: bool) {
    out.push_str("    <tr>");
    for cell in row {
        let tag = if cell.header || in_head { "th" } else { "td" };
        out.push('<');
        out.push_str(tag);
        if cell.colspan > 1 {
            out.push_str(&format!(" colspan=\"{}\"", cell.colspan));
        }
        if cell.rowspan > 1 {
            out.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
        }
        match cell.align {
            CellAlign::Center => out.push_str(" align=\"center\""),
            CellAlign::Right => out.push_str(" align=\"right\""),
            CellAlign::Left => {}
        }
        out.push('>');
        out.push_str(&serialize_cell_runs_html(&cell.runs));
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
    }
    out.push_str("</tr>\n");
}

fn serialize_cell_runs(runs: &[Run]) -> String {
    let md = crate::doc::serialize_runs(runs);
    escape_table_pipes(&md).replace('\n', "<br>")
}

fn escape_table_pipes(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let ticks = count_run(bytes, i, b'`');
            if let Some(close_end) = find_closing_ticks(bytes, i + ticks, ticks) {
                out.push_str(&text[i..close_end]);
                i = close_end;
                continue;
            }
            out.push_str(&text[i..i + ticks]);
            i += ticks;
            continue;
        }
        if bytes[i] == b'|' {
            out.push_str("\\|");
            i += 1;
            continue;
        }
        let Some(ch) = text[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn serialize_cell_runs_html(runs: &[Run]) -> String {
    let mut out = String::new();
    for run in runs {
        let mut piece = escape_html_text(&run.text).replace('\n', "<br>");
        if run.code {
            piece = format!("<code>{piece}</code>");
        }
        if run.bold {
            piece = format!("<b>{piece}</b>");
        }
        if run.italic {
            piece = format!("<i>{piece}</i>");
        }
        if run.strike {
            piece = format!("<s>{piece}</s>");
        }
        if let Some(link) = &run.link {
            let href = escape_html_attr(link.raw_url());
            piece = format!("<a href=\"{href}\">{piece}</a>");
        }
        out.push_str(&piece);
    }
    out
}

fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_attr(text: &str) -> String {
    escape_html_text(text).replace('"', "&quot;")
}

fn strip_container_prefix(line: &str, quote_depth: usize) -> String {
    let mut rest = line.trim_end_matches('\r');
    for _ in 0..quote_depth {
        let trimmed = rest.trim_start_matches(' ');
        if let Some(after) = trimmed.strip_prefix('>') {
            rest = after.strip_prefix(' ').unwrap_or(after);
        } else {
            break;
        }
    }
    rest.to_string()
}

pub fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim_matches(|c| c == ' ' || c == '\t' || c == '\r');
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut pipes = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if i + 1 < bytes.len() {
                    if bytes[i + 1] == b'\\' && bytes.get(i + 2) == Some(&b'|') {
                        i += 2;
                        continue;
                    }
                    if bytes[i + 1] == b'|' || bytes[i + 1] == b'`' || bytes[i + 1].is_ascii_punctuation() {
                        i += 2;
                        continue;
                    }
                }
                i += 1;
            }
            b'`' => {
                let ticks = count_run(bytes, i, b'`');
                if let Some(close_end) = find_closing_ticks(bytes, i + ticks, ticks) {
                    i = close_end;
                } else {
                    i += ticks;
                }
            }
            b'|' => {
                pipes.push(i);
                i += 1;
            }
            _ => i += 1,
        }
    }

    if pipes.is_empty() {
        return vec![trimmed.trim().to_string()];
    }

    let has_leading = pipes.first() == Some(&0);
    let has_trailing = pipes.last() == Some(&(bytes.len() - 1)) && !(has_leading && pipes.len() == 1);
    let mut cells = Vec::new();
    let mut start = if has_leading { 1 } else { 0 };
    let inner_pipes = if has_leading { &pipes[1..] } else { &pipes[..] };
    for &pipe in inner_pipes {
        cells.push(trimmed[start..pipe].trim().to_string());
        start = pipe + 1;
    }
    if !has_trailing {
        cells.push(trimmed[start..].trim().to_string());
    }
    cells
}

pub fn parse_delimiter_row(line: &str) -> Option<Vec<CellAlign>> {
    let trimmed = line.trim_matches(|c| c == ' ' || c == '\t' || c == '\r');
    if !trimmed.contains('|') {
        return None;
    }
    let cells = split_table_row(trimmed);
    if cells.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(cells.len());
    for cell in cells {
        let token = cell.trim();
        if token.is_empty() {
            return None;
        }
        let left = token.starts_with(':');
        let right = token.ends_with(':');
        let inner = token
            .strip_prefix(':')
            .unwrap_or(token)
            .strip_suffix(':')
            .unwrap_or_else(|| token.strip_prefix(':').unwrap_or(token));
        if inner.is_empty() || !inner.bytes().all(|b| b == b'-') {
            return None;
        }
        let align = match (left, right) {
            (true, true) => CellAlign::Center,
            (false, true) => CellAlign::Right,
            _ => CellAlign::Left,
        };
        out.push(align);
    }
    Some(out)
}

fn count_run(bytes: &[u8], start: usize, needle: u8) -> usize {
    let mut count = 0usize;
    while start + count < bytes.len() && bytes[start + count] == needle {
        count += 1;
    }
    count
}

fn find_closing_ticks(bytes: &[u8], mut i: usize, target: usize) -> Option<usize> {
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let len = count_run(bytes, i, b'`');
            if len == target {
                return Some(i + len);
            }
            i += len;
        } else {
            i += 1;
        }
    }
    None
}

fn normalize_code_pipes(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'`' {
            out.push_str(&raw[i..i + 2]);
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            let ticks = count_run(bytes, i, b'`');
            if let Some(close_end) = find_closing_ticks(bytes, i + ticks, ticks) {
                let inner = &raw[i + ticks..close_end - ticks];
                out.push_str(&raw[i..i + ticks]);
                out.push_str(&unescape_code_span_pipes(inner));
                out.push_str(&raw[close_end - ticks..close_end]);
                i = close_end;
                continue;
            } else {
                out.push_str(&raw[i..i + ticks]);
                i += ticks;
                continue;
            }
        }
        let Some(ch) = raw[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn unescape_code_span_pipes(inner: &str) -> String {
    let bytes = inner.as_bytes();
    let mut out = String::with_capacity(inner.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let slashes = count_run(bytes, i, b'\\');
            if bytes.get(i + slashes) == Some(&b'|') && slashes % 2 == 1 {
                for _ in 0..(slashes - 1) {
                    out.push('\\');
                }
                out.push('|');
                i += slashes + 1;
                continue;
            }
            out.push_str(&inner[i..i + slashes]);
            i += slashes;
            continue;
        }
        let Some(ch) = inner[i..].chars().next() else {
            break;
        };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn parse_cell_inline(raw: &str, path: Option<&Path>) -> Vec<Run> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let normalized = normalize_code_pipes(trimmed);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS);

    let mut runs: Vec<Run> = Vec::new();
    let mut state = HtmlInlineState::default();
    let mut image_alt: Option<String> = None;

    for event in Parser::new_ext(&normalized, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => state.bold += 1,
                Tag::Emphasis => state.italic += 1,
                Tag::Strikethrough => state.strike += 1,
                Tag::Link { dest_url, .. } => state.link = resolve_link(&dest_url, path),
                Tag::Image { .. } => image_alt = Some(String::new()),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Strong => state.bold = state.bold.saturating_sub(1),
                TagEnd::Emphasis => state.italic = state.italic.saturating_sub(1),
                TagEnd::Strikethrough => state.strike = state.strike.saturating_sub(1),
                TagEnd::Link => state.link = None,
                TagEnd::Image => {
                    if let Some(alt) = image_alt.take() {
                        let label = alt.trim();
                        if label.is_empty() {
                            push_html_text(&mut runs, "[图片]", &state);
                        } else {
                            push_html_text(&mut runs, &format!("[图片：{label}]"), &state);
                        }
                    }
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some(alt) = image_alt.as_mut() {
                    alt.push_str(&text);
                } else {
                    push_html_text(&mut runs, &text, &state);
                }
            }
            Event::Code(text) => {
                let mut code_state = state.clone();
                code_state.code += 1;
                push_html_text(&mut runs, &text, &code_state);
            }
            Event::SoftBreak => push_html_text(&mut runs, " ", &state),
            Event::HardBreak => push_html_text(&mut runs, "\n", &state),
            Event::TaskListMarker(checked) => {
                push_html_text(&mut runs, if checked { "[x] " } else { "[ ] " }, &state);
            }
            Event::InlineHtml(html) | Event::Html(html) => {
                apply_inline_html(&html, &mut runs, &mut state, path);
            }
            _ => {}
        }
    }
    trim_runs_whitespace(&mut runs);
    runs
}

pub fn is_br_tag(raw: &str) -> bool {
    let trimmed = raw.trim();
    let Some(inner) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) else {
        return false;
    };
    let inner = inner.trim().strip_suffix('/').unwrap_or(inner.trim()).trim();
    if inner.len() < 2 {
        return false;
    }
    let (name, rest) = split_tag_name_attrs(inner);
    name.eq_ignore_ascii_case("br") && (rest.is_empty() || rest.starts_with(char::is_whitespace))
}

fn apply_inline_html(
    html: &str,
    runs: &mut Vec<Run>,
    state: &mut HtmlInlineState,
    path: Option<&Path>,
) {
    if is_br_tag(html) {
        push_html_text(runs, "\n", state);
        return;
    }
    let trimmed = html.trim();
    let Some(inner) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) else {
        return;
    };
    let inner = inner.trim();
    let is_closing = inner.starts_with('/');
    let body = inner.strip_prefix('/').unwrap_or(inner).trim();
    let self_closing = body.ends_with('/');
    let body = body.strip_suffix('/').unwrap_or(body).trim();
    let (name, attrs) = split_tag_name_attrs(body);
    match name.to_ascii_lowercase().as_str() {
        "b" | "strong" => {
            if is_closing {
                state.bold = state.bold.saturating_sub(1);
            } else if !self_closing {
                state.bold += 1;
            }
        }
        "i" | "em" => {
            if is_closing {
                state.italic = state.italic.saturating_sub(1);
            } else if !self_closing {
                state.italic += 1;
            }
        }
        "s" | "del" | "strike" => {
            if is_closing {
                state.strike = state.strike.saturating_sub(1);
            } else if !self_closing {
                state.strike += 1;
            }
        }
        "code" | "kbd" | "samp" => {
            if is_closing {
                state.code = state.code.saturating_sub(1);
            } else if !self_closing {
                state.code += 1;
            }
        }
        "a" => {
            if is_closing {
                state.link = None;
            } else if !self_closing {
                if let Some(href) = get_attr(attrs, "href") {
                    let decoded = decode_html_entities(&href);
                    state.link = resolve_link(&decoded, path);
                }
            }
        }
        _ => {}
    }
}

fn push_html_text(runs: &mut Vec<Run>, text: &str, state: &HtmlInlineState) {
    if text.is_empty() {
        return;
    }
    let run = Run {
        text: text.to_string(),
        bold: state.bold > 0,
        italic: state.italic > 0,
        code: state.code > 0,
        strike: state.strike > 0,
        link: state.link.clone(),
    };
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

fn trim_runs_whitespace(runs: &mut Vec<Run>) {
    while let Some(first) = runs.first_mut() {
        let trimmed = first.text.trim_start_matches([' ', '\t', '\n', '\r']);
        if trimmed.len() != first.text.len() {
            first.text = trimmed.to_string();
        }
        if first.text.is_empty() {
            runs.remove(0);
        } else {
            break;
        }
    }
    while let Some(last) = runs.last_mut() {
        let trimmed = last.text.trim_end_matches([' ', '\t', '\n', '\r']);
        if trimmed.len() != last.text.len() {
            last.text = trimmed.to_string();
        }
        if last.text.is_empty() {
            runs.pop();
        } else {
            break;
        }
    }
}

fn runs_plain(runs: &[Run]) -> String {
    runs.iter().map(|run| run.text.as_str()).collect()
}

fn split_tag_name_attrs(body: &str) -> (&str, &str) {
    match body.find(|c: char| c.is_ascii_whitespace()) {
        Some(pos) => (&body[..pos], body[pos..].trim()),
        None => (body, ""),
    }
}

fn get_attr(attrs: &str, name: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let key_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
        {
            i += 1;
        }
        let key = &attrs[key_start..i];
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
            let quote = bytes[i];
            i += 1;
            let val_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let slice = &attrs[val_start..i];
            if i < bytes.len() {
                i += 1;
            }
            slice
        } else {
            let val_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            &attrs[val_start..i]
        };
        if key.eq_ignore_ascii_case(name) {
            return Some(val.to_string());
        }
    }
    None
}

fn parse_html_span(attrs: &str, attr_name: &str, max: usize) -> usize {
    get_attr(attrs, attr_name)
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|n| n.clamp(1, max))
        .unwrap_or(1)
}

fn parse_html_align(attrs: &str) -> Option<CellAlign> {
    if let Some(align) = get_attr(attrs, "align") {
        match align.trim().to_ascii_lowercase().as_str() {
            "left" => return Some(CellAlign::Left),
            "center" => return Some(CellAlign::Center),
            "right" => return Some(CellAlign::Right),
            _ => {}
        }
    }
    if let Some(style) = get_attr(attrs, "style") {
        let lower = style.to_ascii_lowercase();
        for decl in lower.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                if k.trim() == "text-align" {
                    return match v.trim() {
                        "left" => Some(CellAlign::Left),
                        "center" => Some(CellAlign::Center),
                        "right" => Some(CellAlign::Right),
                        _ => None,
                    };
                }
            }
        }
    }
    None
}

fn collapse_html_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = false;
    for ch in text.chars() {
        if ch == '\n' || ch == '\r' || ch == '\t' || ch == ' ' {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

fn decode_html_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            if semi > 0 && semi <= 12 {
                let entity = &after[..semi];
                if let Some(decoded) = resolve_entity(entity) {
                    out.push_str(&decoded);
                    rest = &after[semi + 1..];
                    continue;
                }
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn resolve_entity(entity: &str) -> Option<String> {
    if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
        let code = u32::from_str_radix(hex, 16).ok()?;
        return char::from_u32(code).map(|c| c.to_string());
    }
    if let Some(dec) = entity.strip_prefix('#') {
        let code = dec.parse::<u32>().ok()?;
        return char::from_u32(code).map(|c| c.to_string());
    }
    match entity {
        "amp" => Some(String::from("&")),
        "lt" => Some(String::from("<")),
        "gt" => Some(String::from(">")),
        "quot" => Some(String::from("\"")),
        "apos" => Some(String::from("'")),
        "nbsp" => Some(String::from("\u{00A0}")),
        "mdash" => Some(String::from("—")),
        "ndash" => Some(String::from("–")),
        "hellip" => Some(String::from("…")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_rows_with_code_pipes_and_escaped_pipes() {
        let cells = split_table_row(r"| `a | b` | c \| d | ``e | f`` | \\| tail |");
        assert_eq!(
            cells,
            vec![r"`a | b`", r"c \| d", r"``e | f``", r"\\", r"tail"]
        );
    }

    #[test]
    fn parses_gfm_without_outer_pipes_and_empty_first_cell() {
        let md = "左 | 中 | 右\n:--- | :---: | ---:\n| | 第二 | 第三\n| | |\n| A | B | C | 多余列 |\n| 缺列 |";
        let tbl = parse_gfm_table(md, 0, None).expect("table");
        assert_eq!(tbl.cols, 4);
        assert_eq!(tbl.rows, 5);
        assert_eq!(
            tbl.alignments,
            vec![
                CellAlign::Left,
                CellAlign::Center,
                CellAlign::Right,
                CellAlign::Left
            ]
        );
        assert_eq!(tbl.cells[4].plain, "");
        assert_eq!(tbl.cells[5].plain, "第二");
        assert_eq!(tbl.cells[6].plain, "第三");
        assert_eq!(tbl.cells[8].plain, "");
        assert_eq!(tbl.cells[15].plain, "多余列");
        assert_eq!(tbl.cells[16].plain, "缺列");
        assert_eq!(tbl.cells[17].plain, "");
    }

    #[test]
    fn parses_inline_code_pipes_br_and_links() {
        let md = "| 用法 | 示例 |\n| --- | --- |\n| 管道 | `a | b` 与 `c \\| d` |\n| 换行 | 第一行<br>第二行<br />[官网](https://example.test) |";
        let tbl = parse_gfm_table(md, 0, None).expect("table");
        assert_eq!(tbl.cols, 2);
        assert_eq!(tbl.cells[3].plain, "a | b 与 c | d");
        assert!(tbl.cells[3].runs.iter().any(|r| r.code && r.text == "a | b"));
        assert!(tbl.cells[3].runs.iter().any(|r| r.code && r.text == "c | d"));
        assert_eq!(tbl.cells[5].plain, "第一行\n第二行\n官网");
        assert!(tbl.cells[5].runs.iter().any(|r| r.link.is_some() && r.text == "官网"));
    }

    #[test]
    fn parses_html_table_with_colspan_and_rowspan() {
        let html = r#"
        <table>
          <thead>
            <tr><th colspan="2" align="center">总览</th><th>备注</th></tr>
          </thead>
          <tbody>
            <tr><td rowspan="2">分组A</td><td><b>子项1</b></td><td>line1<br>line2</td></tr>
            <tr><td><code>k=v</code></td><td><a href="https://example.test">link</a></td></tr>
          </tbody>
        </table>"#;
        let tbl = parse_html_table(html, None).expect("html table");
        assert_eq!(tbl.rows, 3);
        assert_eq!(tbl.cols, 3);
        assert!(tbl.has_rowspan);
        assert_eq!(tbl.cells[0].colspan, 2);
        assert_eq!(tbl.cells[0].align, CellAlign::Center);
        assert_eq!(tbl.cells[2].row, 1);
        assert_eq!(tbl.cells[2].col, 0);
        assert_eq!(tbl.cells[2].rowspan, 2);
        assert_eq!(tbl.cells[3].col, 1);
        assert_eq!(tbl.cells[4].plain, "line1\nline2");
        assert_eq!(tbl.cells[5].row, 2);
        assert_eq!(tbl.cells[5].col, 1);
        assert!(tbl.cells[5].runs.iter().any(|r| r.code && r.text == "k=v"));
        assert!(tbl.cells[6].runs.iter().any(|r| r.link.is_some() && r.text == "link"));
    }
}
