//! The layout solver against GridLayoutBase/CairoMakie reference layouts.
//!
//! `tests/fixtures/layout.json` is written by `tools/gen_layout_fixtures.jl`: for each figure the
//! whole layout tree with every block's inputs (span, side, protrusions, size attributes, autosize,
//! tell flags, alignment) and Makie's result (computed bbox, and for an Axis its scene viewport).
//! We feed exactly those inputs to the solver and require every bbox within 0.01 units, every Axis
//! viewport exactly (after rounding) and the `resize_to_layout!` size exactly.

use sciplot::testing::layout::{AlignMode, BBox, BlockSize, Content, Gap, Grid, LayoutItem, MixedSide, Protrusion};
use sciplot::{GridSize, Side};
use serde_json::Value;

const TOL: f64 = 0.01;

fn f(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

fn opt(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn sides(v: &Value) -> Protrusion {
    Protrusion { left: f(&v[0]), right: f(&v[1]), bottom: f(&v[2]), top: f(&v[3]) }
}

fn bbox(v: &Value) -> BBox {
    BBox::from_origin_size(f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3]))
}

fn span(v: &Value) -> (i32, i32) {
    (v[0].as_i64().unwrap() as i32, v[1].as_i64().unwrap() as i32)
}

fn side(v: &Value) -> Side {
    match v.as_str().unwrap() {
        "Inner" => Side::Inner,
        "Left" => Side::Left,
        "Right" => Side::Right,
        "Top" => Side::Top,
        "Bottom" => Side::Bottom,
        "TopLeft" => Side::TopLeft,
        "TopRight" => Side::TopRight,
        "BottomLeft" => Side::BottomLeft,
        "BottomRight" => Side::BottomRight,
        s => panic!("unsupported side {s}"),
    }
}

fn grid_size(v: &Value) -> GridSize {
    match v["type"].as_str().unwrap() {
        "Auto" => GridSize::AutoWith { trydetermine: v["trydetermine"].as_bool().unwrap(), ratio: f(&v["ratio"]) },
        "Fixed" => GridSize::Fixed(f(&v["x"])),
        "Relative" => GridSize::Relative(f(&v["x"])),
        "Aspect" => GridSize::Aspect(v["index"].as_i64().unwrap() as i32, f(&v["ratio"])),
        t => panic!("unknown size {t}"),
    }
}

fn gap(v: &Value) -> Gap {
    match v["type"].as_str().unwrap() {
        "Fixed" => Gap::Fixed(f(&v["x"])),
        _ => Gap::Relative(f(&v["x"])),
    }
}

fn block_size(v: &Value) -> BlockSize {
    match v["type"].as_str().unwrap() {
        "Nothing" => BlockSize::Fill,
        "Fixed" => BlockSize::Fixed(f(&v["x"])),
        "Relative" => BlockSize::Relative(f(&v["x"])),
        "Auto" => BlockSize::Auto,
        t => panic!("unknown size attribute {t}"),
    }
}

fn alignmode(v: &Value) -> AlignMode {
    let mixed = |s: &Value| match (s.get("pad"), s.get("protrusion")) {
        (Some(p), _) => MixedSide::Pad(f(p)),
        (_, Some(p)) => MixedSide::Protrusion(f(p)),
        _ => MixedSide::Inside,
    };
    match v["type"].as_str().unwrap() {
        "Inside" => AlignMode::Inside,
        "Outside" => AlignMode::Outside(sides(&v["padding"])),
        _ => {
            let s = &v["sides"];
            AlignMode::Mixed { left: mixed(&s[0]), right: mixed(&s[1]), bottom: mixed(&s[2]), top: mixed(&s[3]) }
        }
    }
}

/// A reference block: name for messages, Makie's computed bbox and Axis viewport.
struct Expected {
    what: String,
    bbox: BBox,
    viewport: Option<BBox>,
}

