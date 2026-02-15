use std::path::Path;

use anyhow::{Context, Result};
use console::style;

use crate::audio::wav;
use crate::config::AppConfig;
use crate::dsp::pitch::PitchConfig;
use crate::storage::session_data::*;
use crate::storage::store;
use crate::util;

/// Analyze all recordings for a given date and save the results.
///
/// This is the main entry point for `voice-tracker analyze --date YYYY-MM-DD`.
/// It loads each WAV file, runs the appropriate analysis pipeline, collects
/// the results into a SessionData, and saves it as JSON.
pub fn analyze_session(date: &str, app_config: &AppConfig) -> Result<SessionData> {
    let pitch_config: PitchConfig = (&app_config.analysis).into();

    println!(
        "Analyzing session {}...",
        style(date).cyan()
    );
    println!();

    let date_obj = util::resolve_date(Some(date))?;

    // Check which recordings exist
    let sustained_path = util::recording_path(&date_obj, "sustained");
    let scale_path = util::recording_path(&date_obj, "scale");
    let reading_path = util::recording_path(&date_obj, "reading");

    // Analyze each exercise that has a recording.
    // We print results as we go so the user gets immediate feedback.

    let thresholds = &app_config.analysis.thresholds;

    let sustained = if sustained_path.exists() {
        Some(analyze_exercise(
            "Sustained vowel",
            &sustained_path,
            |samples, sr| {
                let result = super::sustained::analyze(samples, sr, &pitch_config)?;
                print_sustained_results(&result, thresholds);
                Ok(result)
            },
        )?)
    } else {
        println!("  {} sustained.wav not found, skipping", style("SKIP").yellow());
        None
    };

    let scale = if scale_path.exists() {
        Some(analyze_exercise(
            "Chromatic scale",
            &scale_path,
            |samples, sr| {
                let result = super::scale::analyze(samples, sr, &pitch_config)?;
                print_scale_results(&result);
                Ok(result)
            },
        )?)
    } else {
        println!("  {} scale.wav not found, skipping", style("SKIP").yellow());
        None
    };

    let reading = if reading_path.exists() {
        Some(analyze_exercise(
            "Reading passage",
            &reading_path,
            |samples, sr| {
                let result = super::reading::analyze(samples, sr, &pitch_config)?;
                print_reading_results(&result);
                Ok(result)
            },
        )?)
    } else {
        println!("  {} reading.wav not found, skipping", style("SKIP").yellow());
        None
    };

    let session = SessionData {
        date: date.to_string(),
        recordings: SessionRecordings {
            sustained: sustained_path.exists().then(|| sustained_path.to_string_lossy().into()),
            scale: scale_path.exists().then(|| scale_path.to_string_lossy().into()),
            reading: reading_path.exists().then(|| reading_path.to_string_lossy().into()),
        },
        analysis: SessionAnalysis {
            sustained,
            scale,
            reading,
        },
    };

    // Save results
    store::save_session(&session)?;
    let save_path = store::session_path(date);
    println!();
    println!(
        "Results saved to {}",
        style(save_path.display()).green()
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
}

/// Format a label for metrics where lower is better (jitter, shimmer).
fn threshold_label(value: f32, threshold: f32) -> String {
    if value <= threshold {
        format!("{}", style("(normal)").green())
    } else {
        format!("{}", style("(elevated)").yellow())
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
