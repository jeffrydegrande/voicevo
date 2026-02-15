use anyhow::Result;

use crate::dsp::{contour, pitch};
use crate::storage::session_data::ScaleAnalysis;

/// Analyze a chromatic scale recording.
///
/// The user sings from their lowest comfortable note up to their highest,
/// then back down. We extract the pitch range:
///   - Floor: 5th percentile of detected F0
///   - Ceiling: 95th percentile of detected F0
///   - Range in Hz and semitones
///
/// We use percentiles instead of min/max to exclude outlier detections
/// (a stray frame at 30 Hz from a mic bump shouldn't set the floor).
pub fn analyze(
    samples: &[f32],
    sample_rate: u32,
    pitch_config: &pitch::PitchConfig,
) -> Result<ScaleAnalysis> {
    let pitch_contour = pitch::extract_pitch_contour(samples, sample_rate, pitch_config);
    let mut frequencies = pitch::voiced_frequencies(&pitch_contour);

    if frequencies.is_empty() {
        anyhow::bail!(
            "No voiced frames detected in scale recording. \
             Recording may be silent or too quiet."
        );
    }

    // Sort for percentile computation
    frequencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let floor = contour::percentile(&frequencies, 0.05);
    let ceiling = contour::percentile(&frequencies, 0.95);
    let range_hz = ceiling - floor;

    // Semitones: the musical unit of pitch interval.
    // 12 semitones = 1 octave = doubling of frequency.
    // semitones = 12 * log2(f2 / f1)
    let range_semitones = if floor > 0.0 {
        12.0 * (ceiling / floor).log2()
    } else {
        0.0
    };

    Ok(ScaleAnalysis {
        pitch_floor_hz: floor,
        pitch_ceiling_hz: ceiling,
        range_hz,
        range_semitones,
    })
}
