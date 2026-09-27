//! Bit-exact ports of the Julia 1.12 `Base` numerics that Makie's tick code relies on.
//!
//! PlotUtils and Makie decide ties between tick candidates with exact `Float64` arithmetic, so
//! the port reproduces Julia's own implementations instead of calling the platform libm:
//! - `10.0^n` (compensated power by squaring, `base/math.jl` `pow_body(::Float64, ::Integer)`),
//! - `log10`/`log2`/`log` (Tang's table method, `base/special/log.jl`),
//! - `exp10`/`exp2`/`exp` (`base/special/exp.jl` `exp_impl`),
//! - `round(x; sigdigits)`, `floor(x; digits)` (`base/floatfuncs.jl`),
//! - float ranges `a:s:b` and `range(a, b; length)` with `TwicePrecision` (`base/twiceprecision.jl`).
//!
//! Julia's `muladd` compiles to a fused multiply-add on the platforms we target (aarch64,
//! x86-64 with FMA), so it is `mul_add` here. Negated float comparisons (`!(a <= b)`) mirror the
//! Julia source and its NaN behavior.
//!
//! Provenance: ported from Julia 1.12.7 Base: `base/math.jl` (`two_mul`,
//! `pow_body(::Float64, ::Integer)`, the integer-exponent path of `^(::Float64, ::Float64)`),
//! `base/special/log.jl` (`log_proc1`, `log_proc2`, `_log`), `base/special/exp.jl` (`exp_impl` with
//! its range constants and `expm1b_kernel` polynomials), `base/float.jl` (`eps`),
//! `base/floatfuncs.jl` (`isapprox`, `_round_digits`, `_round_invstep`, `_round_invstepsqrt`,
//! `_round_step`, `hidigit`, `_round_sigdigits`) and `base/twiceprecision.jl` (`TwicePrecision`
//! arithmetic, `steprangelen_hp`, `nbitslen`, `floatrange`, `(:)`, `range_start_stop_length`,
//! `_linspace`, `_linspace1`, `rat`, `lcm_unchecked`, `isbetween`). MIT licensed; see
//! THIRD_PARTY_NOTICES.md.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::julia_tables::{J_TABLE, T_LOG};

// ---------------------------------------------------------------------------------------------
// Powers

/// Julia's `x^n` for `x::Float64, n::Integer` (`n` in the power-by-squaring range).
pub(crate) fn powi(x: f64, n: i64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if (-(1 << 12)..=3 * (1 << 13)).contains(&n) {
        return pow_body_int(x, n);
    }
    // Outside the squaring range Julia goes through the float power; use libm (never hit by the
    // tick code, whose exponents stay within +-400).
    x.powf(n as f64)
}

/// `two_mul` with FMA: `x*y == hi + lo` exactly.
#[inline]
fn two_mul(x: f64, y: f64) -> (f64, f64) {
    let hi = x * y;
    (hi, x.mul_add(y, -hi))
}

fn pow_body_int(mut x: f64, mut n: i64) -> f64 {
    let mut y = 1.0f64;
    let mut xnlo = -0.0f64;
    let mut ynlo = 0.0f64;
    if n == 3 {
        return x * x * x;
    }
    if n < 0 {
        let rx = 1.0 / x;
        if n == -2 {
            return rx * rx;
        }
        if x.is_finite() {
            xnlo = -x.mul_add(rx, -1.0) * rx;
        }
        x = rx;
        n = -n;
    }
    while n > 1 {
        if n & 1 > 0 {
            let err = y.mul_add(xnlo, x * ynlo);
            let (h, l) = two_mul(x, y);
            y = h;
            ynlo = l + err;
        }
        let err = x * 2.0 * xnlo;
        let (h, l) = two_mul(x, x);
        x = h;
        xnlo = l + err;
        n >>= 1;
    }
    let err = y.mul_add(xnlo, x * ynlo);
    if x.is_finite() && err.is_finite() { x.mul_add(y, err) } else { x * y }
}