fn grid(v: &Value, path: &str, expected: &mut Vec<Expected>) -> Grid {
    let arr = |k: &str| v[k].as_array().unwrap();
    let mut g = Grid::new(v["nrows"].as_u64().unwrap() as usize, v["ncols"].as_u64().unwrap() as usize);
    g.rowsizes = arr("rowsizes").iter().map(grid_size).collect();
    g.colsizes = arr("colsizes").iter().map(grid_size).collect();
    g.rowgaps = arr("rowgaps").iter().map(gap).collect();
    g.colgaps = arr("colgaps").iter().map(gap).collect();
    g.alignmode = alignmode(&v["alignmode"]);
    g.equalprotrusiongaps =
        [v["equalprotrusiongaps"][0].as_bool().unwrap(), v["equalprotrusiongaps"][1].as_bool().unwrap()];
    g.width = block_size(&v["width"]);
    g.height = block_size(&v["height"]);
    g.tellwidth = v["tellwidth"].as_bool().unwrap();
    g.tellheight = v["tellheight"].as_bool().unwrap();
    g.halign = f(&v["halign"]);
    g.valign = f(&v["valign"]);
    for (i, c) in arr("content").iter().enumerate() {
        let (rows, cols, side) = (span(&c["span"]["rows"]), span(&c["span"]["cols"]), side(&c["side"]));
        let kind = c["kind"].as_str().unwrap();
        let what = format!("{path}/{i}:{kind}[{rows:?},{cols:?},{side:?}]");
        if kind == "GridLayout" {
            let mut sub = grid(&c["grid"], &what, expected);
            (sub.rows, sub.cols, sub.side) = (rows, cols, side);
            g.content.push(Content::Grid(sub));
        } else {
            g.content.push(Content::Block(LayoutItem {
                rows,
                cols,
                side,
                protrusion: sides(&c["protrusions"]),
                width: block_size(&c["width"]),
                height: block_size(&c["height"]),
                autosize: [opt(&c["autosize"][0]), opt(&c["autosize"][1])],
                tellwidth: c["tellwidth"].as_bool().unwrap(),
                tellheight: c["tellheight"].as_bool().unwrap(),
                halign: f(&c["halign"]),
                valign: f(&c["valign"]),
                alignmode: alignmode(&c["alignmode"]),
                round: kind == "Axis",
            }));
            expected.push(Expected { what, bbox: bbox(&c["computedbbox"]), viewport: c.get("viewport").map(bbox) });
        }
    }
    g
}

fn close(a: BBox, b: BBox) -> bool {
    (a.l - b.l).abs() <= TOL && (a.r - b.r).abs() <= TOL && (a.b - b.b).abs() <= TOL && (a.t - b.t).abs() <= TOL
}

#[test]
fn gridlayoutbase_fixtures() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/layout.json")).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let figs = doc["figures"].as_array().unwrap();
    assert!(figs.len() >= 15);
    let (mut checks, mut failures) = (0usize, Vec::new());
    let mut figs_ok = 0;
    for fig in figs {
        let name = fig["name"].as_str().unwrap();
        let size = [f(&fig["size"][0]), f(&fig["size"][1])];
        let mut expected = Vec::new();
        let g = grid(&fig["layout"], name, &mut expected);
        let got = g.solve_root(size);
        assert_eq!(got.len(), expected.len(), "{name}: leaf count");
        let before = failures.len();
        for (b, e) in got.iter().zip(&expected) {
            checks += 1;
            if !close(*b, e.bbox) {
                failures.push(format!("{}: bbox {b:?}, Makie {:?}", e.what, e.bbox));
            }
            if let Some(vp) = e.viewport {
                checks += 1;
                if b.round() != vp {
                    failures.push(format!("{}: viewport {:?}, Makie {vp:?}", e.what, b.round()));
                }
            }
        }
        // resize_to_layout!: the tight bbox and the integer figure size Makie picks from it.
        let tight = g.tight_bbox(size);
        let want = bbox(&fig["tight_bbox"]);
        checks += 1;
        if (tight.width() - want.width()).abs() > TOL || (tight.height() - want.height()).abs() > TOL {
            failures.push(format!("{name}: tight bbox {tight:?}, Makie {want:?}"));
        }
        let resized = [tight.width().round_ties_even(), tight.height().round_ties_even()];
        checks += 1;
        if resized != [f(&fig["resized_size"][0]), f(&fig["resized_size"][1])] {
            failures.push(format!("{name}: resized size {resized:?}, Makie {}", fig["resized_size"]));
        }
        figs_ok += usize::from(failures.len() == before);
    }
    println!("layout fixtures: {figs_ok}/{} figures, {}/{checks} checks pass", figs.len(), checks - failures.len());
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}
