use chrono::NaiveDate;

/// Resolve a date string to a NaiveDate, defaulting to today.
pub fn resolve_date(date: Option<&str>) -> anyhow::Result<NaiveDate> {
    match date {
        Some(s) => Ok(NaiveDate::parse_from_str(s, "%Y-%m-%d")?),
        None => Ok(chrono::Local::now().date_naive()),
    }
}

/// Compute peak amplitude in dB (relative to full scale).
/// Returns -infinity for all-zero input.
pub fn peak_db(samples: &[f32]) -> f32 {
    let peak = samples
        .iter()
        .fold(0.0_f32, |max, &s| max.max(s.abs()));

    if peak == 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * peak.log10()
    }
}

/// Compute RMS level in dB (relative to full scale).
/// Returns -infinity for all-zero input.
pub fn rms_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return f32::NEG_INFINITY;
    }

    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();

    if rms == 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * rms.log10()
    }
}

/// Compute simple linear regression: y = slope * x + intercept.
/// Returns (slope, intercept). Returns (0.0, 0.0) for fewer than 2 points.
pub fn linear_regression(points: &[(f32, f32)]) -> (f32, f32) {
    let n = points.len() as f32;
    if n < 2.0 {
        return (0.0, 0.0);
    }

    let sum_x: f32 = points.iter().map(|(x, _)| x).sum();
    let sum_y: f32 = points.iter().map(|(_, y)| y).sum();
    let sum_xy: f32 = points.iter().map(|(x, y)| x * y).sum();
    let sum_x2: f32 = points.iter().map(|(x, _)| x * x).sum();

    let denom = n * sum_x2 - sum_x * sum_x;
    if denom.abs() < f32::EPSILON {
        return (0.0, sum_y / n);
    }

    let slope = (n * sum_xy - sum_x * sum_y) / denom;
    let intercept = (sum_y - slope * sum_x) / n;

    (slope, intercept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_db_full_scale() {
        // A signal that hits exactly 1.0 should be 0 dB
        let samples = vec![0.0, 0.5, 1.0, -0.5];
        assert!((peak_db(&samples) - 0.0).abs() < 0.01);
    }

    #[test]
    fn peak_db_half_scale() {
        // Peak of 0.5 → 20*log10(0.5) ≈ -6.02 dB
        let samples = vec![0.0, 0.5, -0.3];
        assert!((peak_db(&samples) - (-6.02)).abs() < 0.1);
    }

    #[test]
    fn peak_db_silence() {
        let samples = vec![0.0, 0.0, 0.0];
        assert!(peak_db(&samples).is_infinite());
        assert!(peak_db(&samples).is_sign_negative());
    }

    #[test]
    fn rms_db_full_scale_dc() {
        // Constant 1.0 → RMS = 1.0 → 0 dB
        let samples = vec![1.0, 1.0, 1.0, 1.0];
        assert!((rms_db(&samples) - 0.0).abs() < 0.01);
    }

    #[test]
    fn rms_db_half_scale_dc() {
        // Constant 0.5 → RMS = 0.5 → -6.02 dB
        let samples = vec![0.5, 0.5, 0.5, 0.5];
        assert!((rms_db(&samples) - (-6.02)).abs() < 0.1);
    }

    #[test]
    fn rms_db_silence() {
        let samples = vec![0.0, 0.0, 0.0];
        assert!(rms_db(&samples).is_infinite());
        assert!(rms_db(&samples).is_sign_negative());
    }

    #[test]
    fn rms_db_empty() {
        assert!(rms_db(&[]).is_infinite());
    }

    #[test]
    fn resolve_date_explicit() {
        let date = resolve_date(Some("2026-02-08")).unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 2, 8).unwrap());
    }

    #[test]
    fn resolve_date_today() {
        let date = resolve_date(None).unwrap();
        assert_eq!(date, chrono::Local::now().date_naive());
    }

    #[test]
    fn resolve_date_invalid() {
        assert!(resolve_date(Some("not-a-date")).is_err());
    }

    #[test]
    fn linear_regression_known_slope() {
        // y = 2x + 1: points (0,1), (1,3), (2,5), (3,7)
        let points = vec![(0.0, 1.0), (1.0, 3.0), (2.0, 5.0), (3.0, 7.0)];
        let (slope, intercept) = linear_regression(&points);
        assert!((slope - 2.0).abs() < 0.01, "Expected slope ~2.0, got {slope:.3}");
        assert!((intercept - 1.0).abs() < 0.01, "Expected intercept ~1.0, got {intercept:.3}");
    }

    #[test]
    fn linear_regression_flat() {
        let points = vec![(0.0, 5.0), (1.0, 5.0), (2.0, 5.0)];
        let (slope, _intercept) = linear_regression(&points);
        assert!(slope.abs() < 0.01, "Expected slope ~0, got {slope:.3}");
    }

    #[test]
    fn linear_regression_declining() {
        // y = -1x + 10: points (0,10), (1,9), (2,8), (3,7), (4,6)
        let points = vec![(0.0, 10.0), (1.0, 9.0), (2.0, 8.0), (3.0, 7.0), (4.0, 6.0)];
        let (slope, intercept) = linear_regression(&points);
        assert!((slope - (-1.0)).abs() < 0.01, "Expected slope ~-1.0, got {slope:.3}");
        assert!((intercept - 10.0).abs() < 0.01, "Expected intercept ~10.0, got {intercept:.3}");
    }

    #[test]
    fn linear_regression_too_few_points() {
        let (slope, intercept) = linear_regression(&[(1.0, 2.0)]);
        assert_eq!(slope, 0.0);
        assert_eq!(intercept, 0.0);
    }
}
