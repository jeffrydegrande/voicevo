use super::pitch::PitchFrame;

/// Find consecutive runs of voiced frames in a pitch contour.
/// Returns a list of (start_index, end_index) pairs (inclusive).
///
/// This is used by several other modules:
/// - MPT uses the longest run to compute maximum phonation time
/// - Voice break detection looks for gaps between runs
/// - Jitter/shimmer need consecutive voiced frames to compute perturbation
pub fn voiced_runs(contour: &[PitchFrame]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;

    for (i, frame) in contour.iter().enumerate() {
        match (frame.frequency.is_some(), start) {
            // Voiced frame, not currently in a run → start one
            (true, None) => start = Some(i),
            // Voiced frame, already in a run → continue
            (true, Some(_)) => {}
            // Unvoiced frame, was in a run → end it
            (false, Some(s)) => {
                runs.push((s, i - 1));
                start = None;
            }
            // Unvoiced frame, not in a run → nothing to do
            (false, None) => {}
        }
    }

    // Don't forget a run that extends to the end of the contour
    if let Some(s) = start {
        runs.push((s, contour.len() - 1));
    }

    runs
}

/// Compute the duration of a run in seconds, given the hop size.
pub fn run_duration_secs(start: usize, end: usize, hop_size_ms: f32) -> f32 {
    (end - start + 1) as f32 * hop_size_ms / 1000.0
}

/// Compute percentile value from a sorted slice.
/// `p` is in [0.0, 1.0] — e.g., 0.05 for 5th percentile.
pub fn percentile(sorted: &[f32], p: f32) -> f32 {
    assert!(!sorted.is_empty(), "Cannot compute percentile of empty slice");
    let idx = (p * (sorted.len() - 1) as f32).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(time: f32, freq: Option<f32>) -> PitchFrame {
        PitchFrame {
            time,
            frequency: freq,
        }
    }

    #[test]
    fn single_voiced_run() {
        let contour = vec![
            frame(0.0, Some(100.0)),
            frame(0.01, Some(101.0)),
            frame(0.02, Some(99.0)),
        ];
        let runs = voiced_runs(&contour);
        assert_eq!(runs, vec![(0, 2)]);
    }

    #[test]
    fn gap_in_middle() {
        let contour = vec![
            frame(0.0, Some(100.0)),
            frame(0.01, Some(101.0)),
            frame(0.02, None), // gap
            frame(0.03, None),
            frame(0.04, Some(99.0)),
            frame(0.05, Some(100.0)),
        ];
        let runs = voiced_runs(&contour);
        assert_eq!(runs, vec![(0, 1), (4, 5)]);
    }

    #[test]
    fn all_unvoiced() {
        let contour = vec![frame(0.0, None), frame(0.01, None)];
        let runs = voiced_runs(&contour);
        assert!(runs.is_empty());
    }

    #[test]
    fn empty_contour() {
        let runs = voiced_runs(&[]);
        assert!(runs.is_empty());
    }

    #[test]
    fn run_duration() {
        // 10 frames at 10ms hop = 100ms = 0.1s
        assert!((run_duration_secs(0, 9, 10.0) - 0.1).abs() < 0.001);
    }

    #[test]
    fn percentile_basic() {
        let data: Vec<f32> = (0..100).map(|i| i as f32).collect();
        assert!((percentile(&data, 0.0) - 0.0).abs() < 0.5);
        assert!((percentile(&data, 0.5) - 50.0).abs() < 1.0);
        assert!((percentile(&data, 1.0) - 99.0).abs() < 0.5);
    }
}
