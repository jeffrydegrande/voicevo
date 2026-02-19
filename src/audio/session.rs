use anyhow::Result;
use chrono::NaiveDate;
use console::style;

use crate::analysis;
use crate::config::AppConfig;
use crate::paths;
use crate::report;

use super::mic_check;
use super::recorder;
use super::recorder::PostRecordChoice;

/// Run a full guided recording session.
///
/// This walks the user through:
///   1. Mic check
///   2. Sustained vowel recording (with re-record option)
///   3. Chromatic scale recording (with re-record option)
///   4. Reading passage recording (with re-record option)
///   5. Analysis + full trend report
pub fn run_guided_session(date: &NaiveDate, config: &AppConfig) -> Result<()> {
    let date_str = date.to_string();

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
    println!(
        "  Take a deep breath, then hold {} as long as comfortable.",
        style("\"AAAH\"").cyan()
    );
    println!();

    let sustained_stats = record_with_retry(date, "sustained")?;

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

    let scale_stats = record_with_retry(date, "scale")?;

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

    let reading_stats = record_with_retry(date, "reading")?;

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

    // --- Conditions questionnaire ---
    let mut terminal = crate::tui::init()?;
    let conditions = crate::tui::screens::conditions::run(&mut terminal)?;
    crate::tui::restore()?;

    // --- Analyze ---
    println!(
        "{}",
        style("=== Analyzing ===").bold()
    );
    println!();
    analysis::analyzer::analyze_session_with_conditions(&date_str, config, Some(conditions))?;
    println!();

    // --- Report ---
    println!(
        "{}",
        style("=== Generating Report ===").bold()
    );
    println!();
    report::generate_full_report(config)?;

    Ok(())
}

/// Record an exercise, letting the user re-record until satisfied.
///
/// Loop: record → show stats → Enter to keep / 'r' to re-record.
/// On re-record, the previous file is deleted and a new attempt is created.
fn record_with_retry(
    date: &NaiveDate,
    exercise: &str,
) -> Result<recorder::RecordingStats> {
    loop {
        let path = paths::next_attempt_path(date, exercise);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let stats = record_exercise_with_path(&path)?;

        println!(
            "  Press {} to keep, {} to re-record.",
            style("Enter").green().bold(),
            style("r").yellow().bold(),
        );

        match recorder::wait_for_keep_or_rerecord()? {
            PostRecordChoice::Keep => {
                println!();
                return Ok(stats);
            }
            PostRecordChoice::Rerecord => {
                std::fs::remove_file(&path)?;
                println!(
                    "  {} Re-recording {}...",
                    style("DISCARDED").yellow(),
                    exercise
                );
                println!();
            }
        }
    }
}

/// Record an exercise with clear start/stop prompts.
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
    println!(
        "  Duration: {:.1}s  |  Peak: {:.1} dB  |  RMS: {:.1} dB",
        stats.duration_secs, stats.peak_db, stats.rms_db
    );

    if stats.peak_db < -60.0 {
        eprintln!(
            "  {} Recording appears silent!",
            style("WARNING").red().bold()
        );
    }

    println!();
    Ok(stats)
}

fn print_summary_row(name: &str, stats: &recorder::RecordingStats) {
    println!(
        "  {:12} {:>9.1}s {:>9.1} {:>9.1}",
        name, stats.duration_secs, stats.peak_db, stats.rms_db
    );
}

