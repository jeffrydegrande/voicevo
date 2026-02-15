mod analysis;
mod audio;
mod cli;
mod config;
mod dsp;
mod report;
mod storage;
mod util;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, RecordCommand};
use console::style;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let app_config = config::load_config()?;

    match cli.command {
        Command::Devices => audio::devices::list_devices(),

        Command::Record { exercise } => match exercise {
            RecordCommand::MicCheck => audio::mic_check::run(),

            RecordCommand::Sustained { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("sustained", &date)
            }

            RecordCommand::Scale { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("scale", &date)
            }

            RecordCommand::Reading { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("reading", &date)
            }

            RecordCommand::Session { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::session::run_guided_session(&date, &app_config)
            }
        },

        Command::Play { target, exercise } => {
            audio::playback::play(&target, exercise.as_deref())
        }

        Command::Analyze { date, all } => {
            if all {
                let dates = find_recording_dates()?;
                if dates.is_empty() {
                    println!("No recordings found in data/recordings/");
                    return Ok(());
                }
                for d in &dates {
                    analysis::analyzer::analyze_session(d, &app_config)?;
                    println!();
                }
                println!("Analyzed {} session(s).", dates.len());
                Ok(())
            } else {
                let date = date.unwrap_or_else(|| {
                    chrono::Local::now().format("%Y-%m-%d").to_string()
                });
                analysis::analyzer::analyze_session(&date, &app_config)?;
                Ok(())
            }
        }

        Command::Sessions => {
            let dates = storage::store::list_sessions()?;
            if dates.is_empty() {
                println!("No analyzed sessions found.");
                println!("Run `voice-tracker analyze --date YYYY-MM-DD` first.");
                return Ok(());
            }

            println!("{}", style("Analyzed Sessions").bold());
            println!();
            for date in &dates {
                match storage::store::load_session(date) {
                    Ok(session) => {
                        print!("  {}", style(date).cyan());
                        let mut parts = Vec::new();
                        if session.analysis.sustained.is_some() {
                            parts.push("sustained");
                        }
                        if session.analysis.scale.is_some() {
                            parts.push("scale");
                        }
                        if session.analysis.reading.is_some() {
                            parts.push("reading");
                        }
                        println!("  [{}]", parts.join(", "));
                    }
                    Err(_) => {
                        println!("  {} (corrupt)", style(date).red());
                    }
                }
            }
            println!();
            println!("{} session(s) total.", dates.len());
            Ok(())
        }

        Command::Report { last, all } => {
            let dates = storage::store::list_sessions()?;
            if dates.is_empty() {
                println!("No analyzed sessions found.");
                return Ok(());
            }

            // Select which sessions to include
            let selected: Vec<String> = if all {
                dates
            } else {
                let n = last.unwrap_or(8);
                dates.into_iter().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect()
            };

            // Load session data
            let sessions: Vec<storage::session_data::SessionData> = selected
                .iter()
                .filter_map(|d| storage::store::load_session(d).ok())
                .collect();

            if sessions.is_empty() {
                println!("No valid sessions found.");
                return Ok(());
            }

            // Generate chart PNG
            let chart_path = PathBuf::from("reports")
                .join(format!("report_{}.png", chrono::Local::now().format("%Y-%m-%d")));
            report::charts::generate_trend_chart(&sessions, &chart_path)?;
            println!(
                "Chart saved to {}",
                style(chart_path.display()).green()
            );

            // Generate markdown report
            let md = report::markdown::generate_report(&sessions, &app_config)?;
            let md_path = PathBuf::from("reports")
                .join(format!("report_{}.md", chrono::Local::now().format("%Y-%m-%d")));
            std::fs::create_dir_all("reports")?;
            std::fs::write(&md_path, &md)?;
            println!(
                "Report saved to {}",
                style(md_path.display()).green()
            );

            Ok(())
        }

        Command::Compare { baseline, current } => {
            report::compare::compare_sessions(&baseline, &current)
        }
    }
}

/// Find all dates that have recording directories.
fn find_recording_dates() -> Result<Vec<String>> {
    let dir = PathBuf::from("data").join("recordings");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut dates: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                Some(entry.file_name().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();

    dates.sort();
    Ok(dates)
}
