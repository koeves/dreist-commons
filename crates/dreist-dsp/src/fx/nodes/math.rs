//! Small helpers the [re:f2c879] voices share (each inlined in the original's
//! nodes; these are the same IEEE operations).

/// `((|x| + 1) − |x − 1|) / 2`: `x` clamped to 0..1 the [re:f2c879] way.
#[inline]
pub(crate) fn clip01(x: f32) -> f32 {
    ((x.abs() + 1.0) - (x - 1.0).abs()) * 0.5
}

/// Soft saturator: clamp ±2 then `((|c|/16 − 1/4)c² + 1)c`.
#[inline]
pub(crate) fn sat(x: f32) -> f32 {
    let c = ((x + 2.0).abs() - (x - 2.0).abs()) * 0.5;
    ((c.abs() * 0.0625 - 0.25) * c * c + 1.0) * c
}

/// 4-point cubic read of a power-of-two ring at fractional position `pos`
/// (asm-order arithmetic).
#[inline]
pub(crate) fn cubic(buf: &[f32], mask: u32, pos: f32) -> f32 {
    let i = pos.floor() as i32 as u32;
    let t = pos - (i as i32) as f32;
    let a = buf[(i.wrapping_sub(1) & mask) as usize];
    let b = buf[(i & mask) as usize];
    let c = buf[(i.wrapping_add(1) & mask) as usize];
    let d = buf[(i.wrapping_add(2) & mask) as usize];
    ((((b - c) * 0.5 + (d - a) * 0.16666667) * t + ((a + c) * 0.5 - b)) * t + (((c - a * 0.33333334) - b * 0.5) - d * 0.16666667)) * t + b
}

/// Ring sizes (`SetMaxDelay`): the smallest power of two ≥ `n` (1 below 2).
pub(crate) fn pow2_at_least(n: i64) -> usize {
    if n > 1 { (n as usize).next_power_of_two() } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pow2_matches_the_shift_loop() {
        for n in 0..5000i64 {
            let mut size = 1usize;
            if n > 1 {
                let mut k = (n - 1) as u32;
                while k != 0 {
                    size *= 2;
                    k >>= 1;
                }
            }
            assert_eq!(pow2_at_least(n), size, "{n}");
        }
    }
}
