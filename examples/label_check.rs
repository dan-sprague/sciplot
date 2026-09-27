//! Labels: super title in a prepended row, panel labels at TopLeft, explicit ticks.
use sciplot::prelude::*;

fn main() -> sciplot::Result<()> {
    let fig = Figure!(size = (800, 500));
    let a = Axis!(fig.at(1, 1); title = "ω = 1", xlabel = "t (s)", ylabel = "u (V)");
    let b = Axis!(fig.at(1, 2); title = "ω = 2", xlabel = "t (s)",
        xticks = ([0.0, 5.0, 10.0], ["start", "mid", "end"]), yticks = [-1.0, 0.0, 1.0]);
    let t = linspace(0.0, 10.0, 200);
    lines!(a, &t, t.iter().map(|t| t.sin()));
    lines!(b, &t, t.iter().map(|t| (2.0 * t).sin()); color = WONG[1]);
    Label!(fig.at(Prepend, ..), "Driven oscillators"; fontsize = 20, font = Font::Bold);
    for (pos, s) in [(fig.at(2, 1), "A"), (fig.at(2, 2), "B")] {
        Label!(pos.side(Side::TopLeft), s; fontsize = 18, font = Font::Bold, padding = (0, 5, 5, 0), halign = HAlign::Right);
    }
    std::fs::create_dir_all("out").ok();
    fig.save("out/label_check.png")?;
    fig.save("out/label_check.svg")
}
