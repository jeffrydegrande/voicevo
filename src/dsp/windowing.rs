use std::f32::consts::PI;

/// Apply a Hanning window to a slice of samples, returning a new Vec.
///
/// The Hanning (also called Hann) window smoothly tapers a frame of audio
/// to zero at both edges. This prevents spectral leakage — the artifacts you'd
/// get from abruptly chopping a signal in the middle of a cycle.
///
/// Formula: w(n) = 0.5 * (1 - cos(2π * n / (N - 1)))
///
/// At n=0 and n=N-1 (the edges), w = 0.0 — the signal fades out.
/// At n=N/2 (the center), w = 1.0 — the signal passes through unchanged.
pub fn hanning(samples: &[f32]) -> Vec<f32> {
    let n = samples.len();
    if n <= 1 {
        return samples.to_vec();
    }

    let scale = 2.0 * PI / (n - 1) as f32;

    samples
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let w = 0.5 * (1.0 - (scale * i as f32).cos());
            s * w
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hanning_edges_are_zero() {
        let samples = vec![1.0; 100];
        let windowed = hanning(&samples);

        // First and last samples should be (very close to) zero
        assert!(windowed[0].abs() < 1e-6);
        assert!(windowed[99].abs() < 1e-6);
    }

    #[test]
    fn hanning_center_is_one() {
        let n = 101; // odd length so there's an exact center
        let samples = vec![1.0; n];
        let windowed = hanning(&samples);

        // Middle sample should be ~1.0
        assert!((windowed[50] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hanning_is_symmetric() {
        let samples = vec![1.0; 64];
        let windowed = hanning(&samples);

        for i in 0..32 {
            assert!(
                (windowed[i] - windowed[63 - i]).abs() < 1e-6,
                "Asymmetry at index {i}"
            );
        }
    }

    #[test]
    fn hanning_preserves_silence() {
        let samples = vec![0.0; 50];
        let windowed = hanning(&samples);
        assert!(windowed.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn hanning_single_sample() {
        let windowed = hanning(&[0.5]);
        assert_eq!(windowed, vec![0.5]);
    }

    #[test]
    fn hanning_empty() {
        let windowed = hanning(&[]);
        assert!(windowed.is_empty());
    }
}
