//! Sample interpolation from [re:32ee83].

/// 4-point Catmull-Rom: `x0..x2` are the frames at the integer position and the
/// next two (wrapping at the end marker), `f` the fractional part.
///
/// `prev` is *not* `x[i-1]`: the original keeps the frame at the integer
/// position of the previous output sample and uses that as the left tap. At
/// unity pitch the two coincide; at other rates the result differs.
pub fn catmull_rom(prev: f32, x0: f32, x1: f32, x2: f32, f: f32) -> f32 {
    let d = x0 - x1;
    let a = (x1 - prev) * 0.5;
    let b = a + d;
    let c = (x2 - x0) * 0.5 + d + b;
    ((c * f + (-b - c)) * f + a) * f + x0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_samples() {
        assert_eq!(catmull_rom(0.0, 1.0, 2.0, 3.0, 0.0), 1.0);
        assert!((catmull_rom(1.0, 2.0, 3.0, 4.0, 0.5) - 2.5).abs() < 1e-6);
    }
}
