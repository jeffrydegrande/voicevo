use std::path::Path;

use anyhow::{Context, Result};
use console::style;

use crate::audio::wav;
use crate::config::AppConfig;
use crate::paths;
use crate::storage::session_data::*;
use crate::storage::store;
use crate::util;

/// Analyze all recordings for a given date and save the results.
///
/// This is the main entry point for `voicevo analyze --date YYYY-MM-DD`.
/// It loads each WAV file, runs the appropriate analysis pipeline, collects
/// the results into a SessionData, and saves it as JSON.
pub fn analyze_session(date: &str, app_config: &AppConfig) -> Result<SessionData> {
    let sustained_pitch = app_config.analysis.pitch_config_for("sustained");
    let scale_pitch = app_config.analysis.pitch_config_for("scale");
    let reading_pitch = app_config.analysis.pitch_config_for("reading");

    println!(
        "Analyzing session {}...",
        style(date).cyan()
    );
    println!();

    let date_obj = util::resolve_date(Some(date))?;

    // Find latest attempt for each exercise
    let sustained_path = paths::latest_attempt_path(&date_obj, "sustained");
    let scale_path = paths::latest_attempt_path(&date_obj, "scale");
    let reading_path = paths::latest_attempt_path(&date_obj, "reading");

    // Analyze each exercise that has a recording.
    // We print results as we go so the user gets immediate feedback.

    let thresholds = &app_config.analysis.thresholds;

    let sustained = if let Some(ref p) = sustained_path {
        Some(analyze_exercise(
            "Sustained vowel",
            p,
            |samples, sr| {
                let result = super::sustained::analyze(samples, sr, &sustained_pitch)?;
                print_sustained_results(&result, thresholds);
                Ok(result)
            },
        )?)
    } else {
        println!("  {} sustained recording not found, skipping", style("SKIP").yellow());
        None
    };

    let scale = if let Some(ref p) = scale_path {
        Some(analyze_exercise(
            "Chromatic scale",
            p,
            |samples, sr| {
                let result = super::scale::analyze(samples, sr, &scale_pitch)?;
                print_scale_results(&result);
                Ok(result)
            },
        )?)
    } else {
        println!("  {} scale recording not found, skipping", style("SKIP").yellow());
        None
    };

    let reading = if let Some(ref p) = reading_path {
        Some(analyze_exercise(
            "Reading passage",
            p,
            |samples, sr| {
                let result = super::reading::analyze(samples, sr, &reading_pitch)?;
                print_reading_results(&result);
                Ok(result)
            },
        )?)
    } else {
        println!("  {} reading recording not found, skipping", style("SKIP").yellow());
        None
    };

    let session = SessionData {
        date: date.to_string(),
        recordings: SessionRecordings {
            sustained: sustained_path.map(|p| p.to_string_lossy().into()),
            scale: scale_path.map(|p| p.to_string_lossy().into()),
            reading: reading_path.map(|p| p.to_string_lossy().into()),
        },
        analysis: SessionAnalysis {
            sustained,
            scale,
            reading,
            sz: None,
            fatigue: None,
        },
    };

    // Save results
    store::save_session(&session)?;
    println!();
    println!(
        "Results saved to {}",
        style(crate::paths::db_path().display()).green()
    );

    Ok(session)
}

/// Helper: load a WAV file, run an analysis function, and handle errors.
///
/// This is a pattern using generics and closures:
/// - `F` is a generic type parameter: any function/closure that takes `(&[f32], u32)`
///   and returns `Result<T>`.
/// - `where F: FnOnce(...)` is a "trait bound" — it constrains what `F` can be.
///   FnOnce means the closure is called exactly once (it might consume captured values).
/// - The compiler generates specialized code for each `T` and `F` at compile time
///   (monomorphization) — there's no runtime overhead from this abstraction.
fn analyze_exercise<T, F>(name: &str, path: &Path, analyze_fn: F) -> Result<T>
where
    F: FnOnce(&[f32], u32) -> Result<T>,
{
    println!("  {} {name}", style(">>").cyan());

    let (samples, spec) = wav::load_samples(path)
        .with_context(|| format!("Failed to load {}", path.display()))?;

    let duration = samples.len() as f32 / spec.sample_rate as f32;
    println!("     Loaded: {:.1}s, {} Hz", duration, spec.sample_rate);

    let result = analyze_fn(&samples, spec.sample_rate)?;

    println!();
    Ok(result)
}

