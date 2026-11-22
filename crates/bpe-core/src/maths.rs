//! Maths that gives the same answer on every computer.
//!
//! `f32::sin`, `exp`, `powf` and the rest call the platform's maths library, and Windows' rounds a
//! hair differently from Linux's. The physics is chaotic, so one last-bit difference grows until a
//! scene plays out differently on the owner's PC than in the checks (a built-in scene's big moment
//! landed 4 s late on Windows). Everything that steps the physics, or builds what the physics reads,
//! uses these instead: they are the `libm` crate's, written in Rust, so every platform runs the same
//! code. The physics crates' `clippy.toml` forbids the platform ones.
//!
//! `sqrt`, `abs`, `floor`, `round`, `%` and `powi` are left as they are: the first ones are exact by
//! the floating-point standard, and `powi` is the compiler's own, the same everywhere.

/// [`f32`] and [`f64`] maths that rounds the same on every platform (`use bpe_core::maths::Maths`).
/// Each method is the standard one of the same name with `det_` in front.
pub trait Maths: Copy {
    fn det_sin(self) -> Self;
    fn det_cos(self) -> Self;
    fn det_tan(self) -> Self;
    fn det_asin(self) -> Self;
    fn det_acos(self) -> Self;
    fn det_atan(self) -> Self;
    /// The angle of the point (`other`, `self`), like [`f32::atan2`].
    fn det_atan2(self, other: Self) -> Self;
    fn det_sin_cos(self) -> (Self, Self);
    fn det_exp(self) -> Self;
    fn det_exp2(self) -> Self;
    fn det_exp_m1(self) -> Self;
    fn det_ln(self) -> Self;
    fn det_ln_1p(self) -> Self;
    /// The logarithm to `base`, worked out as std does (`ln(self) / ln(base)`).
    fn det_log(self, base: Self) -> Self;
    fn det_log2(self) -> Self;
    fn det_log10(self) -> Self;
    fn det_powf(self, n: Self) -> Self;
    fn det_sinh(self) -> Self;
    fn det_cosh(self) -> Self;
    fn det_tanh(self) -> Self;
    fn det_asinh(self) -> Self;
    fn det_acosh(self) -> Self;
    fn det_atanh(self) -> Self;
    fn det_hypot(self, other: Self) -> Self;
    fn det_cbrt(self) -> Self;
    /// `self * a + b` rounded once, like [`f32::mul_add`].
    fn det_mul_add(self, a: Self, b: Self) -> Self;
}

macro_rules! maths_for {
    ($t:ty, $sin:ident, $cos:ident, $tan:ident, $asin:ident, $acos:ident, $atan:ident, $atan2:ident,
     $sincos:ident, $exp:ident, $exp2:ident, $expm1:ident, $ln:ident, $ln1p:ident, $log2:ident, $log10:ident,
     $pow:ident, $sinh:ident, $cosh:ident, $tanh:ident, $asinh:ident, $acosh:ident, $atanh:ident, $hypot:ident, $cbrt:ident, $fma:ident) => {
        impl Maths for $t {
            #[inline]
            fn det_sin(self) -> Self {
                libm::$sin(self)
            }
            #[inline]
            fn det_cos(self) -> Self {
                libm::$cos(self)
            }
            #[inline]
            fn det_tan(self) -> Self {
                libm::$tan(self)
            }
            #[inline]
            fn det_asin(self) -> Self {
                libm::$asin(self)
            }
            #[inline]
            fn det_acos(self) -> Self {
                libm::$acos(self)
            }
            #[inline]
            fn det_atan(self) -> Self {
                libm::$atan(self)
            }
            #[inline]
            fn det_atan2(self, other: Self) -> Self {
                libm::$atan2(self, other)
            }
            #[inline]
            fn det_sin_cos(self) -> (Self, Self) {
                libm::$sincos(self)
            }
            #[inline]
            fn det_exp(self) -> Self {
                libm::$exp(self)
            }
            #[inline]
            fn det_exp2(self) -> Self {
                libm::$exp2(self)
            }
            #[inline]
            fn det_exp_m1(self) -> Self {
                libm::$expm1(self)
            }
            #[inline]
            fn det_ln(self) -> Self {
                libm::$ln(self)
            }
            #[inline]
            fn det_ln_1p(self) -> Self {
                libm::$ln1p(self)
            }
            #[inline]
            fn det_log(self, base: Self) -> Self {
                libm::$ln(self) / libm::$ln(base)
            }
            #[inline]
            fn det_log2(self) -> Self {
                libm::$log2(self)
            }
            #[inline]
            fn det_log10(self) -> Self {
                libm::$log10(self)
            }
            #[inline]
            fn det_powf(self, n: Self) -> Self {
                libm::$pow(self, n)
            }
            #[inline]
            fn det_sinh(self) -> Self {
                libm::$sinh(self)
            }
            #[inline]
            fn det_cosh(self) -> Self {
                libm::$cosh(self)
            }
            #[inline]
            fn det_tanh(self) -> Self {
                libm::$tanh(self)
            }
            #[inline]
            fn det_asinh(self) -> Self {
                libm::$asinh(self)
            }
            #[inline]
            fn det_acosh(self) -> Self {
                libm::$acosh(self)
            }
            #[inline]
            fn det_atanh(self) -> Self {
                libm::$atanh(self)
            }
            #[inline]
            fn det_hypot(self, other: Self) -> Self {
                libm::$hypot(self, other)
            }
            #[inline]
            fn det_cbrt(self) -> Self {
                libm::$cbrt(self)
            }
            #[inline]
            fn det_mul_add(self, a: Self, b: Self) -> Self {
                libm::$fma(self, a, b)
            }
        }
    };
}

