//! Lowering plots to draw-list primitives.

use super::drawlist::{Buf, BufKey, Emitter, MarkersPrim, Prim, PrimColor, Space};
use super::{AxisFrame, SceneCache};
use crate::color::Color;
use crate::figure::FigState;
use crate::plots::{ColorSpec, CycleGroup, PlotKind};
use crate::theme::Globals;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Conversion key: data revision + rebase epoch + scales.
fn conv_key(data_rev: u64, a: &AxisFrame) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (data_rev, a.rebase.epoch, a.attrs.xscale, a.attrs.yscale).hash(&mut h);
    h.finish()
}

/// Scaled + rebased local coordinates for data points (NaN for points outside the scale domain).
pub(crate) fn to_local(pts: &[[f64; 2]], a: &AxisFrame) -> Vec<[f32; 2]> {
    let (xs, ys) = (a.attrs.xscale, a.attrs.yscale);
    pts.iter()
        .map(|p| {
            let (sx, sy) = (xs.forward(p[0]), ys.forward(p[1]));
            if sx.is_finite() && sy.is_finite() {
                a.rebase.to_local(sx, sy)
            } else {
                [f32::NAN, f32::NAN]
            }
        })
        .collect()
}

/// Resolves a cycled color.
pub(crate) fn resolve_color(spec: &ColorSpec, cycle: usize, palette: &[Color]) -> Option<Color> {
    let n = palette.len().max(1);
    match spec {
        ColorSpec::Auto => Some(palette[cycle % n]),
        ColorSpec::Solid(c) => Some(*c),
        ColorSpec::Cycled(i) => Some(palette[(i.max(&1) - 1) % n]),
        _ => None,
    }
}

pub(crate) fn emit_plots(
    em: &mut Emitter,
    st: &FigState,
    a: &AxisFrame,
    g: &Globals,
    cache: &mut SceneCache,
) {
    let Some(ax) = st.block(a.id).and_then(|b| b.as_axis()) else {
        return;
    };
    let mut counters: HashMap<CycleGroup, usize> = HashMap::new();
    for pid in &ax.plots {
        let Some(p) = st.plot(*pid) else { continue };
        // Cycle index: only plots whose cycled attribute is automatic advance the counter.
        let group = p.kind.cycle_group();
        let auto = p.kind.color_is_auto(&st.theme);
        let cycle = if auto {
            let c = counters.entry(group).or_insert(0);
            *c += 1;
            *c - 1
        } else {
            0
        };
        if !p.common.visible {
            continue;
        }
        let space = Space::Data(a.slot);
        let clip = Some(a.rect);
        match &p.kind {
            PlotKind::Scatter(s) => {
                let r = s.attrs.resolve(&st.theme.scatter, g);
                let key = conv_key(p.data_rev, a);
                let pos = cache.convert(p.uid, 0, key, || to_local(&s.pos, a));
                let alpha = r.alpha as f32;
                let color = match resolve_color(&r.color, cycle, &g.palette) {
                    Some(c) => PrimColor::Uniform(c.with_alpha(c.a * alpha)),
                    None => match &r.color {
                        ColorSpec::PerPoint(cs) => PrimColor::PerElement(Buf {
                            key: Some(BufKey {
                                uid: p.uid,
                                part: 1,
                                rev: p.data_rev,
                            }),
                            data: std::sync::Arc::new(
                                cs.iter()
                                    .map(|c| c.with_alpha(c.a * alpha).to_premul_u32())
                                    .collect(),
                            ),
                        }),
                        // Values -> colormap: handled with the field pipeline in M5.
                        _ => PrimColor::Uniform(g.palette[0]),
                    },
                };
                em.push(
                    p.common.z,
                    clip,
                    space,
                    Prim::Markers(MarkersPrim {
                        pos: Buf {
                            key: Some(BufKey {
                                uid: p.uid,
                                part: 0,
                                rev: key,
                            }),
                            data: pos,
                        },
                        color,
                        size: r.markersize as f32,
                        sizes: None,
                        marker: r.marker,
                        stroke_color: r.strokecolor.with_alpha(r.strokecolor.a * alpha),
                        stroke_width: r.strokewidth as f32,
                        rotation: r.rotation as f32,
                    }),
                );
            }
        }
    }
}
