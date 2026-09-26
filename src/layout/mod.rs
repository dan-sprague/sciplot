//! Grid layout: places blocks in a figure so that axes line up by their spines.
//!
//! Interface used by the scene builder: each block reports a [`LayoutItem`] (its grid span, the
//! decoration room it needs outside its main area (protrusions), and any fixed/auto size); [`solve`]
//! returns each block's main area (for an Axis: the rectangle inside the spines).
//!
//! (M1 placeholder: equal columns/rows with per-column/row maximum protrusions. M4 replaces this
//! with a port of GridLayoutBase's solver.)

use crate::figure::{GridSize, GridSpec, Side};
use crate::scene::drawlist::Rect;

/// Decoration room outside a block's main area, in units: left, right, bottom, top.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Protrusion {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
}

/// One block's layout request.
#[derive(Clone, Debug)]
pub(crate) struct LayoutItem {
    /// 1-based inclusive row span.
    pub rows: (i32, i32),
    /// 1-based inclusive column span.
    pub cols: (i32, i32),
    pub side: Side,
    pub protrusion: Protrusion,
    /// Fixed main-area width/height (e.g. a Colorbar's 12-unit bar, Axis `width`).
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// Whether a fixed width/height should determine the column/row size (Makie `tellwidth`).
    pub tellwidth: bool,
    pub tellheight: bool,
}

/// The solved main area for each item, in figure units (y down), in input order.
pub(crate) fn solve(
    items: &[LayoutItem],
    spec: &GridSpec,
    size: [f64; 2],
    padding: [f64; 4],
    colgap: f64,
    rowgap: f64,
) -> Vec<Rect> {
    let nrows = items.iter().map(|i| i.rows.1).max().unwrap_or(1).max(1) as usize;
    let ncols = items.iter().map(|i| i.cols.1).max().unwrap_or(1).max(1) as usize;
    let colgap = spec.colgap.unwrap_or(colgap);
    let rowgap = spec.rowgap.unwrap_or(rowgap);
    let [pl, pr, pb, pt] = padding;

    // Max protrusions at each column's left/right edge and each row's top/bottom edge.
    let mut left = vec![0.0f64; ncols];
    let mut right = vec![0.0f64; ncols];
    let mut top = vec![0.0f64; nrows];
    let mut bottom = vec![0.0f64; nrows];
    for it in items.iter().filter(|i| i.side == Side::Inner) {
        let (c0, c1) = ((it.cols.0 - 1) as usize, (it.cols.1 - 1) as usize);
        let (r0, r1) = ((it.rows.0 - 1) as usize, (it.rows.1 - 1) as usize);
        left[c0] = left[c0].max(it.protrusion.left);
        right[c1] = right[c1].max(it.protrusion.right);
        top[r0] = top[r0].max(it.protrusion.top);
        bottom[r1] = bottom[r1].max(it.protrusion.bottom);
    }

    // Fixed column widths / row heights from sizes and told fixed widths.
    let mut colw: Vec<Option<f64>> = vec![None; ncols];
    let mut rowh: Vec<Option<f64>> = vec![None; nrows];
    for it in items.iter().filter(|i| i.side == Side::Inner) {
        if it.tellwidth && it.cols.0 == it.cols.1 {
            if let Some(w) = it.width {
                let c = (it.cols.0 - 1) as usize;
                colw[c] = Some(colw[c].unwrap_or(0.0).max(w));
            }
        }
        if it.tellheight && it.rows.0 == it.rows.1 {
            if let Some(h) = it.height {
                let r = (it.rows.0 - 1) as usize;
                rowh[r] = Some(rowh[r].unwrap_or(0.0).max(h));
            }
        }
    }
    let avail_w = size[0]
        - pl
        - pr
        - colgap * (ncols as f64 - 1.0)
        - left.iter().sum::<f64>()
        - right.iter().sum::<f64>();
    let avail_h = size[1]
        - pt
        - pb
        - rowgap * (nrows as f64 - 1.0)
        - top.iter().sum::<f64>()
        - bottom.iter().sum::<f64>();
    let widths = distribute(&colw, &spec.colsizes, avail_w);
    let heights = distribute(&rowh, &spec.rowsizes, avail_h);

    // Cell main-area origins.
    let mut xs = vec![0.0; ncols];
    let mut x = pl;
    for c in 0..ncols {
        x += left[c];
        xs[c] = x;
        x += widths[c] + right[c] + colgap;
    }
    let mut ys = vec![0.0; nrows];
    let mut y = pt;
    for r in 0..nrows {
        y += top[r];
        ys[r] = y;
        y += heights[r] + bottom[r] + rowgap;
    }

    items
        .iter()
        .map(|it| {
            let (c0, c1) = ((it.cols.0 - 1) as usize, (it.cols.1 - 1) as usize);
            let (r0, r1) = ((it.rows.0 - 1) as usize, (it.rows.1 - 1) as usize);
            let x0 = xs[c0];
            let x1 = xs[c1] + widths[c1];
            let y0 = ys[r0];
            let y1 = ys[r1] + heights[r1];
            let mut r = Rect::new(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0));
            if let Some(w) = it.width {
                r.x += 0.5 * (r.w - w);
                r.w = w;
            }
            if let Some(h) = it.height {
                r.y += 0.5 * (r.h - h);
                r.h = h;
            }
            r.x = r.x.round();
            r.y = r.y.round();
            r.w = r.w.round().max(1.0);
            r.h = r.h.round().max(1.0);
            r
        })
        .collect()
}

fn distribute(fixed: &[Option<f64>], sizes: &[(i32, GridSize)], avail: f64) -> Vec<f64> {
    let n = fixed.len();
    let mut out: Vec<Option<f64>> = fixed.to_vec();
    for &(i, s) in sizes {
        let i = (i - 1) as usize;
        if i < n {
            match s {
                GridSize::Fixed(v) => out[i] = Some(v),
                GridSize::Relative(f) => out[i] = Some(f * avail),
                GridSize::Auto | GridSize::Aspect(..) => out[i] = None,
            }
        }
    }
    let used: f64 = out.iter().flatten().sum();
    let free = out.iter().filter(|o| o.is_none()).count().max(1) as f64;
    let each = ((avail - used) / free).max(1.0);
    out.into_iter().map(|o| o.unwrap_or(each)).collect()
}