maths_for!(
    f32, sinf, cosf, tanf, asinf, acosf, atanf, atan2f, sincosf, expf, exp2f, expm1f, logf, log1pf, log2f, log10f,
    powf, sinhf, coshf, tanhf, asinhf, acoshf, atanhf, hypotf, cbrtf, fmaf
);
maths_for!(
    f64, sin, cos, tan, asin, acos, atan, atan2, sincos, exp, exp2, expm1, log, log1p, log2, log10, pow, sinh, cosh,
    tanh, asinh, acosh, atanh, hypot, cbrt, fma
);

#[cfg(test)]
mod tests {
    use super::Maths;

    /// Each one against the platform's own: a wrong mapping (a swapped argument, the f64 one on an f32) shows.
    #[test]
    #[allow(clippy::disallowed_methods)]
    fn they_agree_with_the_standard_ones_to_a_hair() {
        let close = |a: f32, b: f32, what: &str| assert!((a - b).abs() <= 1e-6 * b.abs().max(1.0), "{what}: {a} {b}");
        for k in -40..=40 {
            let x = k as f32 * 0.173;
            close(x.det_sin(), x.sin(), "sin");
            close(x.det_cos(), x.cos(), "cos");
            close((x * 0.3).det_tan(), (x * 0.3).tan(), "tan");
            close(x.det_atan(), x.atan(), "atan");
            close(x.det_atan2(0.7), x.atan2(0.7), "atan2");
            close(0.7f32.det_atan2(x), 0.7f32.atan2(x), "atan2 the other way");
            let (s, c) = x.det_sin_cos();
            close(s, x.sin(), "sin_cos sin");
            close(c, x.cos(), "sin_cos cos");
            close(x.det_exp(), x.exp(), "exp");
            close(x.det_exp2(), x.exp2(), "exp2");
            close(x.det_exp_m1(), x.exp_m1(), "exp_m1");
            close(x.det_sinh(), x.sinh(), "sinh");
            close(x.det_cosh(), x.cosh(), "cosh");
            close(x.det_tanh(), x.tanh(), "tanh");
            close(x.det_asinh(), x.asinh(), "asinh");
            close(x.det_cbrt(), x.cbrt(), "cbrt");
            close(x.det_hypot(1.3), x.hypot(1.3), "hypot");
            close(x.det_mul_add(1.7, -0.4), x.mul_add(1.7, -0.4), "mul_add");
            // Inside -0.7..0.7: near ±1 atanh is so steep that std's own (built on ln_1p) parts from the true value.
            let u = x / 10.0;
            close(u.det_asin(), u.asin(), "asin");
            close(u.det_acos(), u.acos(), "acos");
            close(u.det_atanh(), u.atanh(), "atanh");
            let y = x.abs() + 0.5;
            close(y.det_ln(), y.ln(), "ln");
            close(y.det_ln_1p(), y.ln_1p(), "ln_1p");
            close(y.det_log(3.0), y.log(3.0), "log");
            close(y.det_log2(), y.log2(), "log2");
            close(y.det_log10(), y.log10(), "log10");
            close(y.det_powf(1.7), y.powf(1.7), "powf");
            close((y + 1.0).det_acosh(), (y + 1.0).acosh(), "acosh");
            let z = x as f64;
            assert!((z.det_sin() - z.sin()).abs() <= 1e-15, "f64 sin {z}");
            assert!((z.det_atan2(0.7) - z.atan2(0.7)).abs() <= 1e-15, "f64 atan2 {z}");
            assert!((z.det_exp() - z.exp()).abs() <= 1e-15 * z.exp().max(1.0), "f64 exp {z}");
            assert_eq!(z.det_mul_add(2.0, 1.0), z.mul_add(2.0, 1.0));
        }
    }
}
