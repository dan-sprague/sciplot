//! Side-by-side comparison of the gallery against CairoMakie (plan §7).
//!
//! Reads `target/gallery/<name>.png` (from `examples/gallery.rs`) and
//! `target/gallery/makie/<name>.png` (from `tools/makie_gallery.jl`), writes a diff image per
//! page to `target/gallery/diff/`, a contact sheet `target/gallery/index.html` (sciplot | Makie |
//! diff) and prints per-page pixel statistics:
//!
//! - `mean`: mean absolute difference over all pixels and RGB channels, in % of full scale;
//! - `>32`: % of pixels whose largest channel difference exceeds 32/255;
//! - `>32±1`: the same, but a pixel only counts when no pixel of the other image within ±1 px
//!   matches it (ignores antialiasing and sub-pixel offsets, so it measures real differences).
//!
//! In the diff image, white means equal; red marks pixels where sciplot is darker (ink only in
//! sciplot), blue where Makie is darker.
//!
//! `cargo run --release --example compare [-- names...]`
use std::fmt::Write as _;
use std::io::BufReader;
use std::path::{Path, PathBuf};

const ROOT: &str = "target/gallery";

/// An RGB image composited on white.
struct Img {
    w: usize,
    h: usize,
    px: Vec<[u8; 3]>,
}

impl Img {
    fn get(&self, x: usize, y: usize) -> [u8; 3] {
        if x < self.w && y < self.h { self.px[y * self.w + x] } else { [255; 3] }
    }
}

fn load(path: &Path) -> Result<Img, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut dec = png::Decoder::new(BufReader::new(f));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0u8; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let over = |c: u8, a: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32) + 127) / 255) as u8;
    let px = match info.color_type {
        png::ColorType::Rgba => {
            buf.chunks_exact(4).map(|p| [over(p[0], p[3]), over(p[1], p[3]), over(p[2], p[3])]).collect()
        }
        png::ColorType::Rgb => buf.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).map(|p| [over(p[0], p[1]); 3]).collect(),
        png::ColorType::Grayscale => buf.iter().map(|g| [*g; 3]).collect(),
        png::ColorType::Indexed => return Err(format!("{}: unexpanded palette", path.display())),
    };
    let mut px: Vec<[u8; 3]> = px;
    px.truncate(w * h);
    Ok(Img { w, h, px })
}

fn maxdiff(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
}

fn luma(p: [u8; 3]) -> f64 {
    0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
}

struct Stats {
    size_e: (usize, usize),
    size_m: (usize, usize),
    mean: f64,
    over32: f64,
    over32_shift: f64,
}

/// Compares two images on their union canvas; returns the stats and the diff image (RGB).
fn compare(e: &Img, m: &Img) -> (Stats, usize, usize, Vec<u8>) {
    let (w, h) = (e.w.max(m.w), e.h.max(m.h));
    let mut sum = 0u64;
    let (mut n32, mut n32s) = (0usize, 0usize);
    let mut out = vec![255u8; w * h * 3];
    // Smallest max-channel difference between `p` and the 3×3 neighbourhood of (x, y) in `img`.
    let near = |img: &Img, x: usize, y: usize, p: [u8; 3]| -> u8 {
        let mut best = 255u8;
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let (xx, yy) = (x as i64 + dx, y as i64 + dy);
                if xx < 0 || yy < 0 {
                    continue;
                }
                best = best.min(maxdiff(p, img.get(xx as usize, yy as usize)));
            }
        }
        best
    };
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (e.get(x, y), m.get(x, y));
            sum += (0..3).map(|c| a[c].abs_diff(b[c]) as u64).sum::<u64>();
            let d = maxdiff(a, b);
            if d > 32 {
                n32 += 1;
                if near(m, x, y, a) > 32 && near(e, x, y, b) > 32 {
                    n32s += 1;
                }
            }
            // Visualization: white = equal; red = sciplot darker; blue = Makie darker.
            let k = (d as f64 * 2.0).min(255.0) as u8;
            let o = (y * w + x) * 3;
            let px = if luma(a) <= luma(b) { [255, 255 - k, 255 - k] } else { [255 - k, 255 - k, 255] };
            out[o..o + 3].copy_from_slice(&px);
        }
    }
    let n = (w * h).max(1) as f64;
    let stats = Stats {
        size_e: (e.w, e.h),
        size_m: (m.w, m.h),
        mean: sum as f64 / (n * 3.0 * 255.0) * 100.0,
        over32: n32 as f64 / n * 100.0,
        over32_shift: n32s as f64 / n * 100.0,
    };
    (stats, w, h, out)
}

