//! G02: the gallery grid engine — pure, UI-free, and shared by the editor
//! canvas, the template picker, breakpoint preview, and the HTML renderer.
//!
//! A gallery is a CSS-style grid of `columns` equal columns; rows are
//! `column width / ratio` tall. Each placed photo occupies a rectangle of
//! whole cells (`col`, `row`, `span_x`, `span_y`). Nothing overlaps and no
//! photo is ever lost: every operation re-flows displaced photos into the
//! next free cells in reading order.

use serde::{Deserialize, Serialize};

/// A placed rectangle (0-based column/row, spans ≥ 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cell {
    pub col: u16,
    pub row: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    Uniform,
    Editorial,
    SingleColumn,
    ContactSheet,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Uniform => "Uniform grid",
            Category::Editorial => "Editorial",
            Category::SingleColumn => "Single column",
            Category::ContactSheet => "Contact sheet",
        }
    }
}

/// One of the eight handoff layouts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: Category,
    pub columns: u8,
    pub gutter: u8,
    /// Row height = column width / ratio.
    pub ratio: f32,
    /// Filmstrip pages scroll sideways row by row.
    pub scroll_rows: bool,
    /// Captions under tiles by default.
    pub captions: bool,
}

impl Template {
    /// Default span for the photo at `index` (clamped to `columns`).
    pub fn span_for(&self, index: usize, columns: u8) -> (u8, u8) {
        let c = columns.max(1);
        let (x, y) = match self.id {
            // Every sixth photo leads a 2×2 block (with three columns the
            // five 1×1s after it fill the rows exactly — no holes).
            "mixed" => {
                if index % 6 == 0 {
                    (2, 2)
                } else {
                    (1, 1)
                }
            }
            // Wide, narrow / narrow, wide.
            "editorial" => match index % 4 {
                0 | 3 => (2, 1),
                _ => (1, 1),
            },
            "masonry" => match index % 4 {
                0 | 3 => (1, 2),
                _ => (1, 1),
            },
            "hero" => {
                if index == 0 {
                    (c, 2)
                } else {
                    (1, 1)
                }
            }
            _ => (1, 1),
        };
        (x.min(c), y)
    }
}

pub const TEMPLATES: [Template; 8] = [
    Template {
        id: "mixed",
        name: "Mixed spans",
        description: "3 columns, variable spans",
        category: Category::Uniform,
        columns: 3,
        gutter: 12,
        ratio: 1.5,
        scroll_rows: false,
        captions: false,
    },
    Template {
        id: "square",
        name: "Square grid",
        description: "3 columns, 1:1",
        category: Category::Uniform,
        columns: 3,
        gutter: 12,
        ratio: 1.0,
        scroll_rows: false,
        captions: false,
    },
    Template {
        id: "editorial",
        name: "Editorial rows",
        description: "Alternating widths",
        category: Category::Editorial,
        columns: 3,
        gutter: 16,
        ratio: 1.5,
        scroll_rows: false,
        captions: true,
    },
    Template {
        id: "single",
        name: "Single column",
        description: "Centered, captions below",
        category: Category::SingleColumn,
        columns: 1,
        gutter: 28,
        ratio: 1.5,
        scroll_rows: false,
        captions: true,
    },
    Template {
        id: "contact",
        name: "Contact sheet",
        description: "4 columns, tight gutter",
        category: Category::ContactSheet,
        columns: 4,
        gutter: 4,
        ratio: 1.5,
        scroll_rows: false,
        captions: false,
    },
    Template {
        id: "masonry",
        name: "Masonry pairs",
        description: "2 columns, mixed ratio",
        category: Category::Editorial,
        columns: 2,
        gutter: 18,
        ratio: 1.25,
        scroll_rows: false,
        captions: true,
    },
    Template {
        id: "hero",
        name: "Hero + grid",
        description: "Lead image, then 3 columns",
        category: Category::Editorial,
        columns: 3,
        gutter: 12,
        ratio: 1.5,
        scroll_rows: false,
        captions: false,
    },
    Template {
        id: "filmstrip",
        name: "Filmstrip",
        description: "Horizontal scroll rows",
        category: Category::Uniform,
        columns: 4,
        gutter: 8,
        ratio: 1.5,
        scroll_rows: true,
        captions: false,
    },
];

pub fn template(id: &str) -> &'static Template {
    TEMPLATES
        .iter()
        .find(|t| t.id == id)
        .unwrap_or(&TEMPLATES[0])
}

