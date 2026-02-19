use std::sync::atomic::Ordering;

use anyhow::Result;
use console::style;

use crate::analysis::sustained;
use crate::audio::capture;
use crate::config::AppConfig;
use crate::dsp::pitch::PitchConfig;
use crate::storage;
use crate::util;

/// Minimum duration to attempt DSP analysis.
const MIN_ANALYSIS_DURATION_SECS: f32 = 0.5;

/// Run the sustained phonation exercise with live feedback.
///
/// Three phases:
///   1. Setup — load reference MPT from session history, show instructions
///   2. Live recording — TUI with timer + volume meter + waveform, auto-stop on silence
///   3. Post-exercise — DSP analysis with comparison to reference (normal stdout)
pub fn run_sustain_exercise(config: &AppConfig) -> Result<()> {
    // --- Phase 1: Setup ---
    let reference_mpt = load_reference_mpt();

    println!();
    println!(
        "{}",
        style("=== Sustained Phonation Exercise ===").bold()
    );
    println!();
    println!("  Take a deep breath, then hold {} as long and steady as you can.", style("\"AAAH\"").cyan());
    println!("  The exercise auto-stops when you go silent.");
    println!();

    if let Some(mpt) = reference_mpt {
        println!("  Your last MPT: {:.1}s — try to match or beat it.", mpt);
    } else {
        println!("  No previous sessions found — just hold as long as you can.");
    }

    println!();
    println!(
        "  Press {} when ready.",
        style("Enter").green().bold()
    );

    crate::audio::recorder::wait_for_enter()?;

    // --- Phase 2: TUI recording ---
    let mut terminal = crate::tui::init()?;
    let (audio_state, stream, collector) = capture::start_capture(false)?;
    let sample_rate = audio_state.sample_rate;

    let outcome = crate::tui::screens::recording::run(
        &mut terminal,
        &audio_state,
        reference_mpt,
        "Sustained Phonation",
        "Hold \"AAAH\" as long and steady as you can.",
    )?;

    crate::tui::restore()?;

    // Stop audio, collect samples
    audio_state.stop.store(true, Ordering::Relaxed);
    drop(stream);

    let mut all_samples = collector
        .join()
        .map_err(|_| anyhow::anyhow!("Collector thread panicked"))?;

    // Trim trailing silence for cleaner DSP analysis
    let trailing_silence_secs = outcome.silent_polls as f32 * 0.033;
    let silence_samples = (trailing_silence_secs * sample_rate as f32) as usize;
    let trimmed_len = all_samples.len().saturating_sub(silence_samples);
    all_samples.truncate(trimmed_len);

    let phonation_secs = outcome.phonation_secs;

    // --- Phase 3: Post-exercise analysis (normal stdout) ---
    println!();
    println!("  {}", style("*** STOPPED ***").dim());
    println!();

    let peak_db = util::peak_db(&all_samples);

    if phonation_secs < MIN_ANALYSIS_DURATION_SECS {
        println!("  Recording too short ({:.1}s) — skipping analysis.", phonation_secs);
        return Ok(());
    }

    if peak_db < -60.0 {
        println!(
            "  {} Recording appears silent (peak {:.1} dB). Check your microphone.",
            style("WARNING").red().bold(),
            peak_db
        );
        return Ok(());
    }

    println!("  {}", style("Results").bold());
    println!();

    // MPT from the exercise timer (clinical stopwatch method)
    print_metric("MPT", &format!("{:.1}s", phonation_secs), reference_mpt.map(|r| {
        format_comparison(phonation_secs, r, "s", true)
    }));

    // Voice quality metrics from DSP analysis
    let pitch_config: PitchConfig = (&config.analysis).into();
    match sustained::analyze(&all_samples, sample_rate, &pitch_config) {
        Ok(result) => {
            print_metric("Mean F0", &format!("{:.1} Hz", result.mean_f0_hz), None);
            print_metric("Jitter", &format!("{:.2}%", result.jitter_local_percent),
                Some(rate_jitter(result.jitter_local_percent, &config.analysis.thresholds)));
            print_metric("Shimmer", &format!("{:.2}%", result.shimmer_local_percent),
                Some(rate_shimmer(result.shimmer_local_percent, &config.analysis.thresholds)));
            print_metric("HNR", &format!("{:.1} dB", result.hnr_db),
                Some(rate_hnr(result.hnr_db, &config.analysis.thresholds)));
            if let Some(cpps) = result.cpps_db {
                print_metric("CPPS", &format!("{:.1} dB", cpps),
                    Some(rate_cpps(cpps)));
            }
        }
        Err(e) => {
            println!(
                "  {} Voice quality analysis failed: {e}",
                style("NOTE").yellow().bold()
            );
        }
    }

    println!();

    Ok(())
}