/// Julia's `x^y` for two floats; exact for integer `y` (via [`powi`]), libm otherwise.
pub(crate) fn powf(x: f64, y: f64) -> f64 {
    if y == y.trunc() && y.abs() < 9.0e15 {
        let n = y as i64;
        if n == 0 {
            return 1.0;
        }
        if (-(1 << 12)..=3 * (1 << 13)).contains(&n) {
            return pow_body_int(x, n);
        }
    }
    x.powf(y)
}

// ---------------------------------------------------------------------------------------------
// Logarithms

#[derive(Clone, Copy)]
enum Base {
    Two,
    E,
    Ten,
}

impl Base {
    fn log_hi_lo(self) -> (f64, f64) {
        match self {
            Base::Two => (std::f64::consts::LOG2_E, 2.0355273740931033e-17),
            Base::E => (1.0, 0.0),
            Base::Ten => (std::f64::consts::LOG10_E, 1.098319650216765e-17),
        }
    }
}

fn log_proc1(y: f64, mf: f64, big_f: f64, f: f64, base: Base) -> f64 {
    let jp = (128.0 * big_f) as i64 - 127;
    let (hi, lo) = T_LOG[(jp - 1) as usize];
    let l_hi = mf * 0.6931471805601177 + hi;
    let l_lo = mf * -1.7239444525614835e-13 + lo;
    let u = (2.0 * f) / (y + big_f);
    let v = u * u;
    let q = u * v * v.mul_add(0.012500053168098584, 0.08333333333303913);
    let (m_hi, m_lo) = base.log_hi_lo();
    m_hi.mul_add(l_hi, m_hi.mul_add(u + (q + l_lo), m_lo * l_hi))
}

fn log_proc2(f: f64, base: Base) -> f64 {
    let g = 1.0 / (2.0 + f);
    let u = 2.0 * f * g;
    let v = u * u;
    let poly = v.mul_add(
        v.mul_add(v.mul_add(0.0004348877777076146, 0.0022321399879194482), 0.012500000003771751),
        0.08333333333333179,
    );
    let q = u * v * poly;
    let (m_hi, m_lo) = base.log_hi_lo();
    m_hi.mul_add(u, m_lo.mul_add(u, m_hi * (-u).mul_add(f, 2.0 * (f - u)).mul_add(g, q)))
}