/// What the engine needs to know about one photo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub span_x: u8,
    pub span_y: u8,
    pub cell: Option<Cell>,
}

/// Occupancy grid that grows downward as needed.
struct Occupancy {
    columns: usize,
    taken: Vec<bool>,
}

impl Occupancy {
    fn new(columns: u8) -> Self {
        Self {
            columns: columns.max(1) as usize,
            taken: Vec::new(),
        }
    }

    fn fits(&self, col: usize, row: usize, sx: usize, sy: usize) -> bool {
        if col + sx > self.columns {
            return false;
        }
        for r in row..row + sy {
            for c in col..col + sx {
                if self.taken.get(r * self.columns + c).copied().unwrap_or(false) {
                    return false;
                }
            }
        }
        true
    }

    fn mark(&mut self, col: usize, row: usize, sx: usize, sy: usize) {
        let need = (row + sy) * self.columns;
        if self.taken.len() < need {
            self.taken.resize(need, false);
        }
        for r in row..row + sy {
            for c in col..col + sx {
                self.taken[r * self.columns + c] = true;
            }
        }
    }

    /// First free position at or after reading index `from`.
    fn next_fit(&self, from: usize, sx: usize, sy: usize) -> (usize, usize) {
        let sx = sx.min(self.columns).max(1);
        let mut i = from;
        loop {
            let (col, row) = (i % self.columns, i / self.columns);
            if self.fits(col, row, sx, sy.max(1)) {
                return (col, row);
            }
            i += 1;
        }
    }
}

fn clamp_span(s: &Slot, columns: u8) -> (usize, usize) {
    (
        (s.span_x.max(1)).min(columns.max(1)) as usize,
        s.span_y.clamp(1, 8) as usize,
    )
}

/// Place every slot in order: keep a slot's existing cell when it fits
/// (after the ones before it), otherwise the next free cell from where it
/// wanted to be. Slots with no cell flow after the last placed one.
/// Returns one cell per slot, in the same order.
pub fn flow(slots: &[Slot], columns: u8) -> Vec<Cell> {
    let mut occ = Occupancy::new(columns);
    let cols = occ.columns;
    let mut cursor = 0usize;
    let mut out = Vec::with_capacity(slots.len());
    for s in slots {
        let (sx, sy) = clamp_span(s, columns);
        let want = s
            .cell
            .map(|c| (c.col as usize).min(cols - 1) + c.row as usize * cols);
        let (col, row) = match want {
            Some(w) => {
                let (c, r) = (w % cols, w / cols);
                if occ.fits(c, r, sx, sy) {
                    (c, r)
                } else {
                    occ.next_fit(w, sx, sy)
                }
            }
            None => occ.next_fit(cursor, sx, sy),
        };
        occ.mark(col, row, sx, sy);
        cursor = cursor.max(row * cols + col);
        out.push(Cell {
            col: col as u16,
            row: row as u16,
        });
    }
    out
}

/// Reading order (row, then column) of placed slots; ties keep list order.
pub fn reading_order(cells: &[Option<Cell>]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..cells.len()).filter(|i| cells[*i].is_some()).collect();
    idx.sort_by_key(|i| {
        let c = cells[*i].expect("filtered");
        (c.row, c.col, *i)
    });
    idx
}

/// Put `target` at `cell` (clamped into the grid) and re-flow the other
/// placed slots around it in reading order. Unplaced slots stay unplaced.
pub fn place_at(slots: &[Slot], target: usize, cell: Cell, columns: u8) -> Vec<Option<Cell>> {
    let cols = columns.max(1);
    let mut work: Vec<Slot> = slots.to_vec();
    let (sx, _) = clamp_span(&work[target], cols);
    let col = (cell.col as usize).min(cols as usize - sx) as u16;
    work[target].cell = Some(Cell { col, row: cell.row });
    reflow_fixed(&work, target, cols)
}

/// Change a slot's span; it keeps its cell when the new size fits there,
/// otherwise it moves to the next fit. Siblings re-flow.
pub fn set_span(slots: &[Slot], target: usize, span_x: u8, span_y: u8, columns: u8) -> Vec<Option<Cell>> {
    let cols = columns.max(1);
    let mut work: Vec<Slot> = slots.to_vec();
    work[target].span_x = span_x.clamp(1, cols);
    work[target].span_y = span_y.clamp(1, 8);
    if work[target].cell.is_none() {
        return work.iter().map(|s| s.cell).collect();
    }
    let c = work[target].cell.expect("placed");
    let col = (c.col as usize).min(cols as usize - work[target].span_x as usize) as u16;
    work[target].cell = Some(Cell { col, row: c.row });
    reflow_fixed(&work, target, cols)
}