/// Load the most recent MPT from session history as a reference target.
fn load_reference_mpt() -> Option<f32> {
    let dates = storage::store::list_sessions().ok()?;
    for date in dates.iter().rev() {
        if let Ok(session) = storage::store::load_session(date) {
            if let Some(ref s) = session.analysis.sustained {
                return Some(s.mpt_seconds);
            }
        }
    }
    None
}

/// Format a comparison string (e.g., "+1.2s" or "-0.5s").
fn format_comparison(current: f32, reference: f32, unit: &str, higher_is_better: bool) -> String {
    let diff = current - reference;
    let sign = if diff >= 0.0 { "+" } else { "" };
    let color_good = diff > 0.0 && higher_is_better || diff < 0.0 && !higher_is_better;

    let text = format!("{sign}{diff:.1}{unit}");
    if color_good {
        style(text).green().to_string()
    } else if diff.abs() < 0.1 {
        style(text).dim().to_string()
    } else {
        style(text).red().to_string()
    }
}

/// Print a metric line with optional annotation.
fn print_metric(label: &str, value: &str, annotation: Option<String>) {
    match annotation {
        Some(ann) => println!("  {:12} {:>12}  {}", style(label).bold(), value, ann),
        None => println!("  {:12} {:>12}", style(label).bold(), value),
    }
}

/// Rate jitter quality.
fn rate_jitter(jitter: f32, thresholds: &crate::config::ThresholdConfig) -> String {
    if jitter < thresholds.jitter_pathological {
        style("normal").green().to_string()
    } else {
        style("elevated").yellow().to_string()
    }
}

/// Rate shimmer quality.
fn rate_shimmer(shimmer: f32, thresholds: &crate::config::ThresholdConfig) -> String {
    if shimmer < thresholds.shimmer_pathological {
        style("normal").green().to_string()
    } else {
        style("elevated").yellow().to_string()
    }
}

/// Rate CPPS quality. Normal ~5-10 dB, < 3 dB = significant dysphonia.
fn rate_cpps(cpps: f32) -> String {
    if cpps >= 5.0 {
        style("normal").green().to_string()
    } else if cpps >= 3.0 {
        style("mild dysphonia").yellow().to_string()
    } else {
        style("significant dysphonia").red().to_string()
    }
}

/// Rate HNR quality.
fn rate_hnr(hnr: f32, thresholds: &crate::config::ThresholdConfig) -> String {
    if hnr >= thresholds.hnr_normal {
        style("healthy").green().to_string()
    } else if hnr >= thresholds.hnr_low {
        style("fair").yellow().to_string()
    } else {
        style("low").red().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_comparison_positive() {
        let result = format_comparison(10.0, 8.0, "s", true);
        assert!(result.contains("2.0s"));
    }

    #[test]
    fn format_comparison_negative() {
        let result = format_comparison(6.0, 8.0, "s", true);
        assert!(result.contains("2.0s"));
    }

    #[test]
    fn format_comparison_equal() {
        let result = format_comparison(8.0, 8.0, "s", true);
        assert!(result.contains("0.0s"));
    }

    #[test]
    fn rate_jitter_normal() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_jitter(0.5, &thresholds);
        assert!(result.contains("normal"));
    }

    #[test]
    fn rate_jitter_elevated() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_jitter(2.0, &thresholds);
        assert!(result.contains("elevated"));
    }

    #[test]
    fn rate_shimmer_normal() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_shimmer(2.0, &thresholds);
        assert!(result.contains("normal"));
    }

    #[test]
    fn rate_shimmer_elevated() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_shimmer(5.0, &thresholds);
        assert!(result.contains("elevated"));
    }

    #[test]
    fn rate_hnr_healthy() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_hnr(25.0, &thresholds);
        assert!(result.contains("healthy"));
    }

    #[test]
    fn rate_hnr_fair() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_hnr(12.0, &thresholds);
        assert!(result.contains("fair"));
    }

    #[test]
    fn rate_hnr_low() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_hnr(3.0, &thresholds);
        assert!(result.contains("low"));
    }

    #[test]
    fn load_reference_mpt_no_sessions() {
        let _ = load_reference_mpt();
    }
}
