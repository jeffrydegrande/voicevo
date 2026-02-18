use anyhow::Result;

use crate::dsp::{contour, pitch, voice_breaks};
use crate::storage::session_data::ReadingAnalysis;

/// Analyze a reading passage recording.
///
/// Uses three-tier pitch detection fallback for breathy voices.
/// Voice break count is zeroed when the energy fallback is used, since
/// gaps in an energy-based contour don't reflect real voicing gaps.
pub fn analyze(
    samples: &[f32],
    sample_rate: u32,
    pitch_config: &pitch::PitchConfig,
) -> Result<ReadingAnalysis> {
    let result = pitch::extract_contour_with_fallback(samples, sample_rate, pitch_config);
    let pitch_contour = &result.contour;

    let mut frequencies = pitch::voiced_frequencies(pitch_contour);
    let vf = pitch::voiced_fraction(pitch_contour);

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

    // Voice breaks: unreliable with energy fallback since gaps in the
    // energy contour reflect silence, not voicing loss.
    let breaks = if result.used_energy_fallback {
        0
    } else {
        voice_breaks::count_voice_breaks(pitch_contour, pitch_config.hop_size_ms)
    };

    let detection_quality = if result.detection_quality == "pitch" {
        None
    } else {
        Some(result.detection_quality.clone())
    };

    Ok(ReadingAnalysis {
        mean_f0_hz: mean_f0,
        f0_std_hz: f0_std,
        f0_range_hz: (f0_low, f0_high),
        voice_breaks: breaks,
        voiced_fraction: vf,
        detection_quality,
    })
}
