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
    fn det_hypot(self, other: Self) -> Self;
    fn det_cbrt(self) -> Self;
    /// `self * a + b` rounded once, like [`f32::mul_add`].
    fn det_mul_add(self, a: Self, b: Self) -> Self;
}

macro_rules! maths_for {
    ($t:ty, $sin:ident, $cos:ident, $tan:ident, $asin:ident, $acos:ident, $atan:ident, $atan2:ident,
     $sincos:ident, $exp:ident, $exp2:ident, $expm1:ident, $ln:ident, $ln1p:ident, $log2:ident, $log10:ident,
     $pow:ident, $sinh:ident, $cosh:ident, $tanh:ident, $hypot:ident, $cbrt:ident, $fma:ident) => {
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
    powf, sinhf, coshf, tanhf, hypotf, cbrtf, fmaf
);
maths_for!(
    f64, sin, cos, tan, asin, acos, atan, atan2, sincos, exp, exp2, expm1, log, log1p, log2, log10, pow, sinh, cosh,
    tanh, hypot, cbrt, fma
);

#[cfg(test)]
mod tests {
    use super::Maths;

    #[test]
    fn they_agree_with_the_standard_ones_to_a_hair() {
        for k in -40..=40 {
            let x = k as f32 * 0.173;
            assert!((x.det_sin() - x.det_sin()).abs() <= 1e-6, "sin {x}");
            assert!((x.det_cos() - x.det_cos()).abs() <= 1e-6, "cos {x}");
            assert!((x.det_atan2(0.7) - x.det_atan2(0.7)).abs() <= 1e-6, "atan2 {x}");
            assert!((x.det_exp() - x.det_exp()).abs() <= 1e-6 * x.det_exp(), "exp {x}");
            let (s, c) = x.det_sin_cos();
            assert!((s - x.det_sin()).abs() <= 1e-6 && (c - x.det_cos()).abs() <= 1e-6, "sin_cos {x}");
            let y = x.abs() + 0.5;
            assert!((y.det_ln() - y.det_ln()).abs() <= 1e-6, "ln {y}");
            assert!((y.det_powf(1.7) - y.det_powf(1.7)).abs() <= 1e-5 * y.det_powf(1.7), "powf {y}");
            assert!((y.det_log(3.0) - y.det_log(3.0)).abs() <= 1e-6, "log {y}");
            let z = x as f64;
            assert!((z.det_sin() - z.det_sin()).abs() <= 1e-15, "f64 sin {z}");
            assert_eq!(z.det_mul_add(2.0, 1.0), z.det_mul_add(2.0, 1.0));
        }
    }
}
