use crate::storage::session_data::FatigueAnalysis;
use crate::util;

/// Compute fatigue slope analysis from multiple sustained vowel trials.
///
/// The patient performs several sustained vowel trials (typically 5) with
/// rest periods between them. If MPT or CPPS decline across trials,
/// it indicates the vocal cords are fatiguing — they can't maintain
/// closure as effectively after repeated use.
///
/// A negative slope means declining performance (fatiguing).
/// A flat or positive slope means good endurance.
pub fn compute_fatigue(
    mpt_per_trial: Vec<f32>,
    cpps_per_trial: Vec<Option<f32>>,
    effort_per_trial: Vec<u8>,
) -> Option<FatigueAnalysis> {
    if mpt_per_trial.len() < 2 {
        return None;
    }

    // Compute MPT slope: trial index (0, 1, 2, ...) vs MPT
    let mpt_points: Vec<(f32, f32)> = mpt_per_trial
        .iter()
        .enumerate()
        .map(|(i, &mpt)| (i as f32, mpt))
        .collect();
    let (mpt_slope, _) = util::linear_regression(&mpt_points);

    // Compute CPPS slope from trials that have CPPS values
    let cpps_points: Vec<(f32, f32)> = cpps_per_trial
        .iter()
        .enumerate()
        .filter_map(|(i, cpps)| cpps.map(|c| (i as f32, c)))
        .collect();
    let cpps_slope = if cpps_points.len() >= 2 {
        util::linear_regression(&cpps_points).0
    } else {
        0.0
    };

    Some(FatigueAnalysis {
        mpt_per_trial,
        cpps_per_trial,
        effort_per_trial,
        mpt_slope,
        cpps_slope,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declining_mpt_negative_slope() {
        let result = compute_fatigue(
            vec![10.0, 9.0, 8.0, 7.0, 6.0],
            vec![None; 5],
            vec![3, 4, 5, 6, 7],
        )
        .unwrap();
        assert!(
            result.mpt_slope < -0.5,
            "Declining MPT should have negative slope, got {:.3}",
            result.mpt_slope
        );
    }

    #[test]
    fn stable_mpt_flat_slope() {
        let result = compute_fatigue(
            vec![10.0, 10.0, 10.0, 10.0],
            vec![None; 4],
            vec![3, 3, 3, 3],
        )
        .unwrap();
        assert!(
            result.mpt_slope.abs() < 0.1,
            "Stable MPT should have ~0 slope, got {:.3}",
            result.mpt_slope
        );
    }

    #[test]
    fn cpps_slope_computed() {
        let result = compute_fatigue(
            vec![10.0, 9.0, 8.0],
            vec![Some(8.0), Some(7.0), Some(6.0)],
            vec![3, 4, 5],
        )
        .unwrap();
        assert!(
            result.cpps_slope < -0.5,
            "Declining CPPS should have negative slope, got {:.3}",
            result.cpps_slope
        );
    }

    #[test]
    fn cpps_slope_zero_when_no_cpps() {
        let result = compute_fatigue(
            vec![10.0, 9.0, 8.0],
            vec![None, None, None],
            vec![3, 4, 5],
        )
        .unwrap();
        assert_eq!(result.cpps_slope, 0.0);
    }

    #[test]
    fn too_few_trials() {
        assert!(compute_fatigue(vec![10.0], vec![None], vec![3]).is_none());
    }
}
