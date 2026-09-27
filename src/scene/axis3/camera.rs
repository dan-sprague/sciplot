//! Makie's Axis3 camera (`calculate_matrices` and `projectionmatrix` in
//! `makielayout/blocks/axis3d.jl`), in f64.
//!
//! The model matrix maps the limits box to a box centred at the origin whose longest side (after
//! `aspect`) spans -1..1; the camera sits on a sphere around it at `azimuth`/`elevation` and looks
//! at the origin with a perspective projection (at least 0.5° field of view, so
//! `perspectiveness = 0` is nearly orthographic). `viewmode` then scales the projection so the box
//! fits the scene area minus the protrusions. Clip space is OpenGL's (-1..1 in x, y and z, y up).
//!
//! Provenance: ported from Makie 0.24.14 `src/makielayout/blocks/axis3d.jl` (`calculate_matrices`,
//! `projectionmatrix`) and `src/camera/projection_math.jl` (`lookat`, `frustum`,
//! `perspectiveprojection`, `transformationmatrix`); `mod1` follows Julia 1.12.7
//! `base/operators.jl`. MIT licensed; see THIRD_PARTY_NOTICES.md.

/// A 4×4 matrix, row-major: `m[row][col]` (Makie prints and multiplies the same way).
pub type M4 = [[f64; 4]; 4];

/// Makie's Axis3 `aspect`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aspect3 {
    /// `:equal`: the box is a cube whatever the data.
    Equal,
    /// `:data`: one data unit is as long on every axis.
    Data,
    /// `(a, b, c)`: the box's side lengths are in this ratio (Makie's default `(1, 1, 2/3)`).
    Ratio(f64, f64, f64),
}

/// Makie's Axis3 `viewmode`: how the projected box is fitted into the scene area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Scale uniformly so the bounding sphere fits (the size does not change while rotating).
    Fit,
    /// Scale uniformly so the projected box touches the area (Makie's default).
    FitZoom,
    /// Scale x and y independently so the projected box fills the area.
    Stretch,
    /// Like `Fit`, with a translation (`axis_offset`) for free panning.
    Free,
}

/// The camera of one Axis3 for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrices {
    /// Data -> world (the normalized box).
    pub model: M4,
    /// World -> eye.
    pub view: M4,
    /// Eye -> clip (including the viewmode fit and the protrusion offset).
    pub projection: M4,
    /// Where the camera looks (world).
    pub lookat: [f64; 3],
    /// Camera position (world).
    pub eyepos: [f64; 3],
}

/// Everything `calculate_matrices` depends on.
#[derive(Clone, Copy, Debug)]
pub struct CameraParams {
    /// `[x0, x1, y0, y1, z0, z1]` with `x0 < x1` etc.
    pub limits: [f64; 6],
    /// Scene area width and height (figure units).
    pub viewport: [f64; 2],
    /// `(left, right, bottom, top)`.
    pub protrusions: [f64; 4],
    pub elevation: f64,
    pub azimuth: f64,
    pub perspectiveness: f64,
    pub aspect: Aspect3,
    pub viewmode: ViewMode,
    pub reversed: [bool; 3],
    /// Scroll zoom multiplier (1 = none; smaller zooms in).
    pub zoom_mult: f64,
    /// `viewmode = Free` translation.
    pub offset: [f64; 2],
    /// Makie's `near` (1e-3).
    pub near: f64,
}

pub fn identity() -> M4 {
    let mut m = [[0.0; 4]; 4];
    for (i, r) in m.iter_mut().enumerate() {
        r[i] = 1.0;
    }
    m
}

