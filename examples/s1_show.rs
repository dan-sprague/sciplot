//! S1: one-liner that opens a window.
use sciplot::prelude::*;

fn main() -> sciplot::Result<()> {
    let x: Vec<f64> = linspace(0.0, 10.0, 400);
    scatter(&x, x.iter().map(|t| t.sin() * (-0.2 * t).exp())).markersize(6).show()
}
