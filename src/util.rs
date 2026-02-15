use std::path::PathBuf;

use chrono::NaiveDate;

/// Build the XDG-compliant path for a recording.
/// Delegates to `paths::recording_path` for consistent directory resolution.
pub fn recording_path(date: &NaiveDate, exercise: &str) -> PathBuf {
    crate::paths::recording_path(date, exercise)
}

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
    fn recording_path_format() {
        let date = NaiveDate::from_ymd_opt(2026, 2, 8).unwrap();
        let path = recording_path(&date, "sustained");
        // XDG-compliant: ends with the expected structure
        assert!(path.ends_with("recordings/2026-02-08/sustained.wav"));
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
}
