use super::contour;
use super::pitch::PitchFrame;

/// Count voice breaks in a pitch contour.
///
/// A voice break is an unexpected dropout in voicing during continuous speech:
///   voiced → unvoiced gap → voiced
///
/// We distinguish breaks from normal speech events:
///   < 50ms gap:  Normal unvoiced consonant (t, s, p) — not a break
///   50–max_break_ms:  Voice break — the cord lost vibration and restarted
///   > max_break_ms:   Intentional pause (breathing, thinking) — not a break
///
/// `max_break_ms` defaults to 250ms. This tighter threshold reduces false
/// positives from breathing pauses that are short but intentional.
///
/// Returns the number of voice breaks detected.
pub fn count_voice_breaks(contour: &[PitchFrame], hop_size_ms: f32, max_break_ms: f32) -> usize {
    let runs = contour::voiced_runs(contour);

    if runs.len() < 2 {
        return 0;
    }

    let min_gap_ms = 50.0;
    let max_gap_ms = max_break_ms;

    let mut breaks = 0;

    // Look at the gaps between consecutive voiced runs.
    // Each gap is the space between one run ending and the next one starting.
    for pair in runs.windows(2) {
        let (_, end_of_prev) = pair[0];
        let (start_of_next, _) = pair[1];

        // Gap in frames: from the frame after the previous run ends
        // to the frame before the next run starts
        let gap_frames = start_of_next - end_of_prev - 1;
        let gap_ms = gap_frames as f32 * hop_size_ms;

        if gap_ms >= min_gap_ms && gap_ms <= max_gap_ms {
            breaks += 1;
        }
    }

    breaks
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

    fn make_contour(pattern: &[(usize, bool)], hop_ms: f32) -> Vec<PitchFrame> {
        // pattern: list of (num_frames, voiced?)
        let mut contour = Vec::new();
        let mut t = 0.0;
        for &(count, voiced) in pattern {
            for _ in 0..count {
                let freq = if voiced { Some(100.0) } else { None };
                contour.push(frame(t, freq));
                t += hop_ms / 1000.0;
            }
        }
        contour
    }

    #[test]
    fn no_breaks_continuous_voicing() {
        let contour = make_contour(&[(100, true)], 10.0);
        assert_eq!(count_voice_breaks(&contour, 10.0, 250.0), 0);
    }

    #[test]
    fn one_break_80ms_gap() {
        // voiced (20 frames) → 80ms gap (8 frames) → voiced (20 frames)
        let contour = make_contour(&[(20, true), (8, false), (20, true)], 10.0);
        assert_eq!(count_voice_breaks(&contour, 10.0, 250.0), 1);
    }

    #[test]
    fn short_gap_not_a_break() {
        // 30ms gap (3 frames at 10ms) — normal consonant, not a break
        let contour = make_contour(&[(20, true), (3, false), (20, true)], 10.0);
        assert_eq!(count_voice_breaks(&contour, 10.0, 250.0), 0);
    }

    #[test]
    fn long_pause_not_a_break() {
        // 600ms gap (60 frames) — intentional pause, not a break
        let contour = make_contour(&[(20, true), (60, false), (20, true)], 10.0);
        assert_eq!(count_voice_breaks(&contour, 10.0, 250.0), 0);
    }

    #[test]
    fn multiple_breaks() {
        // voiced → 100ms gap → voiced → 200ms gap → voiced
        let contour = make_contour(
            &[(20, true), (10, false), (20, true), (20, false), (20, true)],
            10.0,
        );
        assert_eq!(count_voice_breaks(&contour, 10.0, 250.0), 2);
    }

    #[test]
    fn mixed_gaps() {
        // Short gap (not a break) + medium gap (break) + long gap (not a break)
        let contour = make_contour(
            &[
                (20, true),
                (3, false),  // 30ms — too short
                (20, true),
                (10, false), // 100ms — break
                (20, true),
                (60, false), // 600ms — too long
                (20, true),
            ],
            10.0,
        );
        assert_eq!(count_voice_breaks(&contour, 10.0, 250.0), 1);
    }
}
