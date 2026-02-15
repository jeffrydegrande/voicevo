use anyhow::Result;

use crate::dsp::{contour, pitch, voice_breaks};
use crate::storage::session_data::ReadingAnalysis;

/// Analyze a reading passage recording.
///
/// The user reads a standard passage (e.g., The Rainbow Passage) at their
/// normal speaking pace. We extract:
///   - Mean speaking F0 and standard deviation
///   - F0 range (5th to 95th percentile)
///   - Voice break count
///   - Voiced fraction (what % of the time pitch was detected)
pub fn analyze(
    samples: &[f32],
    sample_rate: u32,
    pitch_config: &pitch::PitchConfig,
) -> Result<ReadingAnalysis> {
    let pitch_contour = pitch::extract_pitch_contour(samples, sample_rate, pitch_config);

    let mut frequencies = pitch::voiced_frequencies(&pitch_contour);
    let vf = pitch::voiced_fraction(&pitch_contour);

    if frequencies.is_empty() {
        anyhow::bail!(
            "No voiced frames detected in reading passage. \
             Recording may be silent or too quiet."
        );
    }

    // F0 statistics
    let mean_f0: f32 = frequencies.iter().sum::<f32>() / frequencies.len() as f32;
    let variance: f32 = frequencies.iter().map(|&f| (f - mean_f0).powi(2)).sum::<f32>()
        / frequencies.len() as f32;
    let f0_std = variance.sqrt();

    // F0 range using percentiles (robust to outliers)
    frequencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let f0_low = contour::percentile(&frequencies, 0.05);
    let f0_high = contour::percentile(&frequencies, 0.95);

    // Voice breaks
    let breaks = voice_breaks::count_voice_breaks(&pitch_contour, pitch_config.hop_size_ms);

    Ok(ReadingAnalysis {
        mean_f0_hz: mean_f0,
        f0_std_hz: f0_std,
        f0_range_hz: (f0_low, f0_high),
        voice_breaks: breaks,
        voiced_fraction: vf,
    })
}
