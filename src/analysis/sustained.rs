use anyhow::Result;

use crate::dsp::{hnr, jitter, mpt, pitch, shimmer};
use crate::storage::session_data::SustainedAnalysis;

/// Analyze a sustained vowel recording.
///
/// Uses three-tier pitch detection fallback for breathy voices.
/// Jitter is only computed from real pitch measurements (tiers 1-2).
/// Shimmer and HNR use pitch period only for window sizing, so they
/// still produce meaningful results with estimated pitch.
pub fn analyze(
    samples: &[f32],
    sample_rate: u32,
    pitch_config: &pitch::PitchConfig,
) -> Result<SustainedAnalysis> {
    let result = pitch::extract_contour_with_fallback(samples, sample_rate, pitch_config);
    let contour = &result.contour;

    let frequencies = pitch::voiced_frequencies(contour);

    if frequencies.is_empty() {
        anyhow::bail!(
            "No voiced frames detected in sustained vowel. \
             Recording may be silent or too quiet."
        );
    }

    // F0 statistics
    let mean_f0: f32 = frequencies.iter().sum::<f32>() / frequencies.len() as f32;
    let variance: f32 = frequencies.iter().map(|&f| (f - mean_f0).powi(2)).sum::<f32>()
        / frequencies.len() as f32;
    let f0_std = variance.sqrt();

    // Jitter requires real pitch measurements — skip when using energy fallback
    // since all frames have the same estimated pitch (jitter would be 0%).
    let jitter_percent = if result.used_energy_fallback {
        0.0
    } else {
        jitter::local_jitter_percent(contour).unwrap_or(0.0)
    };

    // Shimmer and HNR use pitch period only for window sizing, so they
    // still produce meaningful results with estimated pitch.
    let shimmer_percent =
        shimmer::local_shimmer_percent(samples, sample_rate, contour, pitch_config.hop_size_ms)
            .unwrap_or(0.0);
    let hnr_db =
        hnr::compute_hnr_db(samples, sample_rate, contour, pitch_config.hop_size_ms)
            .unwrap_or(0.0);

    // Maximum phonation time
    let mpt_seconds = mpt::max_phonation_time_secs(contour, pitch_config.hop_size_ms);

    let detection_quality = if result.detection_quality == "pitch" {
        None
    } else {
        Some(result.detection_quality.clone())
    };

    Ok(SustainedAnalysis {
        mpt_seconds,
        mean_f0_hz: mean_f0,
        f0_std_hz: f0_std,
        jitter_local_percent: jitter_percent,
        shimmer_local_percent: shimmer_percent,
        hnr_db,
        detection_quality,
    })
}
