//! A static figure for comparing the browser canvas (WebGPU and WebGL2) with the native PNG
//! export pixel by pixel (`tools/web/check.sh`).
//!
//! Native: `cargo run --example web_static` writes `out/web_static_native.png` at
//! 2 px per unit (the capture's device pixel ratio).
//! Browser: mounts the figure into `<canvas id="ezviz">` (`examples/web/web_static.html`), then
//! renders it again offscreen with `Figure::to_png_bytes_async` and shows that PNG in an `<img
//! id="png">` below the canvas, so one page capture checks both paths.
use ezviz::prelude::*;

/// The figure: lines, scatter, legend and text on the left, a heatmap with a colorbar on the
/// right. Deterministic data.
fn figure() -> Figure {
    let fig = Figure::new().size((720, 360));
    let ax = Axis::new(fig.at(1, 1)).title("lines and scatter").xlabel("t").ylabel("u");
    let t: Vec<f64> = (0..=120).map(|i| i as f64 / 12.0).collect();
    ax.lines(&t, t.iter().map(|t| (-0.2 * t).exp() * (2.0 * t).cos())).label("damped");
    ax.lines(&t, t.iter().map(|t| 0.5 * (0.7 * t).sin())).linestyle(Linestyle::Dash).label("slow");
    let ts: Vec<f64> = (0..=20).map(|i| i as f64 / 2.0).collect();
    ax.scatter(&ts, ts.iter().map(|t| 0.8 * (-0.3 * t).exp())).markersize(8).label("decay");
    axislegend(&ax);
    let hax = Axis::new(fig.at(1, 2)).title("field").xlabel("x").ylabel("y");
    let n = 48;
    let z: Vec<f64> = (0..n * n)
        .map(|k| {
            let (i, j) = ((k % n) as f64 / n as f64, (k / n) as f64 / n as f64);
            (6.0 * i).sin() * (4.0 * j).cos() + i * j
        })
        .collect();
    let hm = hax.heatmap(Field::new(&z, n, n)).colormap(Colormap::VIRIDIS);
    Colorbar::new(fig.at(1, 3), &hm);
    fig
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> ezviz::Result<()> {
    std::fs::create_dir_all("out")?;
    figure().save_with("out/web_static_native.png", Save::new().px_per_unit(2))?;
    println!("wrote out/web_static_native.png");
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn main() -> ezviz::Result<()> {
    let fig = figure();
    fig.show_in("ezviz")?;
    // A second figure on the same page, in a canvas ezviz appends (the page's CSS moves it out
    // of the captured area): it reaches the running event loop through its proxy and gets its
    // own GPU context on WebGL2.
    let small = Figure::new().size((300, 200));
    let ax = Axis::new(small.at(1, 1)).title("second figure");
    ax.scatter([1.0, 2.0, 3.0], [2.0, 1.0, 3.0]).markersize(12);
    small.show()?;
    // The offscreen GPU path: the same figure as a PNG, rendered and read back asynchronously.
    wasm_bindgen_futures::spawn_local(async move {
        let src = match fig.to_png_bytes_async(&Save::new().px_per_unit(2)).await {
            Ok(png) => {
                use base64::Engine as _;
                format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
            }
            Err(e) => {
                log::error!("to_png_bytes_async failed: {e}");
                return;
            }
        };
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
        let (Ok(img), Some(body)) = (doc.create_element("img"), doc.body()) else { return };
        let _ = img.set_attribute("id", "png");
        let _ = img.set_attribute("style", "display:block;width:720px;height:360px");
        let _ = img.set_attribute("src", &src);
        let _ = body.append_child(&img);
    });
    Ok(())
}
