//! The memory readout's text ([re:04a6c7]): bytes up to 1 KB, then KB,
//! MB and GB with two, one or no decimals as the value grows past 10 and
//! 100 of the unit (1 KB = 1024 bytes).

/// `bytes` as the editors show it ("19.7MB").
pub fn bytes_label(bytes: u64) -> String {
    const K: u64 = 1024;
    const M: u64 = K * K;
    let (v, unit, dec) = match bytes {
        0..=K => return format!("{bytes}B"),
        b if b <= 10 * K => (b as f64 / K as f64, "KB", 2),
        b if b <= 100 * K => (b as f64 / K as f64, "KB", 1),
        b if b <= M => (b as f64 / K as f64, "KB", 0),
        b if b <= 10 * M => (b as f64 / M as f64, "MB", 2),
        b if b <= 100 * M => (b as f64 / M as f64, "MB", 1),
        b if b <= 1024 * M => (b as f64 / M as f64, "MB", 0),
        b => (b as f64 / (1024 * M) as f64, "GB", 2),
    };
    format!("{v:.dec$}{unit}")
}

#[cfg(test)]
mod tests {
    use super::bytes_label;

    #[test]
    fn units_and_decimals() {
        assert_eq!(bytes_label(0), "0B");
        assert_eq!(bytes_label(1024), "1024B");
        assert_eq!(bytes_label(1025), "1.00KB");
        assert_eq!(bytes_label(10 * 1024), "10.00KB");
        assert_eq!(bytes_label(50 * 1024), "50.0KB");
        assert_eq!(bytes_label(1024 * 1024), "1024KB");
        assert_eq!(bytes_label(5 * 1024 * 1024), "5.00MB");
        assert_eq!(bytes_label(20_656_947), "19.7MB");
        assert_eq!(bytes_label(500 * 1024 * 1024), "500MB");
        assert_eq!(bytes_label(3 * 1024 * 1024 * 1024), "3.00GB");
    }

    #[test]
    fn sample_size_counts_both_channels() {
        let d = dreist_dsp::voice::SampleData { left: vec![0.0; 100], right: Some(vec![0.0; 100]), sample_rate: 44_100.0 };
        assert_eq!(d.bytes(), 800);
    }
}
