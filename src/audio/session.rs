use anyhow::Result;
use chrono::NaiveDate;
use console::style;

use crate::config::AppConfig;
use crate::paths;

use super::mic_check;
use super::recorder;

/// Run a full guided recording session.
///
/// This walks the user through:
///   1. Mic check (ensures the microphone is working)
///   2. Sustained vowel recording
///   3. Chromatic scale recording
///   4. Reading passage recording
///   5. Summary of all three recordings
pub fn run_guided_session(date: &NaiveDate, config: &AppConfig) -> Result<()> {
    println!();
    println!(
        "{}",
        style("=== Voice Recovery Tracker — Recording Session ===").bold()
    );
    println!("  Date: {}", style(date).cyan());
    println!();

    // --- Step 1: Mic check ---
    println!(
        "{} {}",
        style("Step 1/4:").bold(),
        "Mic check"
    );
    println!();

    mic_check::run()?;
    println!();

    println!(
        "  Press {} to continue to the exercises.",
        style("Enter").green().bold()
    );
    recorder::wait_for_enter()?;
    println!();

    // --- Step 2: Sustained vowel ---
    println!(
        "{} {}",
        style("Step 2/4:").bold(),
        "Sustained vowel"
    );
    println!();
    println!("  Take a deep breath, then hold {} as long as comfortable.", style("\"AAAH\"").cyan());
    println!();

    let sustained_path = paths::next_attempt_path(date, "sustained");
    if let Some(parent) = sustained_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let sustained_stats = record_exercise_with_path(&sustained_path)?;

    println!();

    // --- Step 3: Chromatic scale ---
    println!(
        "{} {}",
        style("Step 3/4:").bold(),
        "Chromatic scale"
    );
    println!();
    println!(
        "  Sing from your {} comfortable note up to your {},",
        style("lowest").cyan(),
        style("highest").cyan()
    );
    println!("  then back down.");
    println!();

    let scale_path = paths::next_attempt_path(date, "scale");
    if let Some(parent) = scale_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let scale_stats = record_exercise_with_path(&scale_path)?;

    println!();

    // --- Step 4: Reading passage ---
    println!(
        "{} {}",
        style("Step 4/4:").bold(),
        "Reading passage"
    );
    println!();
    println!("  Read the following at your normal speaking pace:");
    println!();
    for line in config.session.reading_passage.lines() {
        println!("    {}", style(line.trim()).italic());
    }
    println!();

    let reading_path = paths::next_attempt_path(date, "reading");
    if let Some(parent) = reading_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let reading_stats = record_exercise_with_path(&reading_path)?;

    println!();

    // --- Summary ---
    println!(
        "{}",
        style("=== Session Summary ===").bold()
    );
    println!();
    println!(
        "  {:12} {:>10} {:>10} {:>10}",
        style("Exercise").bold(),
        style("Duration").bold(),
        style("Peak dB").bold(),
        style("RMS dB").bold()
    );
    println!("  {:-<12} {:->10} {:->10} {:->10}", "", "", "", "");

    print_summary_row("Sustained", &sustained_stats);
    print_summary_row("Scale", &scale_stats);
    print_summary_row("Reading", &reading_stats);

    println!();
    let rec_dir = paths::recordings_dir().join(date.to_string());
    println!(
        "  Recordings saved to {}",
        style(rec_dir.display()).green()
    );
    println!();
    println!(
        "  Run {} to analyze this session.",
        style(format!("voicevo analyze --date {date}")).cyan()
    );

    Ok(())
}

/// Record an exercise with clear start/stop prompts.
///
/// The flow is:
///   1. "Press Enter when ready" — user prepares
///   2. *** RECORDING *** — unmistakable indicator
///   3. User presses Enter
///   4. "Stopped." — clear end
///   5. Stats displayed
fn record_exercise_with_path(
    path: &std::path::Path,
) -> Result<recorder::RecordingStats> {
    println!(
        "  Press {} when ready to record.",
        style("Enter").green().bold()
    );
    recorder::wait_for_enter()?;

    println!();
    println!(
        "  {} Press {} to stop.",
        style("*** RECORDING ***").red().bold(),
        style("Enter").bold()
    );

    let stats = recorder::record_to_file(path)?;

    println!(
        "  {}",
        style("*** STOPPED ***").dim()
    );
    println!();
    println!("  Duration: {:.1}s  |  Peak: {:.1} dB  |  RMS: {:.1} dB",
        stats.duration_secs, stats.peak_db, stats.rms_db
    );

    if stats.peak_db < -60.0 {
        eprintln!(
            "  {} Recording appears silent!",
            style("WARNING").red().bold()
        );
    }

    Ok(stats)
}

fn print_summary_row(name: &str, stats: &recorder::RecordingStats) {
    println!(
        "  {:12} {:>9.1}s {:>9.1} {:>9.1}",
        name, stats.duration_secs, stats.peak_db, stats.rms_db
    );
}