fn log_impl(x: f64, base: Base) -> f64 {
    if x > 0.0 {
        if x == f64::INFINITY {
            return x;
        }
        if 0.9394130628134757 < x && x < 1.0644944589178595 {
            return log_proc2(x - 1.0, base);
        }
        let mut x = x;
        let mut xu = x.to_bits();
        let mut m = ((xu >> 52) & 0x07ff) as i64;
        if m == 0 {
            x *= 1.8014398509481984e16;
            xu = x.to_bits();
            m = ((xu >> 52) & 0x07ff) as i64 - 54;
        }
        m -= 1023;
        let y = f64::from_bits((xu & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
        let mf = m as f64;
        let big_f = (y + 3.5184372088832e13) - 3.5184372088832e13;
        let f = y - big_f;
        log_proc1(y, mf, big_f, f, base)
    } else if x == 0.0 {
        f64::NEG_INFINITY
    } else {
        // Julia throws a DomainError; NaN keeps the render path panic-free.
        f64::NAN
    }
}

/// Julia's `log10(::Float64)`.
pub(crate) fn log10(x: f64) -> f64 {
    log_impl(x, Base::Ten)
}
/// Julia's `log2(::Float64)`.
pub(crate) fn log2(x: f64) -> f64 {
    log_impl(x, Base::Two)
}
/// Julia's `log(::Float64)`.
pub(crate) fn ln(x: f64) -> f64 {
    log_impl(x, Base::E)
}

// ---------------------------------------------------------------------------------------------
// Exponentials

const MAGIC_ROUND: f64 = 6.755399441055744e15;

struct ExpConsts {
    max_exp: f64,
    min_exp: f64,
    subnorm_exp: f64,
    inv256: f64,
    u: f64,
    l: f64,
    kernel: [f64; 4],
}

fn exp_consts(base: Base) -> ExpConsts {
    match base {
        Base::Two => ExpConsts {
            max_exp: 1024.0,
            min_exp: -1075.0,
            subnorm_exp: 1022.0,
            inv256: 256.0,
            u: -0.00390625,
            l: 0.0,
            kernel: [0.6931471805599393, 0.24022650695910058, 0.05550411502333161, 0.009618129548366803],
        },
        Base::E => ExpConsts {
            max_exp: 709.7827128933841,
            min_exp: -745.1332191019412,
            subnorm_exp: 708.3964185322641,
            inv256: 369.3299304675746,
            u: -0.002707606173999011,
            l: -6.327543041662719e-14,
            kernel: [0.9999999999999912, 0.4999999999999997, 0.1666666857598779, 0.04166666857598777],
        },
        Base::Ten => ExpConsts {
            max_exp: 308.25471555991675,
            min_exp: -323.60724533877976,
            subnorm_exp: 307.6526555685887,
            inv256: 850.4135922911647,
            u: -0.0011758984204561784,
            l: -1.0624811566412999e-13,
            kernel: [2.3025850929940255, 2.6509490552391974, 2.034678825384765, 1.1712552025835192],
        },
    }
}

fn exp_impl(x: f64, base: Base) -> f64 {
    let c = exp_consts(base);
    let mut n_float = x.mul_add(c.inv256, MAGIC_ROUND);
    let n = n_float.to_bits() as u32 as i32;
    n_float -= MAGIC_ROUND;
    let mut r = n_float.mul_add(c.u, x);
    r = n_float.mul_add(c.l, r);
    let k = n >> 8;
    let j = J_TABLE[(n & 255) as usize];
    let ju = f64::from_bits(0x3FF0000000000000 | (j & (u64::MAX >> 12)));
    let jl = f64::from_bits(0x3C00000000000000 | (j >> 8));
    let [p0, p1, p2, p3] = c.kernel;
    let kern = r * r.mul_add(r.mul_add(r.mul_add(p3, p2), p1), p0);
    let small_part = ju.mul_add(kern, jl) + ju;
    if !(x.abs() <= c.subnorm_exp) {
        if x.is_nan() {
            return x;
        }
        if x >= c.max_exp {
            return f64::INFINITY;
        }
        if x <= c.min_exp {
            return 0.0;
        }
        if k <= -53 {
            let twopk = ((k as i64 + 53) as u64) << 52;
            return f64::from_bits(twopk.wrapping_add(small_part.to_bits())) * f64::from_bits(0x3CA0000000000000);
        }
    }
    let twopk = (k as i64) << 52;
    f64::from_bits(twopk.wrapping_add(small_part.to_bits() as i64) as u64)
}

/// Julia's `exp10(::Float64)`.
pub(crate) fn exp10(x: f64) -> f64 {
    exp_impl(x, Base::Ten)
}
/// Julia's `exp2(::Float64)`.
pub(crate) fn exp2(x: f64) -> f64 {
    exp_impl(x, Base::Two)
}
/// Julia's `exp(::Float64)`.
pub(crate) fn exp(x: f64) -> f64 {
    exp_impl(x, Base::E)
}

// ---------------------------------------------------------------------------------------------
// Rounding and spacing

/// Julia's `eps(x)`: the spacing of floats at `x`.
pub(crate) fn eps(x: f64) -> f64 {
    if !x.is_finite() {
        return f64::NAN;
    }
    let ax = x.abs();
    if ax >= f64::MIN_POSITIVE {
        // ldexp(eps(Float64), exponent(x))
        let e = ((ax.to_bits() >> 52) & 0x7ff) as i32 - 1023;
        ldexp(f64::EPSILON, e)
    } else {
        f64::from_bits(1)
    }
}

fn ldexp(x: f64, e: i32) -> f64 {
    // Exact scaling by 2^e in (at most) three exact steps.
    let mut x = x;
    let mut e = e;
    while e > 1023 {
        x *= f64::from_bits(0x7FE0000000000000); // 2^1023
        e -= 1023;
    }
    while e < -1022 {
        x *= f64::from_bits(0x0010000000000000); // 2^-1022
        e += 1022;
    }
    x * f64::from_bits(((e + 1023) as u64) << 52)
}

/// Julia's `isapprox(x, y; rtol)` with `atol = 0`.
pub(crate) fn isapprox(x: f64, y: f64, rtol: f64) -> bool {
    x == y || (x.is_finite() && y.is_finite() && (x - y).abs() <= rtol * x.abs().max(y.abs()))
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Nearest,
    Down,
}

fn round_mode(x: f64, mode: Mode) -> f64 {
    match mode {
        Mode::Nearest => x.round_ties_even(),
        Mode::Down => x.floor(),
    }
}

/// Julia's `_round_digits(x, mode, digits, 10)`.
pub(crate) fn round_digits(x: f64, mode: Mode, digits: i64) -> f64 {
    if digits >= 0 {
        let invstep = powi(10.0, digits);
        if invstep.is_finite() {
            let y = round_mode(x * invstep, mode) / invstep;
            if y.is_finite() { y } else { x }
        } else {
            let s = powf(10.0, digits as f64 / 2.0);
            let y = round_mode((x * s) * s, mode) / s / s;
            if y.is_finite() { y } else { x }
        }
    } else {
        let step = powi(10.0, -digits);
        let y = round_mode(x / step, mode) * step;
        if y.is_finite() {
            y
        } else if x > 0.0 {
            0.0
        } else if x < 0.0 {
            match mode {
                Mode::Down => f64::NEG_INFINITY,
                Mode::Nearest => -0.0,
            }
        } else {
            x
        }
    }
}

/// Julia's `round(x; sigdigits = n)` (base 10, ties to even).
pub(crate) fn round_sigdigits(x: f64, n: i64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let h = if x == 0.0 { 0 } else { 1 + log10(x.abs()).floor() as i64 };
    round_digits(x, Mode::Nearest, n - h)
}

// ---------------------------------------------------------------------------------------------
// Float ranges (TwicePrecision)

/// A `(hi, lo)` double-double as in Julia's `TwicePrecision{Float64}`.
#[derive(Clone, Copy, Debug)]
struct Tp {
    hi: f64,
    lo: f64,
}

#[inline]
fn canonicalize2(big: f64, little: f64) -> (f64, f64) {
    let h = big + little;
    (h, (big - h) + little)
}

#[inline]
fn add12(x: f64, y: f64) -> (f64, f64) {
    let (x, y) = if y.abs() > x.abs() { (y, x) } else { (x, y) };
    canonicalize2(x, y)
}

fn mul12(x: f64, y: f64) -> (f64, f64) {
    let (h, l) = two_mul(x, y);
    if h.is_finite() { (h, l) } else { (h, h) }
}

fn truncbits(x: f64, nb: u32) -> f64 {
    if nb >= 64 {
        return f64::from_bits(0);
    }
    f64::from_bits(x.to_bits() & (u64::MAX << nb))
}

impl Tp {
    fn from_int(i: i128) -> Tp {
        // splitprec(Float64, i) then canonicalize2
        let hi = truncbits(i as f64, 27);
        let ihi = hi as i128;
        let lo = (i.wrapping_sub(ihi)) as f64;
        let (h, l) = canonicalize2(hi, lo);
        Tp { hi: h, lo: l }
    }
    fn div(self, y: Tp) -> Tp {
        let hi = self.hi / y.hi;
        let (uh, ul) = mul12(hi, y.hi);
        let lo = ((((self.hi - uh) - ul) + self.lo) - hi * y.lo) / y.hi;
        if hi == 0.0 || !hi.is_finite() {
            return Tp { hi, lo: hi };
        }
        let (h, l) = canonicalize2(hi, lo);
        Tp { hi: h, lo: l }
    }
    /// `TwicePrecision{Float64}((n, d))`.
    fn ratio(n: i128, d: i128) -> Tp {
        Tp::from_int(n).div(Tp { hi: d as f64, lo: 0.0 })
    }
    /// `twiceprecision(val, nb)`.
    fn trunc(self, nb: u32) -> Tp {
        let hi = truncbits(self.hi, nb);
        Tp { hi, lo: (self.hi - hi) + self.lo }
    }
}

/// A `StepRangeLen{Float64, TwicePrecision, TwicePrecision}`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FloatRange {
    rref: Tp,
    step: Tp,
    len: i64,
    offset: i64,
}

impl FloatRange {
    pub(crate) fn len(&self) -> usize {
        self.len.max(0) as usize
    }
    /// `r[i]` for 1-based `i` (Julia's `unsafe_getindex`).
    pub(crate) fn get(&self, i: i64) -> f64 {
        let u = (i - self.offset) as f64;
        let shift_hi = u * self.step.hi;
        let shift_lo = u * self.step.lo;
        let (x_hi, x_lo) = add12(self.rref.hi, shift_hi);
        x_hi + (x_lo + (shift_lo + self.rref.lo))
    }
    pub(crate) fn collect(&self) -> Vec<f64> {
        (1..=self.len).map(|i| self.get(i)).collect()
    }
}

fn top_set_bit(x: i64) -> u32 {
    64 - (x as u64).leading_zeros()
}

fn nbitslen(len: i64, offset: i64) -> u32 {
    let n = if len < 2 { 0 } else { top_set_bit((offset - 1).max(len - offset) - 1) + 1 };
    n.min(27)
}

/// `steprangelen_hp(Float64, (ref_n, den), (step_n, den), nb, len, offset)`.
fn hp_ratio(ref_n: i128, ref_d: i128, step_n: i128, step_d: i128, nb: u32, len: i64, offset: i64) -> FloatRange {
    FloatRange { rref: Tp::ratio(ref_n, ref_d), step: Tp::ratio(step_n, step_d).trunc(nb), len, offset }
}

/// `steprangelen_hp(Float64, ref, step, nb, len, offset)` for float `ref`, `step`.
fn hp_float(rref: Tp, step: Tp, nb: u32, len: i64, offset: i64) -> FloatRange {
    FloatRange { rref, step: step.trunc(nb), len, offset }
}

/// Julia's `rat(x)`: a best rational approximation with `Int` numerator and denominator.
fn rat(x: f64) -> (i64, i64) {
    let mut y = x;
    let (mut a, mut d) = (1i64, 1i64);
    let (mut b, mut c) = (0i64, 0i64);
    let m = 16777216.0f64; // maxintfloat(Float32, Int)
    while y.abs() <= m {
        let f = y.trunc() as i64;
        y -= f as f64;
        let (na, nc) = (f.wrapping_mul(a).wrapping_add(c), a);
        a = na;
        c = nc;
        let (nb, nd) = (f.wrapping_mul(b).wrapping_add(d), b);
        b = nb;
        d = nd;
        if a.unsigned_abs().max(b.unsigned_abs()) > m as u64 {
            return (c, d);
        }
        if a as f64 / b as f64 == x {
            break;
        }
        y = 1.0 / y;
    }
    (a, b)
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.unsigned_abs(), b.unsigned_abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a as i64
}

fn lcm_unchecked(a: i64, b: i64) -> Option<i64> {
    let g = gcd(a, b);
    if g == 0 {
        return None;
    }
    Some(a.wrapping_mul(b.wrapping_div(g)))
}

fn isbetween(a: f64, x: f64, b: f64) -> bool {
    (a <= x && x <= b) || (b <= x && x <= a)
}

const MAXINTFLOAT: f64 = 9007199254740992.0;

/// `floatrange(Float64, start_n, step_n, len, den)`.
fn floatrange(start_n: i64, step_n: i64, len: i64, den: i64) -> FloatRange {
    if len < 2 || step_n == 0 {
        return hp_ratio(start_n as i128, den as i128, step_n as i128, den as i128, 0, len, 1);
    }
    let imin = ((-(start_n as f64) / step_n as f64 + 1.0).round_ties_even() as i64).clamp(1, len);
    let ref_n = start_n.wrapping_add((imin - 1).wrapping_mul(step_n));
    let nb = nbitslen(len, imin);
    hp_ratio(ref_n as i128, den as i128, step_n as i128, den as i128, nb, len, imin)
}

/// Julia's `start:step:stop` for `Float64`. `None` for a zero step (Julia throws).
pub(crate) fn colon(start: f64, step: f64, stop: f64) -> Option<FloatRange> {
    if step == 0.0 {
        return None;
    }
    let (step_n, step_d) = rat(step);
    if step_d != 0 && step_n as f64 / step_d as f64 == step {
        let (start_n, start_d) = rat(start);
        let (stop_n, stop_d) = rat(stop);
        if start_d != 0
            && stop_d != 0
            && start_n as f64 / start_d as f64 == start
            && stop_n as f64 / stop_d as f64 == stop
            && let Some(den) = lcm_unchecked(start_d, step_d)
        {
            let m = MAXINTFLOAT;
            if den != 0
                && (start * den as f64).abs() <= m
                && (step * den as f64).abs() <= m
                && den.wrapping_rem(start_d) == 0
                && den.wrapping_rem(step_d) == 0
            {
                let start_n = (start * den as f64).round_ties_even() as i64;
                let step_n = (step * den as f64).round_ties_even() as i64;
                let num = den
                    .wrapping_mul(stop_n)
                    .wrapping_sub(stop_d.wrapping_mul(start_n))
                    .wrapping_add(step_n.wrapping_mul(stop_d));
                let dd = step_n.wrapping_mul(stop_d);
                if dd != 0 && !(num == i64::MIN && dd == -1) {
                    let len = (num / dd).max(0);
                    if isbetween(start, start + (len - 1) as f64 * step, stop + step / 2.0)
                        && !isbetween(start, start + len as f64 * step, stop)
                    {
                        return Some(floatrange(start_n, step_n, len, den));
                    }
                }
            }
        }
    }
    let lf = (stop - start) / step;
    let len = if lf < 0.0 {
        0
    } else if lf == 0.0 {
        1
    } else if !(lf < 1e9) {
        // NaN, or a range too long to materialize (Julia would try).
        return None;
    } else {
        let mut len = lf.round_ties_even() as i64 + 1;
        let stop2 = start + (len - 1) as f64 * step;
        len -= ((start < stop && stop < stop2) as i64) + ((start > stop && stop > stop2) as i64);
        len
    };
    Some(hp_float(Tp { hi: start, lo: 0.0 }, Tp { hi: step, lo: 0.0 }, 0, len, 1))
}

/// Julia's `range(start, stop; length = len)` for `Float64`.
pub(crate) fn linspace(start: f64, stop: f64, len: i64) -> Option<FloatRange> {
    if len < 2 {
        // `_linspace1`
        if len < 0 || (len == 1 && start != stop) {
            return None;
        }
        return Some(FloatRange { rref: Tp { hi: start, lo: 0.0 }, step: Tp { hi: start, lo: -stop }, len, offset: 1 });
    }
    if start == stop {
        return Some(hp_float(Tp { hi: start, lo: 0.0 }, Tp { hi: 0.0, lo: 0.0 }, 0, len, 1));
    }
    let (_, start_d) = rat(start);
    let (_, stop_d) = rat(stop);
    if start_d != 0
        && stop_d != 0
        && let Some(den) = lcm_unchecked(start_d, stop_d)
    {
        let m = MAXINTFLOAT;
        if den != 0 && (den as f64 * start).abs() <= m && (den as f64 * stop).abs() <= m {
            let start_n = (den as f64 * start).round_ties_even() as i64;
            let stop_n = (den as f64 * stop).round_ties_even() as i64;
            if start_n as f64 / den as f64 == start && stop_n as f64 / den as f64 == stop {
                return Some(linspace_rat(start_n, stop_n, len, den));
            }
        }
    }
    linspace_float(start, stop, len)
}

fn linspace_rat(start_n: i64, stop_n: i64, len: i64, den: i64) -> FloatRange {
    if start_n == stop_n {
        return hp_ratio(start_n as i128, den as i128, 0, den as i128, 0, len, 1);
    }
    let tmin = -(start_n as f64) / (stop_n as f64 - start_n as f64);
    let imin = ((tmin * (len - 1) as f64 + 1.0).round_ties_even() as i64).clamp(1, len);
    let (start_n, stop_n) = (start_n as i128, stop_n as i128);
    let ref_num = (len - imin) as i128 * start_n + (imin - 1) as i128 * stop_n;
    let ref_denom = (len - 1) as i128 * den as i128;
    hp_ratio(ref_num, ref_denom, stop_n - start_n, ref_denom, nbitslen(len, imin), len, imin)
}

fn linspace_float(start: f64, stop: f64, len: i64) -> Option<FloatRange> {
    if !(start.is_finite() && stop.is_finite()) {
        return None;
    }
    let (mut delta, mut dfac) = (stop - start, 1.0f64);
    if !delta.is_finite() {
        delta = stop / len as f64 - start / len as f64;
        dfac = len as f64;
    }
    let tmin = -(start / delta) / dfac;
    let lenn1 = len - 1;
    let mut imin = (tmin * lenn1 as f64 + 1.0).round_ties_even() as i64;
    let (rref, step);
    if 1 < imin && imin < len {
        let t = (imin - 1) as f64 / lenn1 as f64;
        rref = (1.0 - t) * start + t * stop;
        step = if imin - 1 < len - imin {
            (rref - start) / (imin - 1) as f64
        } else {
            (stop - rref) / (len - imin) as f64
        };
    } else if imin <= 1 {
        imin = 1;
        rref = start;
        step = (delta / lenn1 as f64) * dfac;
    } else {
        imin = len;
        rref = stop;
        step = (delta / lenn1 as f64) * dfac;
    }
    if len == 2 && !step.is_finite() {
        return Some(hp_float(Tp { hi: start, lo: 0.0 }, Tp { hi: -start, lo: stop }, 0, len, 1));
    }
    let m = f64::MAX.next_down();
    let k = ((imin - 1).max(len - imin)) as f64;
    let (lo, hi) = ((-(m + rref) / k).max((-m + rref) / k), ((m - rref) / k).min((m + rref) / k));
    // Julia's clamp (no panic on lo > hi or NaN).
    let step_hi_pre = if step > hi {
        hi
    } else if step < lo {
        lo
    } else {
        step
    };
    let nb = nbitslen(len, imin);
    let step_hi = truncbits(step_hi_pre, nb);
    let (x1_hi, x1_lo) = add12((1 - imin) as f64 * step_hi, rref);
    let (x2_hi, x2_lo) = add12((len - imin) as f64 * step_hi, rref);
    let a = (start - x1_hi) - x1_lo;
    let b = (stop - x2_hi) - x2_lo;
    let step_lo = (b - a) / (len - 1) as f64;
    let ref_lo = a - (1 - imin) as f64 * step_lo;
    Some(hp_float(Tp { hi: rref, lo: ref_lo }, Tp { hi: step_hi, lo: step_lo }, 0, len, imin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn julia_quirks() {
        assert_eq!(powi(10.0, -2), 0.010000000000000002);
        assert_eq!(powi(10.0, 3), 1000.0);
        assert_eq!(log10(1000.0), 3.0);
        assert_eq!(exp10(2.0), 100.0);
        let r = colon(0.1, 0.1, 0.3).unwrap().collect();
        assert_eq!(r, vec![0.1, 0.2, 0.3]);
        assert_eq!(linspace(0.0, 1.0, 3).unwrap().collect(), vec![0.0, 0.5, 1.0]);
    }
}