pub fn mul(a: &M4, b: &M4) -> M4 {
    let mut m = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            m[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// `m * (p, 1)`.
pub fn apply(m: &M4, p: [f64; 3]) -> [f64; 4] {
    let v = [p[0], p[1], p[2], 1.0];
    std::array::from_fn(|i| (0..4).map(|k| m[i][k] * v[k]).sum())
}

fn translation(t: [f64; 3]) -> M4 {
    let mut m = identity();
    for i in 0..3 {
        m[i][3] = t[i];
    }
    m
}

fn scale(s: [f64; 3]) -> M4 {
    let mut m = identity();
    for i in 0..3 {
        m[i][i] = s[i];
    }
    m
}

/// Makie's `transformationmatrix(translation, scale)`.
fn translate_scale(t: [f64; 3], s: [f64; 3]) -> M4 {
    let mut m = scale(s);
    for i in 0..3 {
        m[i][3] = t[i];
    }
    m
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / n, a[1] / n, a[2] / n]
}

/// Makie's `lookat(eye, target, up)`.
pub fn lookat(eye: [f64; 3], target: [f64; 3], up: [f64; 3]) -> M4 {
    let z = normalize(sub(eye, target));
    let x = normalize(cross(up, z));
    let y = normalize(cross(z, x));
    let basis = [[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], [0.0, 0.0, 0.0, 1.0]];
    mul(&basis, &translation([-eye[0], -eye[1], -eye[2]]))
}

/// Makie's `frustum`.
fn frustum(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> M4 {
    if right == left || bottom == top || near == far {
        return identity();
    }
    [
        [2.0 * near / (right - left), 0.0, (right + left) / (right - left), 0.0],
        [0.0, 2.0 * near / (top - bottom), (top + bottom) / (top - bottom), 0.0],
        [0.0, 0.0, -(far + near) / (far - near), -2.0 * near * far / (far - near)],
        [0.0, 0.0, -1.0, 0.0],
    ]
}

/// Makie's `perspectiveprojection(fovy (degrees), aspect, near, far)`.
fn perspective(fovy: f64, aspect: f64, near: f64, far: f64) -> M4 {
    let h = (fovy / 360.0 * std::f64::consts::PI).tan() * near;
    let w = h * aspect;
    frustum(-w, w, -h, h, near, far)
}

/// Makie's `calculate_matrices` for Axis3.
pub fn calculate_matrices(p: &CameraParams) -> Matrices {
    let l = p.limits;
    let mut origin = [l[0], l[2], l[4]];
    let mut ws = [l[1] - l[0], l[3] - l[2], l[5] - l[4]];
    for i in 0..3 {
        if p.reversed[i] {
            origin[i] += ws[i];
            ws[i] = -ws[i];
        }
    }
    let norm = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let (mut scales, axis_radius) = match p.aspect {
        Aspect3::Equal => (ws.map(|w| 2.0 / w), 3f64.sqrt()),
        Aspect3::Data => {
            let m = ws[0].max(ws[1]).max(ws[2]);
            (ws.map(|w| 2.0 * w.signum() / m), norm(ws.map(|w| w / m)))
        }
        Aspect3::Ratio(a, b, c) => {
            let m = a.max(b).max(c);
            let n = [a / m, b / m, c / m];
            (std::array::from_fn(|i| 2.0 / ws[i] * n[i]), norm(n))
        }
    };
    // Do not allow dimensions to collapse.
    for s in &mut scales {
        if s.abs() < f32::MIN_POSITIVE as f64 || !s.is_finite() {
            *s = if *s < 0.0 { -1.0 } else { 1.0 };
        }
    }
    let model = mul(
        &mul(&translation(std::array::from_fn(|i| -0.5 * ws[i] * scales[i])), &scale(scales)),
        &translation(origin.map(|o| -o)),
    );

    let fov = 0.5 + (90.0 - 0.5) * p.perspectiveness.clamp(0.0, 1.0);
    let radius = p.zoom_mult * axis_radius / (fov / 2.0).to_radians().sin();
    let (se, ce) = p.elevation.sin_cos();
    let (sa, ca) = p.azimuth.sin_cos();
    let camdir = [ce * ca, ce * sa, se];
    let mut eyepos = camdir.map(|c| radius * c);
    let lookat_pt = if p.viewmode == ViewMode::Free {
        let u_z = camdir;
        let u_x = normalize(cross([0.0, 0.0, 1.0], u_z));
        let u_y = cross(u_z, u_x);
        let s = p.zoom_mult * axis_radius;
        let la: [f64; 3] = std::array::from_fn(|i| s * (p.offset[0] * u_x[i] + p.offset[1] * u_y[i]));
        for i in 0..3 {
            eyepos[i] += la[i];
        }
        la
    } else {
        [0.0; 3]
    };
    let view = lookat(eyepos, lookat_pt, [0.0, 0.0, 1.0]);

    let box_origin = origin;
    let corners: Vec<[f64; 3]> = (0..8)
        .map(|k| std::array::from_fn(|i| if k >> (2 - i) & 1 == 1 { box_origin[i] + ws[i] } else { box_origin[i] }))
        .collect();
    let projection = projection_matrix(p, &mul(&view, &model), &corners, radius, fov, axis_radius);
    Matrices { model, view, projection, lookat: lookat_pt, eyepos }
}

/// Makie's Axis3 `projectionmatrix`.
fn projection_matrix(
    p: &CameraParams,
    viewmodel: &M4,
    corners: &[[f64; 3]],
    radius: f64,
    fov: f64,
    axis_radius: f64,
) -> M4 {
    let near = p.near.max(radius - axis_radius);
    let far = ((1.0 + 1.0e-3) * near).max(radius + axis_radius);
    let [width, height] = p.viewport;
    let aspect_ratio = width / height;
    let fov = if height > width { fov / aspect_ratio } else { fov };
    let pm = perspective(fov, aspect_ratio, near, far);
    let [pl, pr, pb, pt] = p.protrusions;
    let dx = (pl - pr) / width;
    let dy = (pb - pt) / height;
    let w = width - pl - pr;
    let h = height - pb - pt;
    match p.viewmode {
        ViewMode::FitZoom | ViewMode::Stretch => {
            let pv = mul(&pm, viewmodel);
            let (w_eff, h_eff) = (w / width, h / height);
            let (mut maxx, mut maxy) = (0.0f64, 0.0f64);
            for c in corners {
                let q = apply(&pv, *c);
                maxx = maxx.max((q[0] / (w_eff * q[3])).abs());
                maxy = maxy.max((q[1] / (h_eff * q[3])).abs());
            }
            let (rx, ry) = (1.0 / maxx, 1.0 / maxy);
            let s = if p.viewmode == ViewMode::FitZoom { [rx.min(ry); 2] } else { [rx, ry] };
            mul(&translate_scale([dx, dy, 0.0], [s[0], s[1], 1.0]), &pm)
        }
        ViewMode::Fit | ViewMode::Free => {
            let wh = w.min(h) / width.min(height);
            mul(&translate_scale([dx, dy, 0.0], [wh, wh, 1.0]), &pm)
        }
    }
}

/// Julia's `mod1(x, m)`: the value in `(0, m]` congruent to `x`.
pub fn mod1(x: f64, m: f64) -> f64 {
    let r = x.rem_euclid(m);
    if r == 0.0 { m } else { r }
}