/// The target stays exactly where it is; everyone else placed re-flows in
/// reading order around it.
fn reflow_fixed(work: &[Slot], target: usize, columns: u8) -> Vec<Option<Cell>> {
    let mut occ = Occupancy::new(columns);
    let cols = occ.columns;
    let t = work[target];
    let tc = t.cell.expect("target placed");
    let (tsx, tsy) = clamp_span(&t, columns);
    occ.mark(tc.col as usize, tc.row as usize, tsx, tsy);
    let mut out: Vec<Option<Cell>> = work.iter().map(|s| s.cell).collect();
    let cells: Vec<Option<Cell>> = work.iter().map(|s| s.cell).collect();
    for i in reading_order(&cells) {
        if i == target {
            continue;
        }
        let s = work[i];
        let (sx, sy) = clamp_span(&s, columns);
        let c = s.cell.expect("placed");
        let want = (c.col as usize).min(cols - 1) + c.row as usize * cols;
        let (col, row) = if occ.fits(want % cols, want / cols, sx, sy) {
            (want % cols, want / cols)
        } else {
            occ.next_fit(want, sx, sy)
        };
        occ.mark(col, row, sx, sy);
        out[i] = Some(Cell {
            col: col as u16,
            row: row as u16,
        });
    }
    out
}

/// Flow photos in order into a template: spans from the template's rule,
/// cells packed from the top. Every slot is placed.
pub fn apply_template(count: usize, t: &Template, columns: u8) -> Vec<(Cell, u8, u8)> {
    let slots: Vec<Slot> = (0..count)
        .map(|i| {
            let (x, y) = t.span_for(i, columns);
            Slot {
                span_x: x,
                span_y: y,
                cell: None,
            }
        })
        .collect();
    flow(&slots, columns)
        .into_iter()
        .zip(slots)
        .map(|(c, s)| (c, s.span_x.min(columns.max(1)), s.span_y))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Breakpoint {
    #[default]
    Desktop,
    Tablet,
    Phone,
}

impl Breakpoint {
    pub fn label(self) -> &'static str {
        match self {
            Breakpoint::Desktop => "Desktop",
            Breakpoint::Tablet => "Tablet",
            Breakpoint::Phone => "Phone",
        }
    }

    pub fn columns(self, desktop: u8) -> u8 {
        match self {
            Breakpoint::Desktop => desktop.max(1),
            Breakpoint::Tablet => desktop.clamp(1, 2),
            Breakpoint::Phone => 1,
        }
    }

    /// Page width the canvas previews at.
    pub fn page_width(self) -> f32 {
        match self {
            Breakpoint::Desktop => 760.,
            Breakpoint::Tablet => 640.,
            Breakpoint::Phone => 390.,
        }
    }
}

/// Re-flow placed slots (in desktop reading order) for a narrower
/// breakpoint: spans clamp to the column count. The desktop cells are not
/// modified; returns (slot index, cell, span_x, span_y).
pub fn at_breakpoint(slots: &[Slot], desktop_columns: u8, bp: Breakpoint) -> Vec<(usize, Cell, u8, u8)> {
    let cols = bp.columns(desktop_columns);
    let cells: Vec<Option<Cell>> = slots.iter().map(|s| s.cell).collect();
    let order = reading_order(&cells);
    let narrowed: Vec<Slot> = order
        .iter()
        .map(|&i| {
            let s = slots[i];
            let sx = s.span_x.min(cols);
            // A span wider than the new grid keeps its shape roughly.
            let sy = if bp == Breakpoint::Phone { 1 } else { s.span_y.min(2) };
            Slot {
                span_x: sx,
                span_y: sy,
                cell: None,
            }
        })
        .collect();
    flow(&narrowed, cols)
        .into_iter()
        .zip(order)
        .zip(narrowed)
        .map(|((c, i), s)| (i, c, s.span_x, s.span_y))
        .collect()
}

/// Rows the grid needs.
pub fn row_count(cells: &[(Cell, u8, u8)]) -> u16 {
    cells
        .iter()
        .map(|(c, _, sy)| c.row + *sy as u16)
        .max()
        .unwrap_or(0)
}

/// Pixel geometry for a page: (column width, row height).
pub fn metrics(page_inner_w: f32, columns: u8, gutter: f32, ratio: f32) -> (f32, f32) {
    let n = columns.max(1) as f32;
    let col_w = ((page_inner_w - gutter * (n - 1.)) / n).max(1.);
    (col_w, col_w / ratio.max(0.1))
}

