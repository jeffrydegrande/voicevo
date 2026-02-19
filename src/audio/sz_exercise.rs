use std::sync::atomic::Ordering;

use anyhow::Result;
use console::style;

use crate::analysis::sz;
use crate::audio::capture;
use crate::config::AppConfig;
use crate::storage;

/// Run the S/Z ratio exercise with TUI.
///
/// The patient sustains /s/ (voiceless) and /z/ (voiced) multiple times.
/// We measure durations and compute the ratio.
pub fn run_sz_exercise(_config: &AppConfig) -> Result<()> {
    println!();
    println!("{}", style("=== S/Z Ratio Test ===").bold());
    println!();
    println!("  This test compares how long you can hold /s/ vs /z/.");
    println!("  A ratio close to 1.0 means healthy vocal cord function.");
    println!("  A ratio above 1.4 may indicate air leak through the vocal cords.");
    println!();
    println!("  Press {} when ready.", style("Enter").green().bold());

    crate::audio::recorder::wait_for_enter()?;

    // TUI phase
    let mut terminal = crate::tui::init()?;
    let (audio_state, stream, collector) = capture::start_capture(false)?;

    let outcome = crate::tui::screens::sz::run(&mut terminal, &audio_state)?;

    crate::tui::restore()?;

    // Stop audio
    audio_state.stop.store(true, Ordering::Relaxed);
    drop(stream);
    drop(collector);

    // Compute results (normal stdout)
    match sz::compute_sz(outcome.s_durations, outcome.z_durations) {
        Some(result) => {
            println!();
            println!("{}", style("Results").bold());
            println!();
            println!("  Mean /s/: {:.1}s", result.mean_s);
            println!("  Mean /z/: {:.1}s", result.mean_z);
            println!(
                "  S/Z ratio: {:.2} {}",
                result.sz_ratio,
                if result.sz_ratio > 1.4 {
                    style("(elevated — may indicate glottal air leak)").red().to_string()
                } else {
                    style("(normal range)").green().to_string()
                }
            );

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
            session.analysis.sz = Some(result);
            storage::store::save_session(&session)?;
            println!();
            println!("  Results saved.");
        }
        None => {
            println!();
            println!("  {} Insufficient data to compute S/Z ratio.", style("ERROR").red().bold());
        }
    }

    println!();
    Ok(())
}
