//! Heatmap orientation: `z[i, j]` is the cell at `(x_i, y_j)` for every 2D input form, so a field
//! that is nonzero only at `(nx - 1, 0)` lights the bottom-right cell.

use sciplot::prelude::*;

const NX: usize = 4;
const NY: usize = 3;

/// Bounding box `[x0, y0, x1, y1]` (inclusive px) of the pixels matching `pred`.
fn bbox(img: &sciplot::RgbaImage, pred: impl Fn(&[u8]) -> bool) -> Option<[u32; 4]> {
    let mut b: Option<[u32; 4]> = None;
    for (k, px) in img.data.as_chunks::<4>().0.iter().enumerate() {
        if pred(px) {
            let (x, y) = (k as u32 % img.width, k as u32 / img.width);
            let e = b.get_or_insert([x, y, x, y]);
            *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
        }
    }
    b
}

/// Renders `fig` and asserts the only bright (viridis-yellow) cell is the bottom-right one.
fn assert_bottom_right(what: &str, hm: Heatmap) {
    let img = match hm.figure().render_rgba(&Save::new().px_per_unit(1)) {
        Ok(img) => img,
        Err(sciplot::Error::NoGpuAdapter(_)) => return,
        Err(e) => panic!("{e}"),
    };
    // Viridis ends: dark purple (68, 1, 84) for 0, yellow (253, 231, 37) for 1.
    let purple = bbox(&img, |p| p[0] < 90 && p[1] < 20 && p[2] > 60 && p[2] < 110).expect("no low cells");
    let yellow = bbox(&img, |p| p[0] > 230 && p[1] > 210 && p[2] < 60).expect("no high cell");
    let field =
        [purple[0].min(yellow[0]), purple[1].min(yellow[1]), purple[2].max(yellow[2]), purple[3].max(yellow[3])];
    let (cw, ch) = ((field[2] - field[0] + 1) as f64 / NX as f64, (field[3] - field[1] + 1) as f64 / NY as f64);
    let near = |a: u32, b: f64| (a as f64 - b).abs() <= 2.0;
    assert!(
        near(yellow[2], field[2] as f64) && near(yellow[3], field[3] as f64),
        "{what}: high cell {yellow:?} is not at the bottom-right of {field:?}"
    );
    assert!(
        near(yellow[0], field[2] as f64 + 1.0 - cw) && near(yellow[1], field[3] as f64 + 1.0 - ch),
        "{what}: high cell {yellow:?} is not one cell of {field:?}"
    );
}

#[test]
fn every_input_form_lights_the_bottom_right_cell() {
    let mut flat = vec![0.0f64; NX * NY];
    flat[NX - 1] = 1.0; // x fastest: index j * nx + i with (i, j) = (nx - 1, 0)
    assert_bottom_right("Field::new", heatmap(Field::new(&flat, NX, NY)));
    assert_bottom_right("tuple", heatmap((&flat, NX, NY)));
    assert_bottom_right("tuple slice", heatmap((&flat[..], NX, NY)));

    let mut yf = vec![0.0f32; NX * NY];
    yf[(NX - 1) * NY] = 1.0; // y fastest: index i * ny + j
    assert_bottom_right("Field::y_fastest", heatmap(Field::y_fastest(&yf, NX, NY)));

    let mut cols = vec![vec![0i32; NY]; NX];
    cols[NX - 1][0] = 1; // v[ix][iy]
    assert_bottom_right("Vec<Vec>", heatmap(&cols));

    // Same with explicit coordinates, including edges and an interval.
    assert_bottom_right("heatmap_xy", heatmap_xy(Edges(0.0, 1.0), 10.0..=12.0, Field::new(&flat, NX, NY)));
}

#[cfg(feature = "ndarray")]
#[test]
fn ndarray_c_and_f_order() {
    use ndarray::{Array2, ShapeBuilder};
    let mut c = Array2::<f64>::zeros((NX, NY));
    c[[NX - 1, 0]] = 1.0;
    assert_bottom_right("ndarray C order", heatmap(&c));
    let mut f = Array2::<f64>::zeros((NX, NY).f());
    f[[NX - 1, 0]] = 1.0;
    assert_bottom_right("ndarray F order", heatmap(&f));
    assert_bottom_right("ndarray view", heatmap(&c.view()));
}

#[test]
fn value_colored_scatter_renders() {
    let v = [0.0, 1.0, f64::NAN];
    let sc = scatter([1.0, 2.0, 3.0], [1.0, 2.0, 3.0]).color(&v[..]).markersize(40).colormap("magma");
    let img = match sc.figure().render_rgba(&Save::new().px_per_unit(1)) {
        Ok(img) => img,
        Err(sciplot::Error::NoGpuAdapter(_)) => return,
        Err(e) => panic!("{e}"),
    };
    // magma's last color (252, 253, 191) is drawn; the NaN point is transparent (nan_color).
    assert!(bbox(&img, |p| p[0] > 245 && p[1] > 245 && (180..200).contains(&p[2])).is_some());
}
