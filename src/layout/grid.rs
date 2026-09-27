//! The solver, ported from GridLayoutBase 0.11 (`GLB/gridlayout.jl`, `GLB/layoutobservables.jl`)
//! in its own y-up coordinates. Function names in comments refer to the Julia originals.
//!
//! Deviations, all in cases where GridLayoutBase throws: an Aspect size whose reference cannot be
//! determined shares the leftover space like an Auto size; relative gaps make a grid's size
//! undeterminable; `Mixed` align modes are supported by `determinedirsize` and `tight_bbox`.
//!
//! Provenance: ported from GridLayoutBase 0.11.3 `src/gridlayout.jl` (`compute_rowcols`,
//! `_compute_maxgrid`, `compute_col_row_sizes`, `determinedirsize`, `dirgaps`, `align_to_bbox!`,
//! `tight_bbox`) and `src/layoutobservables.jl` (`computed_size`, `update_computedbbox!`,
//! `effective_protrusion`). The 18-unit default gaps follow Makie 0.24.14 `src/theming.jl`. MIT
//! licensed; see THIRD_PARTY_NOTICES.md.

use super::{AlignMode, BBox, BlockSize, Gap, LayoutItem, MixedSide, Protrusion};
use crate::figure::{GridSize, Side};

/// A grid layout: the figure's root layout or a nested one.
#[derive(Clone, Debug)]
pub struct Grid {
    /// Placement in the parent grid (1-based inclusive rows); unused for the root.
    pub rows: (i32, i32),
    pub cols: (i32, i32),
    pub side: Side,
    pub nrows: usize,
    pub ncols: usize,
    /// One per row/column; missing entries are `Auto`.
    pub rowsizes: Vec<GridSize>,
    pub colsizes: Vec<GridSize>,
    /// Gaps added between rows/columns (`nrows - 1` / `ncols - 1`); missing entries are 0.
    pub rowgaps: Vec<Gap>,
    pub colgaps: Vec<Gap>,
    pub alignmode: AlignMode,
    /// Makie `equalprotrusiongaps` as (rows, cols): make every protrusion gap as big as the largest.
    pub equalprotrusiongaps: [bool; 2],
    pub width: BlockSize,
    pub height: BlockSize,
    pub tellwidth: bool,
    pub tellheight: bool,
    /// Where the grid sits in a larger bbox: 0 = left/bottom, 1 = right/top.
    pub halign: f64,
    pub valign: f64,
    pub content: Vec<Content>,
}