/// A tile's rectangle in page px (x, y, w, h).
pub fn tile_rect(cell: Cell, sx: u8, sy: u8, col_w: f32, row_h: f32, gutter: f32) -> (f32, f32, f32, f32) {
    (
        cell.col as f32 * (col_w + gutter),
        cell.row as f32 * (row_h + gutter),
        sx as f32 * col_w + (sx.max(1) - 1) as f32 * gutter,
        sy as f32 * row_h + (sy.max(1) - 1) as f32 * gutter,
    )
}

/// The cell under a page point (clamped to the grid; rows unbounded).
pub fn cell_at(x: f32, y: f32, columns: u8, col_w: f32, row_h: f32, gutter: f32) -> Cell {
    let col = (x.max(0.) / (col_w + gutter)).floor() as i64;
    let row = (y.max(0.) / (row_h + gutter)).floor() as i64;
    Cell {
        col: col.clamp(0, columns.max(1) as i64 - 1) as u16,
        row: row.clamp(0, u16::MAX as i64) as u16,
    }
}

/// Whole-cell span for a resize drag of `px` from the tile's origin.
pub fn quantize_span(px: f32, cell_px: f32, gutter: f32, max: u8) -> u8 {
    let pitch = (cell_px + gutter).max(1.);
    (((px + gutter) / pitch).round() as i64).clamp(1, max.max(1) as i64) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlaps(cells: &[(Cell, u8, u8)]) -> bool {
        let mut seen = std::collections::HashSet::new();
        for (c, sx, sy) in cells {
            for r in c.row..c.row + *sy as u16 {
                for k in c.col..c.col + *sx as u16 {
                    if !seen.insert((k, r)) {
                        return true;
                    }
                }
            }
        }
        false
    }

    struct Lcg(u32);
    impl Lcg {
        fn next(&mut self, n: u32) -> u32 {
            self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
            (self.0 >> 8) % n.max(1)
        }
    }

    #[test]
    fn templates_flow_without_overlap_or_loss() {
        for t in TEMPLATES {
            for count in [0, 1, 7, 48, 200] {
                let cells = apply_template(count, &t, t.columns);
                assert_eq!(cells.len(), count);
                assert!(!overlaps(&cells), "{} {count}", t.id);
                for (c, sx, _) in &cells {
                    assert!(c.col as u8 + sx <= t.columns, "{} spills", t.id);
                }
            }
        }
        // Default templates pack densely: no empty cell above the last row.
        for t in TEMPLATES {
            let cells = apply_template(24, &t, t.columns);
            let rows = row_count(&cells);
            let mut filled = std::collections::HashSet::new();
            for (c, sx, sy) in &cells {
                for dy in 0..*sy as u16 {
                    for dx in 0..*sx as u16 {
                        filled.insert((c.col + dx, c.row + dy));
                    }
                }
            }
            for row in 0..rows.saturating_sub(2) {
                for col in 0..t.columns as u16 {
                    assert!(filled.contains(&(col, row)), "{} hole at {col},{row}", t.id);
                }
            }
        }
        // Re-applying is idempotent.
        let t = template("mixed");
        assert_eq!(apply_template(30, t, 3), apply_template(30, t, 3));
        // Hero leads with a full-width block.
        let hero = apply_template(4, template("hero"), 3);
        assert_eq!(hero[0], (Cell { col: 0, row: 0 }, 3, 2));
        assert_eq!(hero[1].0, Cell { col: 0, row: 2 });
    }

    #[test]
    fn random_moves_and_resizes_never_overlap_or_lose_photos() {
        let mut rng = Lcg(7);
        for columns in [1u8, 2, 3, 4, 6] {
            let t = template("mixed");
            let mut slots: Vec<Slot> = apply_template(40, t, columns)
                .into_iter()
                .map(|(c, sx, sy)| Slot {
                    span_x: sx,
                    span_y: sy,
                    cell: Some(c),
                })
                .collect();
            for step in 0..300 {
                let target = rng.next(slots.len() as u32) as usize;
                let mut next = slots.clone();
                let cells = if step % 2 == 0 {
                    let cell = Cell {
                        col: rng.next(columns as u32) as u16,
                        row: rng.next(20) as u16,
                    };
                    place_at(&slots, target, cell, columns)
                } else {
                    let (sx, sy) = (1 + rng.next(columns as u32) as u8, 1 + rng.next(3) as u8);
                    next[target].span_x = sx.clamp(1, columns);
                    next[target].span_y = sy;
                    set_span(&slots, target, sx, sy, columns)
                };
                for (i, c) in cells.iter().enumerate() {
                    next[i].cell = *c;
                }
                slots = next;
                let placed: Vec<(Cell, u8, u8)> = slots
                    .iter()
                    .map(|s| (s.cell.expect("never unplaced"), s.span_x.min(columns), s.span_y))
                    .collect();
                assert_eq!(placed.len(), 40);
                assert!(!overlaps(&placed), "step {step} columns {columns}");
            }
        }
    }

    #[test]
    fn resize_moves_neighbours_predictably() {
        // [A B C] [D E F] — grow A to 2×2: B and D move out of the way.
        let slots: Vec<Slot> = (0..6)
            .map(|i| Slot {
                span_x: 1,
                span_y: 1,
                cell: Some(Cell {
                    col: (i % 3) as u16,
                    row: (i / 3) as u16,
                }),
            })
            .collect();
        let cells = set_span(&slots, 0, 2, 2, 3);
        assert_eq!(cells[0], Some(Cell { col: 0, row: 0 }));
        let placed: Vec<(Cell, u8, u8)> = cells
            .iter()
            .enumerate()
            .map(|(i, c)| (c.unwrap(), if i == 0 { 2 } else { 1 }, if i == 0 { 2 } else { 1 }))
            .collect();
        assert!(!overlaps(&placed));
        // Siblings keep their reading order: B, C, D, E, F wrap around A.
        assert_eq!(cells[1], Some(Cell { col: 2, row: 0 }));
        assert_eq!(cells[2], Some(Cell { col: 2, row: 1 }));
        assert_eq!(cells[3], Some(Cell { col: 0, row: 2 }));
        assert_eq!(cells[4], Some(Cell { col: 1, row: 2 }));
        assert_eq!(cells[5], Some(Cell { col: 2, row: 2 }));
    }

    #[test]
    fn drop_places_in_reading_order_and_breakpoints_dont_mutate() {
        // Twenty photos dropped one by one at the first free cell fill
        // left-to-right, top-to-bottom.
        let mut slots: Vec<Slot> = Vec::new();
        for i in 0..20 {
            slots.push(Slot {
                span_x: 1,
                span_y: 1,
                cell: None,
            });
            let cells = flow(&slots, 3);
            slots[i].cell = Some(cells[i]);
            assert_eq!(cells[i], Cell { col: (i % 3) as u16, row: (i / 3) as u16 });
        }
        let before = slots.clone();
        let phone = at_breakpoint(&slots, 3, Breakpoint::Phone);
        assert_eq!(slots, before);
        assert_eq!(phone.len(), 20);
        assert!(phone.iter().enumerate().all(|(k, (i, c, sx, _))| *i == k && c.row as usize == k && *sx == 1));
        let tablet = at_breakpoint(&slots, 3, Breakpoint::Tablet);
        assert!(tablet.iter().all(|(_, c, sx, _)| c.col as u8 + sx <= 2));
    }

    #[test]
    fn geometry_helpers() {
        let (cw, rh) = metrics(676., 3, 12., 1.5);
        assert!((cw - 217.333).abs() < 0.01 && (rh - cw / 1.5).abs() < 0.01);
        let (x, y, w, h) = tile_rect(Cell { col: 1, row: 1 }, 2, 1, cw, rh, 12.);
        assert!((x - (cw + 12.)).abs() < 0.01 && (y - (rh + 12.)).abs() < 0.01);
        assert!((w - (2. * cw + 12.)).abs() < 0.01 && (h - rh).abs() < 0.01);
        assert_eq!(cell_at(x + 5., y + 5., 3, cw, rh, 12.), Cell { col: 1, row: 1 });
        assert_eq!(cell_at(9999., -5., 3, cw, rh, 12.), Cell { col: 2, row: 0 });
        assert_eq!(quantize_span(w, cw, 12., 3), 2);
        assert_eq!(quantize_span(10., cw, 12., 3), 1);
        assert_eq!(quantize_span(5000., cw, 12., 3), 3);
    }

    #[test]
    fn five_hundred_photo_flow_is_fast() {
        let t = template("mixed");
        let start = std::time::Instant::now();
        let cells = apply_template(500, t, 3);
        assert_eq!(cells.len(), 500);
        assert!(start.elapsed().as_millis() < 50, "{:?}", start.elapsed());
    }
}
