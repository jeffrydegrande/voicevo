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

/// Merge voiced runs separated by small gaps (≤ max_gap_frames unvoiced frames).
///
/// Pitch detectors sometimes drop a frame or two in the middle of continuous
/// voicing, fragmenting one long run into many short ones. This causes MPT
/// to under-report. Merging runs with tiny gaps (e.g., 3 frames × 10ms = 30ms)
/// fixes this without affecting voice break detection, which uses a 50ms floor.
pub fn merge_close_runs(runs: &[(usize, usize)], max_gap_frames: usize) -> Vec<(usize, usize)> {
    if runs.is_empty() {
        return Vec::new();
    }

    let mut merged = Vec::new();
    let (mut cur_start, mut cur_end) = runs[0];

    for &(start, end) in &runs[1..] {
        let gap = start.saturating_sub(cur_end + 1);
        if gap <= max_gap_frames {
            cur_end = end;
        } else {
            merged.push((cur_start, cur_end));
            cur_start = start;
            cur_end = end;
        }
    }

    merged.push((cur_start, cur_end));
    merged
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

    #[test]
    fn merge_close_runs_empty() {
        assert_eq!(merge_close_runs(&[], 3), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn merge_close_runs_single() {
        assert_eq!(merge_close_runs(&[(0, 10)], 3), vec![(0, 10)]);
    }

    #[test]
    fn merge_close_runs_bridges_small_gap() {
        // Two runs separated by 2 unvoiced frames → merged with max_gap=3
        let runs = vec![(0, 5), (8, 15)]; // gap: 8 - 5 - 1 = 2
        assert_eq!(merge_close_runs(&runs, 3), vec![(0, 15)]);
    }

    #[test]
    fn merge_close_runs_keeps_large_gap() {
        // Two runs separated by 10 unvoiced frames → NOT merged with max_gap=3
        let runs = vec![(0, 5), (16, 25)]; // gap: 16 - 5 - 1 = 10
        assert_eq!(merge_close_runs(&runs, 3), vec![(0, 5), (16, 25)]);
    }

    #[test]
    fn merge_close_runs_chains_multiple() {
        // Three runs with small gaps between each → all merge into one
        let runs = vec![(0, 10), (12, 20), (23, 30)]; // gaps: 1, 2
        assert_eq!(merge_close_runs(&runs, 3), vec![(0, 30)]);
    }

    #[test]
    fn merge_close_runs_mixed() {
        // Small gap then large gap → first two merge, third stays separate
        let runs = vec![(0, 10), (12, 20), (30, 40)]; // gaps: 1, 9
        assert_eq!(merge_close_runs(&runs, 3), vec![(0, 20), (30, 40)]);
    }
}