/// Something placed in a grid cell.
#[derive(Clone, Debug)]
pub enum Content {
    Block(LayoutItem),
    Grid(Grid),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Edge {
    Left,
    Right,
    Bottom,
    Top,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Col,
    Row,
}

impl Dir {
    /// Index into `[width, height]` pairs.
    fn idx(self) -> usize {
        match self {
            Dir::Col => 0,
            Dir::Row => 1,
        }
    }
    /// The edge at a row/column's start (GLB `startside`): rows are counted from the top.
    fn start(self) -> Edge {
        match self {
            Dir::Col => Edge::Left,
            Dir::Row => Edge::Top,
        }
    }
    fn stop(self) -> Edge {
        match self {
            Dir::Col => Edge::Right,
            Dir::Row => Edge::Bottom,
        }
    }
}

impl Edge {
    /// The direction measured across this edge's protrusion strip.
    fn dir(self) -> Dir {
        match self {
            Edge::Left | Edge::Right => Dir::Col,
            Edge::Bottom | Edge::Top => Dir::Row,
        }
    }
}

impl Protrusion {
    fn get(&self, e: Edge) -> f64 {
        match e {
            Edge::Left => self.left,
            Edge::Right => self.right,
            Edge::Bottom => self.bottom,
            Edge::Top => self.top,
        }
    }
    fn from_fn(f: impl Fn(Edge) -> f64) -> Protrusion {
        Protrusion { left: f(Edge::Left), right: f(Edge::Right), bottom: f(Edge::Bottom), top: f(Edge::Top) }
    }
}

impl AlignMode {
    /// Every align mode seen per side (`Outside(pad)` is `Mixed` with `Pad` everywhere).
    fn side(&self, e: Edge) -> MixedSide {
        match *self {
            AlignMode::Inside => MixedSide::Inside,
            AlignMode::Outside(p) => MixedSide::Pad(p.get(e)),
            AlignMode::Mixed { left, right, bottom, top } => match e {
                Edge::Left => left,
                Edge::Right => right,
                Edge::Bottom => bottom,
                Edge::Top => top,
            },
        }
    }
    /// Outside padding on this side (`_compute_content_bbox`).
    fn pad(&self, e: Edge) -> f64 {
        match self.side(e) {
            MixedSide::Pad(p) => p,
            _ => 0.0,
        }
    }
    /// The protrusion a parent sees (`effective_protrusion`).
    fn effective(&self, e: Edge, prot: f64) -> f64 {
        match self.side(e) {
            MixedSide::Inside => prot,
            MixedSide::Pad(_) => 0.0,
            MixedSide::Protrusion(p) => p,
        }
    }
    /// Room a grid keeps for its outer protrusion inside its padded bbox (`compute_rowcols`).
    fn prot_room(&self, e: Edge, prot: f64) -> f64 {
        match self.side(e) {
            MixedSide::Inside => 0.0,
            MixedSide::Pad(_) => prot,
            MixedSide::Protrusion(p) => p,
        }
    }
    /// What a block adds to its reported size / removes from its bbox on this side.
    fn block_extra(&self, e: Edge, prot: f64) -> f64 {
        match self.side(e) {
            MixedSide::Pad(p) => prot + p,
            _ => 0.0,
        }
    }
}

/// Whether content placed at `side` sits in the protrusion strip beyond edge `e` of its cell.
fn side_touches(side: Side, e: Edge) -> bool {
    match e {
        Edge::Left => matches!(side, Side::Left | Side::TopLeft | Side::BottomLeft),
        Edge::Right => matches!(side, Side::Right | Side::TopRight | Side::BottomRight),
        Edge::Bottom => matches!(side, Side::Bottom | Side::BottomLeft | Side::BottomRight),
        Edge::Top => matches!(side, Side::Top | Side::TopLeft | Side::TopRight),
    }
}

/// What a content element reports to its grid (`Dimensions` plus the autosize).
#[derive(Clone, Copy, Debug)]
struct Dims {
    /// Width/height that may determine a column/row size.
    inner: [Option<f64>; 2],
    /// Effective protrusions.
    outer: Protrusion,
    autosize: [Option<f64>; 2],
}

/// `computed_size`: the size a block tells its grid.
fn computed_size(attr: BlockSize, auto: Option<f64>, tell: bool) -> Option<f64> {
    match attr {
        _ if !tell => None,
        BlockSize::Fixed(v) => Some(v),
        BlockSize::Auto => auto,
        BlockSize::Fill | BlockSize::Relative(_) => None,
    }
}

/// `update_computedbbox!`: a block's (or grid's) own bbox inside the suggested one.
fn computed_bbox(sugg: BBox, d: &Dims, size: [BlockSize; 2], align: [f64; 2], am: AlignMode, prot: Protrusion) -> BBox {
    let (bw, bh) = (sugg.width(), sugg.height());
    let target = |i: usize, avail: f64| {
        d.inner[i].unwrap_or(match size[i] {
            BlockSize::Relative(x) => x * avail,
            BlockSize::Fill => avail,
            BlockSize::Auto => d.autosize[i].unwrap_or(avail),
            BlockSize::Fixed(x) => x,
        })
    };
    let (wt, ht) = (target(0, bw), target(1, bh));
    let extra = |e: Edge| am.block_extra(e, prot.get(e));
    let l = sugg.l + align[0] * (bw - wt) + extra(Edge::Left);
    let b = sugg.b + align[1] * (bh - ht) + extra(Edge::Bottom);
    BBox {
        l,
        r: l + wt - extra(Edge::Left) - extra(Edge::Right),
        b,
        t: b + ht - extra(Edge::Bottom) - extra(Edge::Top),
    }
}

impl Content {
    fn span(&self) -> ((i32, i32), (i32, i32)) {
        match self {
            Content::Block(it) => (it.rows, it.cols),
            Content::Grid(g) => (g.rows, g.cols),
        }
    }

