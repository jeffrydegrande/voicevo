use std::sync::atomic::Ordering;

use anyhow::Result;
use console::style;

use crate::analysis::scale;
use crate::audio::capture;
use crate::config::AppConfig;
use crate::dsp::pitch::PitchConfig;
use crate::storage;

/// Minimum duration to attempt DSP analysis.
const MIN_ANALYSIS_DURATION_SECS: f32 = 0.5;

/// Run the chromatic scale exercise with live pitch feedback.
pub fn run_scale_exercise(config: &AppConfig) -> Result<()> {
    println!();
    println!("{}", style("=== Chromatic Scale Exercise ===").bold());
    println!();
    println!("  Sing from your lowest note to your highest, then back down.");
    println!("  The display shows your current pitch in real time.");
    println!();
    println!("  Press {} when ready.", style("Enter").green().bold());

    crate::audio::recorder::wait_for_enter()?;

    // TUI phase: recording with live pitch feedback
    let mut terminal = crate::tui::init()?;
    let (audio_state, stream, collector) = capture::start_capture(true)?;
    let sample_rate = audio_state.sample_rate;

    let outcome = crate::tui::screens::scale::run(&mut terminal, &audio_state)?;

    crate::tui::restore()?;

    // Stop audio, collect samples
    audio_state.stop.store(true, Ordering::Relaxed);
    drop(stream);

    let mut all_samples = collector
        .join()
        .map_err(|_| anyhow::anyhow!("Collector thread panicked"))?;

    // Trim trailing silence
    let trailing_samples = (outcome.silent_polls as f32 * 0.033 * sample_rate as f32) as usize;
    let trimmed_len = all_samples.len().saturating_sub(trailing_samples);
    all_samples.truncate(trimmed_len);

    println!();
    println!("  {}", style("*** STOPPED ***").dim());
    println!();

    if outcome.phonation_secs < MIN_ANALYSIS_DURATION_SECS {
        println!("  Recording too short ({:.1}s) — skipping analysis.", outcome.phonation_secs);
        return Ok(());
    }

    let peak_db = crate::util::peak_db(&all_samples);
    if peak_db < -60.0 {
        println!(
            "  {} Recording appears silent (peak {:.1} dB). Check your microphone.",
            style("WARNING").red().bold(),
            peak_db
        );
        return Ok(());
    }

    // Analyze
    println!("  {}", style("Results").bold());
    println!();

    let pitch_config: PitchConfig = (&config.analysis).into();
    match scale::analyze(&all_samples, sample_rate, &pitch_config) {
        Ok(result) => {
            println!("  {:12} {:>12}", style("Pitch floor").bold(), format!("{:.1} Hz", result.pitch_floor_hz));
            println!("  {:12} {:>12}", style("Pitch ceil").bold(), format!("{:.1} Hz", result.pitch_ceiling_hz));
            println!("  {:12} {:>12}", style("Range").bold(), format!("{:.1} semitones", result.range_semitones));
            println!("  {:12} {:>12}", style("Duration").bold(), format!("{:.1}s", outcome.phonation_secs));

            // Save results
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            let mut session = storage::store::load_session(&date).unwrap_or_else(|_| {
                crate::storage::session_data::SessionData {
                    date: date.clone(),
                    recordings: crate::storage::session_data::SessionRecordings {
                        sustained: None,
                        scale: None,
                        reading: None,
                    },
                    analysis: crate::storage::session_data::SessionAnalysis {
                        sustained: None,
                        scale: None,
                        reading: None,
                        sz: None,
                        fatigue: None,
                    },
                    conditions: None,
                }
            });
            session.analysis.scale = Some(result);
            storage::store::save_session(&session)?;
            println!();
            println!("  Results saved.");
        }
        Err(e) => {
            println!(
                "  {} Scale analysis failed: {e}",
                style("NOTE").yellow().bold()
            );
        }
    }

    println!();
    Ok(())
}
