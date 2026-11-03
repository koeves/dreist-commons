//! Layer velocity splits ([re:30df67]) and the pad
//! "auto split" / "auto overlap" actions (`e_autoVelocitySplitPad` 0x57,
//! `e_autoVelocityCross` 0x58, [re:5723c8]).
//!
//! A split is `[lowest, low turnover, high turnover, highest]`,
//! every setter clamped to 127. Only lowest..=highest gates a layer
//! ([re:39b7ef]); the turnovers are drawn by the
//! editor and saved, nothing else reads them.

/// `[lowest, low turnover, high turnover, highest]`.
pub type Split = [u32; 4];

/// [re:30df67] default (also the default-state check).
pub const DEFAULT: Split = [0, 0, 127, 127];

/// Child node of a layer ([re:77e04b]).
pub const NODE_CLASS: &str = "LayerVelocitySplit";
pub const NODE_NAME: &str = "VelocitySplit";
pub const ATTRS: [&str; 4] = ["LowPoint", "LowTurn", "HighTurn", "Highest"];

/// [re:bc5e17].
fn init(a: u32, b: u32, c: u32, d: u32) -> Split {
    [a.min(127), b.min(127), c.min(127), d.min(127)]
}

/// [re:1f9bc3]: the layers with audio share 0..127 in
/// equal `ceil(127 / n)` bands, the last one reaching 127.
pub fn auto_split(valid: &[bool], splits: &mut [Split]) {
    let n = valid.iter().filter(|v| **v).count() as u32;
    if n == 0 {
        return;
    }
    let step = (127.0f32 / n as f32).ceil() as u32;
    let mut k = 0u32;
    for (s, _) in splits.iter_mut().zip(valid).filter(|(_, v)| **v) {
        let lo = k * step;
        let hi = lo + step - 1;
        *s = init(lo, lo, hi, hi);
        k += 1;
        if k >= n {
            *s = init(lo, lo, 127, 127);
        }
    }
}

/// [re:62fb54]: auto split, then every band but the
/// first starts `floor(ceil(127 / n) * amount)` lower and every band but the
/// last ends that much higher (turnovers stay, so the overlap ramps).
pub fn auto_cross(valid: &[bool], splits: &mut [Split], amount: f32) {
    let n = valid.iter().filter(|v| **v).count() as u32;
    if n == 0 {
        return;
    }
    auto_split(valid, splits);
    let step = (127.0f32 / n as f32).ceil();
    let ov = (step as i32 as f32 * amount).floor() as i32 as u32;
    for (k, (s, _)) in (0u32..).zip(splits.iter_mut().zip(valid).filter(|(_, v)| **v)) {
        if k != 0 {
            s[0] = s[0].wrapping_sub(ov).min(127);
        }
        if k == 0 || k + 1 < n {
            s[3] = s[3].wrapping_add(ov).min(127);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_split_bands() {
        let valid = [true, false, true, true, false, false, false, false];
        let mut s = [DEFAULT; 8];
        auto_split(&valid, &mut s);
        assert_eq!(s[0], [0, 0, 42, 42]);
        assert_eq!(s[1], DEFAULT);
        assert_eq!(s[2], [43, 43, 85, 85]);
        assert_eq!(s[3], [86, 86, 127, 127]);
        let mut one = [[5, 6, 7, 8]; 2];
        auto_split(&[false, true], &mut one);
        assert_eq!(one[1], [0, 0, 127, 127]);
    }

    #[test]
    fn auto_cross_overlaps() {
        let valid = [true, true, true, false];
        let mut s = [DEFAULT; 4];
        auto_cross(&valid, &mut s, 0.1);
        // step 43, overlap 4.
        assert_eq!(s[0], [0, 0, 42, 46]);
        assert_eq!(s[1], [39, 43, 85, 89]);
        assert_eq!(s[2], [82, 86, 127, 127]);
    }
}
