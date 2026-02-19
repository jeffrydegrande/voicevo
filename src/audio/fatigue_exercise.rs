use std::sync::atomic::Ordering;

use anyhow::Result;
use console::style;

use crate::analysis::fatigue;
use crate::audio::capture;
use crate::config::AppConfig;
use crate::dsp::cpps;
use crate::storage;

/// Run the fatigue slope exercise with TUI.
///
/// The patient performs 5 sustained vowel attempts with 45s rest between each.
/// We measure MPT, CPPS, and effort for each trial, then compute the slope
/// to detect vocal fatigue.
pub fn run_fatigue_exercise(_config: &AppConfig) -> Result<()> {
    println!();
    println!("{}", style("=== Vocal Fatigue Test ===").bold());
    println!();
    println!("  You'll hold \"AAAH\" 5 times with 45s rest between trials.");
    println!("  This measures if your voice tires with repeated use.");
    println!();
    println!("  Press {} when ready.", style("Enter").green().bold());

    crate::audio::recorder::wait_for_enter()?;

    // TUI phase
    let mut terminal = crate::tui::init()?;
    let (audio_state, stream, collector) = capture::start_capture(false)?;
    let sample_rate = audio_state.sample_rate;

    let outcome = crate::tui::screens::fatigue::run(
        &mut terminal,
        &audio_state,
    )?;

    crate::tui::restore()?;

    // Stop audio, collect samples
    audio_state.stop.store(true, Ordering::Relaxed);
    drop(stream);

    let all_samples = collector
        .join()
        .map_err(|_| anyhow::anyhow!("Collector thread panicked"))?;

    // Compute CPPS for each trial (approximate from full recording split by MPT durations)
    let mut cpps_per_trial: Vec<Option<f32>> = Vec::new();
    let mut sample_offset = 0usize;
    for &mpt in &outcome.mpt_per_trial {
        let trial_samples = (mpt * sample_rate as f32) as usize;
        let end = (sample_offset + trial_samples).min(all_samples.len());
        if end > sample_offset {
            let trial_data = &all_samples[sample_offset..end];
            let c = cpps::compute_cpps(trial_data, sample_rate, &cpps::CppsConfig::default());
            cpps_per_trial.push(c);
        } else {
            cpps_per_trial.push(None);
        }
        // Account for the rest period too (roughly 45s of silence between trials)
        sample_offset = end + (45 * sample_rate as usize);
    }

    // Compute results (normal stdout)
    let mpt_per_trial = outcome.mpt_per_trial;
    let effort_per_trial = outcome.effort_per_trial;

    match fatigue::compute_fatigue(mpt_per_trial, cpps_per_trial, effort_per_trial) {
        Some(result) => {
            println!();
            println!("{}", style("Results").bold());
            println!();

            for (i, mpt) in result.mpt_per_trial.iter().enumerate() {
                let cpps_str = result.cpps_per_trial[i]
                    .map(|c| format!(", CPPS={c:.1}dB"))
                    .unwrap_or_default();
                println!(
                    "  Trial {}: {:.1}s{}, effort={}",
                    i + 1,
                    mpt,
                    cpps_str,
                    result.effort_per_trial[i],
                );
            }
            println!();

            let mpt_direction = if result.mpt_slope < -0.3 {
                style("declining (vocal fatigue)").red().to_string()
            } else if result.mpt_slope > 0.3 {
                style("improving (warming up)").green().to_string()
            } else {
                style("stable (good endurance)").green().to_string()
            };
            println!("  MPT slope: {:+.2}s/trial — {}", result.mpt_slope, mpt_direction);

            if result.cpps_slope != 0.0 {
                let cpps_direction = if result.cpps_slope < -0.2 {
                    "declining"
                } else if result.cpps_slope > 0.2 {
                    "improving"
                } else {
                    "stable"
                };
                println!("  CPPS slope: {:+.2}dB/trial — {}", result.cpps_slope, cpps_direction);
            }

            // Save to today's session
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            let mut session = match storage::store::load_session(&date) {
                Ok(s) => s,
                Err(_) => crate::storage::session_data::SessionData {
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
                },
            };
            session.analysis.fatigue = Some(result);
            storage::store::save_session(&session)?;
            println!();
            println!("  Results saved.");
        }
        None => {
            println!();
            println!("  {} Insufficient data to compute fatigue slope.", style("ERROR").red().bold());
        }
    }

    println!();
    Ok(())
}