    fn side(&self) -> Side {
        match self {
            Content::Block(it) => it.side,
            Content::Grid(g) => g.side,
        }
    }

    /// `reporteddimensions` (a grid's own layout observables always use `Inside`).
    fn reported(&self) -> Dims {
        match self {
            Content::Block(it) => {
                let (am, p) = (it.alignmode, it.protrusion);
                let w = computed_size(it.width, it.autosize[0], it.tellwidth);
                let h = computed_size(it.height, it.autosize[1], it.tellheight);
                let x = |e: Edge| am.block_extra(e, p.get(e));
                Dims {
                    inner: [
                        w.map(|w| w + x(Edge::Left) + x(Edge::Right)),
                        h.map(|h| h + x(Edge::Bottom) + x(Edge::Top)),
                    ],
                    outer: Protrusion::from_fn(|e| am.effective(e, p.get(e))),
                    autosize: it.autosize,
                }
            }
            Content::Grid(g) => g.reported(),
        }
    }

    /// `protrusion(content, side)`: the protrusion before a block's align mode is applied.
    fn raw_protrusion(&self, e: Edge) -> f64 {
        match self {
            Content::Block(it) => it.protrusion.get(e),
            Content::Grid(g) => g.raw_protrusion(e),
        }
    }

    /// `effective_protrusion(gc, side, gc.side)`: how far this content reaches beyond its cell at
    /// edge `e`. Side-placed content contributes its determined width/height.
    fn effective_protrusion(&self, e: Edge, d: &Dims) -> f64 {
        match self.side() {
            Side::Inner => d.outer.get(e),
            s if side_touches(s, e) => d.inner[e.dir().idx()].unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// `protrusion(gc, side)`, used when a grid determines its own size (`dirgaps`).
    fn gc_protrusion(&self, e: Edge, d: &Dims) -> f64 {
        match self.side() {
            Side::Inner => self.raw_protrusion(e),
            s if side_touches(s, e) => d.inner[e.dir().idx()].unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// Places this content in its suggested bbox and appends the leaves' bboxes to `out`.
    fn place(&self, sugg: BBox, d: &Dims, out: &mut Vec<BBox>) {
        match self {
            Content::Block(it) => out.push(computed_bbox(
                sugg,
                d,
                [it.width, it.height],
                [it.halign, it.valign],
                it.alignmode,
                it.protrusion,
            )),
            Content::Grid(g) => g.place(sugg, d, out),
        }
    }
}

/// Per-column left/right and per-row top/bottom values (GLB `RowCols`).
struct RowCols {
    lefts: Vec<f64>,
    rights: Vec<f64>,
    tops: Vec<f64>,
    bottoms: Vec<f64>,
}

fn cumsum0(v: &[f64]) -> Vec<f64> {
    let mut acc = 0.0;
    let mut out = Vec::with_capacity(v.len() + 1);
    out.push(0.0);
    for x in v {
        acc += x;
        out.push(acc);
    }
    out
}

fn auto_ratio(s: GridSize) -> Option<f64> {
    match s {
        GridSize::Auto => Some(1.0),
        GridSize::AutoWith { ratio, .. } => Some(ratio),
        _ => None,
    }
}

impl Grid {
    /// An empty `nrows × ncols` grid with Makie's `GridLayout()` defaults (Auto sizes, 18-unit
    /// gaps, `Inside`, Auto width/height, tell both, centered).
    pub fn new(nrows: usize, ncols: usize) -> Grid {
        let (nrows, ncols) = (nrows.max(1), ncols.max(1));
        Grid {
            rows: (1, 1),
            cols: (1, 1),
            side: Side::Inner,
            nrows,
            ncols,
            rowsizes: vec![GridSize::Auto; nrows],
            colsizes: vec![GridSize::Auto; ncols],
            rowgaps: vec![Gap::Fixed(18.0); nrows - 1],
            colgaps: vec![Gap::Fixed(18.0); ncols - 1],
            alignmode: AlignMode::Inside,
            equalprotrusiongaps: [false, false],
            width: BlockSize::Auto,
            height: BlockSize::Auto,
            tellwidth: true,
            tellheight: true,
            halign: 0.5,
            valign: 0.5,
            content: Vec::new(),
        }
    }

    /// Solves this grid as a figure's root layout (suggested bbox = the whole figure) and returns
    /// every block's bbox, depth first in content order. Axis viewports are `BBox::round` of these.
    pub fn solve_root(&self, size: [f64; 2]) -> Vec<BBox> {
        let mut out = Vec::new();
        self.place(BBox { l: 0.0, r: size[0], b: 0.0, t: size[1] }, &self.reported(), &mut out);
        out
    }

    /// `tight_bbox`: the suggested bbox this grid would fit exactly (for `resize_to_layout`).
    pub fn tight_bbox(&self, size: [f64; 2]) -> BBox {
        let dims = self.dims();
        let (maxgrid, boxes) = self.compute_rowcols(BBox { l: 0.0, r: size[0], b: 0.0, t: size[1] }, &dims);
        let al = self.alignmode;
        let room = |e: Edge, prot: f64| al.prot_room(e, prot) + al.pad(e);
        BBox {
            l: boxes.lefts[0] - room(Edge::Left, maxgrid.lefts[0]),
            r: boxes.rights[self.ncols - 1] + room(Edge::Right, maxgrid.rights[self.ncols - 1]),
            b: boxes.bottoms[self.nrows - 1] - room(Edge::Bottom, maxgrid.bottoms[self.nrows - 1]),
            t: boxes.tops[0] + room(Edge::Top, maxgrid.tops[0]),
        }
    }

    fn n(&self, d: Dir) -> usize {
        match d {
            Dir::Col => self.ncols.max(1),
            Dir::Row => self.nrows.max(1),
        }
    }

    fn size_spec(&self, d: Dir, i: usize) -> GridSize {
        let v = match d {
            Dir::Col => &self.colsizes,
            Dir::Row => &self.rowsizes,
        };
        v.get(i).copied().unwrap_or(GridSize::Auto)
    }

    fn gap_spec(&self, d: Dir, i: usize) -> Gap {
        let v = match d {
            Dir::Col => &self.colgaps,
            Dir::Row => &self.rowgaps,
        };
        v.get(i).copied().unwrap_or(Gap::Fixed(0.0))
    }

    /// A content's 0-based (start, stop) indices along `d`, clamped into the grid.
    fn span_idx(&self, c: &Content, d: Dir) -> (usize, usize) {
        let (rows, cols) = c.span();
        let s = if d == Dir::Col { cols } else { rows };
        let n = self.n(d) as i32;
        let f = |i: i32| (i.clamp(1, n) - 1) as usize;
        (f(s.0), f(s.1).max(f(s.0)))
    }

    /// `ismostin`: whether content touches this grid's edge `e`.
    fn touches_edge(&self, c: &Content, e: Edge) -> bool {
        let d = e.dir();
        let (s0, s1) = self.span_idx(c, d);
        if e == d.start() { s0 == 0 } else { s1 == self.n(d) - 1 }
    }

    fn dims(&self) -> Vec<Dims> {
        self.content.iter().map(Content::reported).collect()
    }

    /// The grid's own `reporteddimensions` (its layout observables are `Inside`; autosize is the
    /// determined size and protrusions are the effective ones of its edge content).
    fn reported(&self) -> Dims {
        let dims = self.dims();
        let autosize = [self.determined(Dir::Col, &dims), self.determined(Dir::Row, &dims)];
        let outer = Protrusion::from_fn(|e| match self.alignmode.side(e) {
            MixedSide::Pad(_) => 0.0,
            MixedSide::Protrusion(p) => p,
            MixedSide::Inside => self.max_edge(e, |c, i| c.effective_protrusion(e, &dims[i])),
        });
        Dims {
            inner: [
                computed_size(self.width, autosize[0], self.tellwidth),
                computed_size(self.height, autosize[1], self.tellheight),
            ],
            outer,
            autosize,
        }
    }

    /// `protrusion(gl, side)`.
    fn raw_protrusion(&self, e: Edge) -> f64 {
        match self.alignmode.side(e) {
            MixedSide::Pad(_) => 0.0,
            MixedSide::Protrusion(p) => p,
            MixedSide::Inside => self.max_edge(e, |c, _| c.gc_protrusion(e, &c.reported())),
        }
    }

    /// Maximum of `f` over the content touching edge `e` (0 if none).
    fn max_edge(&self, e: Edge, f: impl Fn(&Content, usize) -> f64) -> f64 {
        self.content.iter().enumerate().filter(|(_, c)| self.touches_edge(c, e)).fold(0.0, |m, (i, c)| m.max(f(c, i)))
    }

    /// `determinedirsize(idir, gl, dir)`: one column's width / row's height if content fixes it.
    fn determine_one(&self, d: Dir, i: usize, dims: &[Dims]) -> Option<f64> {
        match self.size_spec(d, i) {
            GridSize::Fixed(x) => Some(x),
            GridSize::Relative(_) | GridSize::Aspect(..) => None,
            GridSize::AutoWith { trydetermine: false, .. } => None,
            GridSize::Auto | GridSize::AutoWith { .. } => self
                .content
                .iter()
                .zip(dims)
                .filter(|(c, _)| c.side() == Side::Inner && self.span_idx(c, d) == (i, i))
                .filter_map(|(_, dm)| dm.inner[d.idx()])
                .reduce(f64::max),
        }
    }

    /// `determinedirsize(gl, dir)`: the grid's whole width/height if every column/row is
    /// determined (including the protrusion gaps, fixed gaps and, when aligned outside, the outer
    /// protrusions and paddings).
    fn determined(&self, d: Dir, dims: &[Dims]) -> Option<f64> {
        let n = self.n(d);
        let mut total = 0.0;
        for i in 0..n {
            total += self.determine_one(d, i, dims)?;
        }
        // `dirgaps`
        let (mut starts, mut stops) = (vec![0.0f64; n], vec![0.0f64; n]);
        for (c, dm) in self.content.iter().zip(dims) {
            let (s0, s1) = self.span_idx(c, d);
            starts[s0] = starts[s0].max(c.gc_protrusion(d.start(), dm));
            stops[s1] = stops[s1].max(c.gc_protrusion(d.stop(), dm));
        }
        let inner: Vec<f64> = (1..n).map(|i| starts[i] + stops[i - 1]).collect();
        total += if self.equalprotrusiongaps[1 - d.idx()] {
            inner.iter().copied().fold(0.0, f64::max) * inner.len() as f64
        } else {
            inner.iter().sum()
        };
        for i in 0..n - 1 {
            match self.gap_spec(d, i) {
                Gap::Fixed(x) => total += x,
                Gap::Relative(_) => return None,
            }
        }
        let al = self.alignmode;
        let room = |e: Edge, prot: f64| al.prot_room(e, prot) + al.pad(e);
        Some(total + room(d.start(), starts[0]) + room(d.stop(), stops[n - 1]))
    }

    /// `compute_col_row_sizes`: Fixed, Relative, determinable Auto, Aspect referring to a
    /// determined size; then the remaining Autos share the leftover space by ratio on a side
    /// without pending Aspects, the remaining Aspects resolve, and the last Autos share.
    fn col_row_sizes(&self, space: [f64; 2], dims: &[Dims]) -> [Vec<f64>; 2] {
        let dirs = [Dir::Col, Dir::Row];
        let specs: [Vec<GridSize>; 2] = dirs.map(|d| (0..self.n(d)).map(|i| self.size_spec(d, i)).collect());
        let mut size: [Vec<f64>; 2] = dirs.map(|d| vec![0.0; self.n(d)]);
        let mut done: [Vec<bool>; 2] = dirs.map(|d| vec![false; self.n(d)]);

        for d in dirs {
            let k = d.idx();
            for (i, s) in specs[k].iter().enumerate() {
                let v = match *s {
                    GridSize::Fixed(x) => Some(x),
                    GridSize::Relative(x) => Some(x * space[k]),
                    GridSize::Auto | GridSize::AutoWith { .. } => self.determine_one(d, i, dims),
                    GridSize::Aspect(..) => None,
                };
                if let Some(v) = v {
                    size[k][i] = v;
                    done[k][i] = true;
                }
            }
        }

        // Aspect(j, ratio) in a column refers to row j's height, and vice versa.
        let resolve_aspects = |size: &mut [Vec<f64>; 2], done: &mut [Vec<bool>; 2]| {
            for k in 0..2 {
                let o = 1 - k;
                for (i, s) in specs[k].iter().enumerate() {
                    if let GridSize::Aspect(j, ratio) = *s {
                        let j = usize::try_from(j - 1).ok().filter(|&j| j < done[o].len());
                        if let Some(j) = j.filter(|&j| done[o][j]) {
                            size[k][i] = ratio * size[o][j];
                            done[k][i] = true;
                        }
                    }
                }
            }
        };
        let share_autos = |k: usize, size: &mut [Vec<f64>; 2], done: &mut [Vec<bool>; 2], aspects_too: bool| {
            let remaining = space[k] - size[k].iter().sum::<f64>();
            let open: Vec<(usize, f64)> = specs[k]
                .iter()
                .enumerate()
                .filter(|&(i, _)| !done[k][i])
                .filter_map(|(i, s)| auto_ratio(*s).or(aspects_too.then_some(1.0)).map(|r| (i, r)))
                .collect();
            let sum: f64 = open.iter().map(|(_, r)| r).sum();
            for &(i, r) in &open {
                size[k][i] = if sum > 0.0 { r / sum } else { 1.0 / open.len() as f64 } * remaining;
                done[k][i] = true;
            }
        };

        resolve_aspects(&mut size, &mut done);
        for k in 0..2 {
            let aspects_left = specs[k].iter().zip(&done[k]).any(|(s, &d)| matches!(s, GridSize::Aspect(..)) && !d);
            if !aspects_left {
                share_autos(k, &mut size, &mut done, false);
            }
        }
        resolve_aspects(&mut size, &mut done);
        // Unresolvable Aspects (GridLayoutBase errors) share the leftover like Autos.
        share_autos(0, &mut size, &mut done, true);
        share_autos(1, &mut size, &mut done, true);
        size
    }

    /// `compute_rowcols`: the protrusion grid and each column's left/right and row's top/bottom.
    fn compute_rowcols(&self, bbox: BBox, dims: &[Dims]) -> (RowCols, RowCols) {
        let al = self.alignmode;
        let (nc, nr) = (self.n(Dir::Col), self.n(Dir::Row));
        let content = BBox {
            l: bbox.l + al.pad(Edge::Left),
            r: bbox.r - al.pad(Edge::Right),
            b: bbox.b + al.pad(Edge::Bottom),
            t: bbox.t - al.pad(Edge::Top),
        };

        // `_compute_maxgrid`
        let mut mg =
            RowCols { lefts: vec![0.0; nc], rights: vec![0.0; nc], tops: vec![0.0; nr], bottoms: vec![0.0; nr] };
        for (c, d) in self.content.iter().zip(dims) {
            let (c0, c1) = self.span_idx(c, Dir::Col);
            let (r0, r1) = self.span_idx(c, Dir::Row);
            mg.lefts[c0] = mg.lefts[c0].max(c.effective_protrusion(Edge::Left, d));
            mg.rights[c1] = mg.rights[c1].max(c.effective_protrusion(Edge::Right, d));
            mg.tops[r0] = mg.tops[r0].max(c.effective_protrusion(Edge::Top, d));
            mg.bottoms[r1] = mg.bottoms[r1].max(c.effective_protrusion(Edge::Bottom, d));
        }
        let room_l = al.prot_room(Edge::Left, mg.lefts[0]);
        let room_r = al.prot_room(Edge::Right, mg.rights[nc - 1]);
        let room_t = al.prot_room(Edge::Top, mg.tops[0]);
        let room_b = al.prot_room(Edge::Bottom, mg.bottoms[nr - 1]);

        let prot_gaps = |starts: &[f64], stops: &[f64], equal: bool| {
            let mut g: Vec<f64> = (1..starts.len()).map(|i| starts[i] + stops[i - 1]).collect();
            if equal {
                let m = g.iter().copied().fold(0.0, f64::max);
                g.iter_mut().for_each(|x| *x = m);
            }
            g
        };
        let colgaps = prot_gaps(&mg.lefts, &mg.rights, self.equalprotrusiongaps[1]);
        let rowgaps = prot_gaps(&mg.tops, &mg.bottoms, self.equalprotrusiongaps[0]);

        let remaining_w = content.width() - colgaps.iter().sum::<f64>() - room_l - room_r;
        let remaining_h = content.height() - rowgaps.iter().sum::<f64>() - room_b - room_t;
        let added = |d: Dir, remaining: f64| -> Vec<f64> {
            (0..self.n(d) - 1)
                .map(|i| match self.gap_spec(d, i) {
                    Gap::Fixed(x) => x,
                    Gap::Relative(x) => x * remaining,
                })
                .collect()
        };
        let added_c = added(Dir::Col, remaining_w);
        let added_r = added(Dir::Row, remaining_h);
        let space = [remaining_w - added_c.iter().sum::<f64>(), remaining_h - added_r.iter().sum::<f64>()];

        let [mut widths, mut heights] = self.col_row_sizes(space, dims);
        // No column/row below 1 unit, even if that breaks the layout.
        widths.iter_mut().for_each(|w| *w = w.max(1.0));
        heights.iter_mut().for_each(|h| *h = h.max(1.0));
        let final_c: Vec<f64> = colgaps.iter().zip(&added_c).map(|(a, b)| a + b).collect();
        let final_r: Vec<f64> = rowgaps.iter().zip(&added_r).map(|(a, b)| a + b).collect();

        let gridwidth = widths.iter().sum::<f64>() + final_c.iter().sum::<f64>() + room_l + room_r;
        let gridheight = heights.iter().sum::<f64>() + final_r.iter().sum::<f64>() + room_b + room_t;
        let xadj = self.halign * (content.width() - gridwidth);
        let yadj = (1.0 - self.valign) * (content.height() - gridheight);

        let (cw, cg) = (cumsum0(&widths), cumsum0(&final_c));
        let lefts: Vec<f64> = (0..nc).map(|c| xadj + content.l + cw[c] + cg[c] + room_l).collect();
        let rights = lefts.iter().zip(&widths).map(|(l, w)| l + w).collect();
        let (rh, rg) = (cumsum0(&heights), cumsum0(&final_r));
        let tops: Vec<f64> = (0..nr).map(|r| content.t - yadj - rh[r] - rg[r] - room_t).collect();
        let bottoms = tops.iter().zip(&heights).map(|(t, h)| t - h).collect();
        (mg, RowCols { lefts, rights, tops, bottoms })
    }

    /// Computes the grid's own bbox in `sugg` (like any layoutable), then `align_to_bbox!`: gives
    /// every content element its cell, or the protrusion strip next to it for side content.
    fn place(&self, sugg: BBox, own: &Dims, out: &mut Vec<BBox>) {
        let bbox = computed_bbox(
            sugg,
            own,
            [self.width, self.height],
            [self.halign, self.valign],
            AlignMode::Inside,
            Protrusion::default(),
        );
        let dims = self.dims();
        let (mg, boxes) = self.compute_rowcols(bbox, &dims);
        for (c, d) in self.content.iter().zip(&dims) {
            let (c0, c1) = self.span_idx(c, Dir::Col);
            let (r0, r1) = self.span_idx(c, Dir::Row);
            let (l, r, b, t) = (boxes.lefts[c0], boxes.rights[c1], boxes.bottoms[r1], boxes.tops[r0]);
            let (pl, pr, pb, pt) = (mg.lefts[c0], mg.rights[c1], mg.bottoms[r1], mg.tops[r0]);
            // `bbox_for_solving_from_side`
            let (l, r, b, t) = match c.side() {
                Side::Inner => (l, r, b, t),
                Side::Left => (l - pl, l, b, t),
                Side::Right => (r, r + pr, b, t),
                Side::Top => (l, r, t, t + pt),
                Side::Bottom => (l, r, b - pb, b),
                Side::TopLeft => (l - pl, l, t, t + pt),
                Side::TopRight => (r, r + pr, t, t + pt),
                Side::BottomLeft => (l - pl, l, b - pb, b),
                Side::BottomRight => (r, r + pr, b - pb, b),
            };
            c.place(BBox { l, r, b, t }, d, out);
        }
    }
}