fn write_png(path: &Path, w: usize, h: usize, rgb: &[u8]) -> Result<(), String> {
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(rgb).map_err(|e| e.to_string())
}

/// Page names in gallery order (`data/pages.json`), else every sciplot PNG.
fn page_names() -> Vec<String> {
    let listed = std::fs::read_to_string(Path::new(ROOT).join("data/pages.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok());
    listed.unwrap_or_else(|| {
        let mut v: Vec<String> = std::fs::read_dir(ROOT)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().to_str()?.strip_suffix(".png").map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    })
}

fn main() {
    let filters: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(ROOT);
    let _ = std::fs::create_dir_all(root.join("diff"));
    let mut rows = String::new();
    let mut summary = String::new();
    println!("{:<20} {:>11} {:>11} {:>7} {:>7} {:>7}", "page", "sciplot px", "makie px", "mean%", ">32%", ">32±1%");
    for name in page_names() {
        if !filters.is_empty() && !filters.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let ep = root.join(format!("{name}.png"));
        let mp = root.join("makie").join(format!("{name}.png"));
        let (e, m) = match (load(&ep), load(&mp)) {
            (Ok(e), Ok(m)) => (e, m),
            (a, b) => {
                let why = [a.err(), b.err()].into_iter().flatten().collect::<Vec<_>>().join("; ");
                println!("{name:<20} missing: {why}");
                continue;
            }
        };
        let (s, w, h, diff) = compare(&e, &m);
        let dp = root.join("diff").join(format!("{name}.png"));
        if let Err(err) = write_png(&dp, w, h, &diff) {
            println!("{name}: writing the diff failed: {err}");
        }
        let size = |(w, h): (usize, usize)| format!("{w}×{h}");
        let mismatch = if s.size_e != s.size_m { " SIZE MISMATCH" } else { "" };
        println!(
            "{name:<20} {:>11} {:>11} {:>7.2} {:>7.2} {:>7.2}{mismatch}",
            size(s.size_e),
            size(s.size_m),
            s.mean,
            s.over32,
            s.over32_shift
        );
        let _ = writeln!(
            summary,
            "<tr><td><a href=\"#{name}\">{name}</a></td><td>{}</td><td>{}</td><td>{:.2}</td><td>{:.2}</td><td>{:.2}</td></tr>",
            size(s.size_e),
            size(s.size_m),
            s.mean,
            s.over32,
            s.over32_shift
        );
        let svg = if root.join(format!("{name}.svg")).exists() {
            format!(" · <a href=\"{name}.svg\">sciplot SVG</a>")
        } else {
            String::new()
        };
        let _ = writeln!(
            rows,
            "<section id=\"{name}\"><h2>{name}</h2><p>mean {:.2}% · &gt;32: {:.2}% · &gt;32 (±1 px): {:.2}%{mismatch}{svg}</p>\
             <div class=\"row\"><figure><a href=\"{name}.png\"><img src=\"{name}.png\"></a><figcaption>sciplot</figcaption></figure>\
             <figure><a href=\"makie/{name}.png\"><img src=\"makie/{name}.png\"></a><figcaption>CairoMakie</figcaption></figure>\
             <figure><a href=\"diff/{name}.png\"><img src=\"diff/{name}.png\"></a><figcaption>diff (red: sciplot darker, blue: Makie darker)</figcaption></figure></div></section>",
            s.mean, s.over32, s.over32_shift
        );
    }
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\">\
         <title>sciplot vs CairoMakie</title><style>\
         body{{font-family:system-ui,sans-serif;margin:16px;background:#f4f4f4;color:#222}}\
         table{{border-collapse:collapse}}td,th{{padding:2px 10px;text-align:right}}td:first-child{{text-align:left}}\
         .row{{display:grid;grid-template-columns:repeat(3,1fr);gap:8px}}figure{{margin:0;background:#fff;padding:4px}}\
         img{{width:100%;display:block}}figcaption{{font-size:12px;color:#666}}h2{{margin:24px 0 4px}}</style></head><body>\
         <h1>sciplot vs CairoMakie</h1><p>Mean absolute difference (% of full scale); % of pixels differing by more than \
         32/255, and the same ignoring ±1 px offsets.</p>\
         <table><tr><th>page</th><th>sciplot</th><th>Makie</th><th>mean %</th><th>&gt;32 %</th><th>&gt;32 ±1 %</th></tr>{summary}</table>\
         {rows}</body></html>"
    );
    let index = root.join("index.html");
    match std::fs::write(&index, html) {
        Ok(()) => println!("wrote {}", index.display()),
        Err(e) => println!("writing {} failed: {e}", index.display()),
    }
}
