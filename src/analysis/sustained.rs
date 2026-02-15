use anyhow::Result;

use crate::dsp::{hnr, jitter, mpt, pitch, shimmer};
use crate::storage::session_data::SustainedAnalysis;

/// Analyze a sustained vowel recording.
///
/// This runs the full DSP pipeline:
///   1. Pitch tracking → F0 contour
///   2. Jitter (pitch perturbation)
///   3. Shimmer (amplitude perturbation)
///   4. HNR (breathiness)
///   5. MPT (longest continuous voicing)
///   6. F0 statistics (mean, std)
pub fn analyze(
    samples: &[f32],
    sample_rate: u32,
    pitch_config: &pitch::PitchConfig,
) -> Result<SustainedAnalysis> {
    let contour = pitch::extract_pitch_contour(samples, sample_rate, pitch_config);
    let frequencies = pitch::voiced_frequencies(&contour);

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

    // Voice quality metrics
    let jitter_percent = jitter::local_jitter_percent(&contour).unwrap_or(0.0);
    let shimmer_percent =
        shimmer::local_shimmer_percent(samples, sample_rate, &contour, pitch_config.hop_size_ms)
            .unwrap_or(0.0);
    let hnr_db =
        hnr::compute_hnr_db(samples, sample_rate, &contour, pitch_config.hop_size_ms)
            .unwrap_or(0.0);

    // Maximum phonation time
    let mpt_seconds = mpt::max_phonation_time_secs(&contour, pitch_config.hop_size_ms);

    Ok(SustainedAnalysis {
        mpt_seconds,
        mean_f0_hz: mean_f0,
        f0_std_hz: f0_std,
        jitter_local_percent: jitter_percent,
        shimmer_local_percent: shimmer_percent,
        hnr_db,
    })
}
