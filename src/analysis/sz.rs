use crate::storage::session_data::SzAnalysis;

/// Compute S/Z ratio analysis from measured durations.
///
/// The S/Z ratio compares how long a patient can sustain the voiceless
/// fricative /s/ (uses only airflow) vs. the voiced fricative /z/ (requires
/// vocal cord vibration). Since both use the same breath, a ratio > 1.4
/// suggests the vocal cords can't sustain vibration as efficiently as
/// they should — air is leaking through the glottal gap.
///
/// - Normal: S/Z ≈ 1.0 (both about the same duration)
/// - Concerning: S/Z > 1.4 (vocal cord dysfunction limits /z/)
pub fn compute_sz(s_durations: Vec<f32>, z_durations: Vec<f32>) -> Option<SzAnalysis> {
    if s_durations.is_empty() || z_durations.is_empty() {
        return None;
    }

    let mean_s = s_durations.iter().sum::<f32>() / s_durations.len() as f32;
    let mean_z = z_durations.iter().sum::<f32>() / z_durations.len() as f32;

    if mean_z == 0.0 {
        return None;
    }

    let sz_ratio = mean_s / mean_z;

    Some(SzAnalysis {
        s_durations,
        z_durations,
        mean_s,
        mean_z,
        sz_ratio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_ratio() {
        let result = compute_sz(vec![10.0, 11.0], vec![10.5, 10.0]).unwrap();
        assert!((result.sz_ratio - 1.0).abs() < 0.15);
        assert!((result.mean_s - 10.5).abs() < 0.01);
    }

    #[test]
    fn concerning_ratio() {
        // /s/ much longer than /z/ — cord dysfunction
        let result = compute_sz(vec![15.0, 14.0], vec![8.0, 7.0]).unwrap();
        assert!(result.sz_ratio > 1.4, "Expected ratio > 1.4, got {:.2}", result.sz_ratio);
    }

    #[test]
    fn single_trial_each() {
        let result = compute_sz(vec![12.0], vec![10.0]).unwrap();
        assert!((result.sz_ratio - 1.2).abs() < 0.01);
    }

    #[test]
    fn empty_s_returns_none() {
        assert!(compute_sz(vec![], vec![10.0]).is_none());
    }

    #[test]
    fn empty_z_returns_none() {
        assert!(compute_sz(vec![10.0], vec![]).is_none());
    }
}
