use anyhow::Result;

use crate::dsp::{activity, cpps, hnr, jitter, mpt, periodicity, pitch, shimmer};
use crate::storage::session_data::{ReliabilityInfo, SustainedAnalysis};

/// Analyze a sustained vowel recording.
///
/// Uses three-tier pitch detection fallback for breathy voices.
/// Energy-based activity detection runs alongside pitch detection to
/// establish ground truth for "is the patient making sound?"
pub fn analyze(
    samples: &[f32],
    sample_rate: u32,
    pitch_config: &pitch::PitchConfig,
) -> Result<SustainedAnalysis> {
    // Activity detection — ground truth for sound production
    let activity_result = activity::detect_activity(samples, sample_rate, &activity::ActivityConfig::default());

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

    // Prefer gated jitter/shimmer (tier 1/2 frames only) for more accurate
    // measurements. Fall back to ungated if insufficient high-quality data.
    let jitter_percent = if result.used_energy_fallback {
        0.0
    } else {
        jitter::local_jitter_percent_gated(contour, &result.frame_tiers, pitch_config.hop_size_ms)
            .or_else(|| jitter::local_jitter_percent(contour))
            .unwrap_or(0.0)
    };

    let shimmer_percent = if result.used_energy_fallback {
        shimmer::local_shimmer_percent(samples, sample_rate, contour, pitch_config.hop_size_ms)
            .unwrap_or(0.0)
    } else {
        shimmer::local_shimmer_percent_gated(samples, sample_rate, contour, &result.frame_tiers, pitch_config.hop_size_ms)
            .or_else(|| shimmer::local_shimmer_percent(samples, sample_rate, contour, pitch_config.hop_size_ms))
            .unwrap_or(0.0)
    };

    let hnr_db =
        hnr::compute_hnr_db(samples, sample_rate, contour, pitch_config.hop_size_ms)
            .unwrap_or(0.0);

    // Maximum phonation time
    let mpt_seconds = mpt::max_phonation_time_secs(contour, pitch_config.hop_size_ms, 250.0);

    // CPPS — pitch-independent periodicity metric
    let cpps_db = cpps::compute_cpps(samples, sample_rate, &cpps::CppsConfig::default());

    // Periodicity score — mean normalized autocorrelation
    let periodicity_mean = periodicity::compute_periodicity(
        samples, sample_rate, contour, &activity_result.active_frames, pitch_config.hop_size_ms,
    );

    // Compute reliability info
    let pitched_fraction = activity::voiced_quality(contour, &activity_result.active_frames);
    let reliability = ReliabilityInfo::compute(
        result.tier_counts,
        activity_result.active_fraction,
        pitched_fraction,
        cpps_db.is_some(),
    );

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
        cpps_db,
        periodicity_mean,
        detection_quality,
        reliability: Some(reliability),
    })
}