fn print_sustained_results(r: &SustainedAnalysis, t: &crate::config::ThresholdConfig) {
    println!("     MPT:      {:.1}s", r.mpt_seconds);
    println!("     Mean F0:  {:.1} Hz", r.mean_f0_hz);
    println!("     F0 std:   {:.1} Hz", r.f0_std_hz);
    println!(
        "     Jitter:   {:.2}% {}",
        r.jitter_local_percent,
        threshold_label(r.jitter_local_percent, t.jitter_pathological)
    );
    println!(
        "     Shimmer:  {:.2}% {}",
        r.shimmer_local_percent,
        threshold_label(r.shimmer_local_percent, t.shimmer_pathological)
    );
    println!(
        "     HNR:      {:.1} dB {}",
        r.hnr_db,
        hnr_label(r.hnr_db, t)
    );
    if let Some(cpps) = r.cpps_db {
        println!(
            "     CPPS:     {:.1} dB {}",
            cpps,
            cpps_label(cpps)
        );
    }
    if let Some(p) = r.periodicity_mean {
        println!("     Periodicity: {:.2}", p);
    }
    if let Some(ref rel) = r.reliability {
        println!(
            "     Quality:  {} (active {:.0}%, pitched {:.0}%, tier {})",
            style(&rel.analysis_quality).cyan(),
            rel.active_fraction * 100.0,
            rel.pitched_fraction * 100.0,
            rel.dominant_tier,
        );
    }
}

fn print_scale_results(r: &ScaleAnalysis) {
    println!("     Floor:      {:.1} Hz", r.pitch_floor_hz);
    println!("     Ceiling:    {:.1} Hz", r.pitch_ceiling_hz);
    println!("     Range:      {:.1} Hz ({:.1} semitones)", r.range_hz, r.range_semitones);
}

fn print_reading_results(r: &ReadingAnalysis) {
    println!("     Mean F0:    {:.1} Hz", r.mean_f0_hz);
    println!("     F0 std:     {:.1} Hz", r.f0_std_hz);
    println!(
        "     F0 range:   {:.1} - {:.1} Hz",
        r.f0_range_hz.0, r.f0_range_hz.1
    );
    println!("     Breaks:     {}", r.voice_breaks);
    println!("     Voiced:     {:.0}%", r.voiced_fraction * 100.0);
    if let Some(cpps) = r.cpps_db {
        println!(
            "     CPPS:       {:.1} dB {}",
            cpps,
            cpps_label(cpps)
        );
    }
    if let Some(ref rel) = r.reliability {
        println!(
            "     Quality:    {} (active {:.0}%, pitched {:.0}%, tier {})",
            style(&rel.analysis_quality).cyan(),
            rel.active_fraction * 100.0,
            rel.pitched_fraction * 100.0,
            rel.dominant_tier,
        );
    }
}

/// Format a label for metrics where lower is better (jitter, shimmer).
fn threshold_label(value: f32, threshold: f32) -> String {
    if value <= threshold {
        format!("{}", style("(normal)").green())
    } else {
        format!("{}", style("(elevated)").yellow())
    }
}

/// Format a label for CPPS. Normal ~5-10 dB, < 3 dB = significant dysphonia.
fn cpps_label(cpps: f32) -> String {
    if cpps >= 5.0 {
        format!("{}", style("(normal)").green())
    } else if cpps >= 3.0 {
        format!("{}", style("(mild dysphonia)").yellow())
    } else {
        format!("{}", style("(significant dysphonia)").red())
    }
}

/// Format a label for HNR where higher is better.
fn hnr_label(hnr: f32, t: &crate::config::ThresholdConfig) -> String {
    if hnr >= t.hnr_normal {
        format!("{}", style("(healthy)").green())
    } else if hnr >= t.hnr_low {
        format!("{}", style("(improving)").yellow())
    } else {
        format!("{}", style("(low)").red())
    }
}
