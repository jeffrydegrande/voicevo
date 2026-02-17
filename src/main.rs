mod analysis;
mod audio;
mod cli;
mod config;
mod dsp;
mod llm;
mod paths;
mod report;
mod storage;
mod util;

use anyhow::{Context, Result};
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
                println!("Run `voicevo analyze --date YYYY-MM-DD` first.");
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
            if all {
                report::generate_full_report(&app_config)?;
            } else {
                let dates = storage::store::list_sessions()?;
                if dates.is_empty() {
                    println!("No analyzed sessions found.");
                    return Ok(());
                }

                let n = last.unwrap_or(8);
                let selected: Vec<String> = dates
                    .into_iter()
                    .rev()
                    .take(n)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();

                let sessions: Vec<storage::session_data::SessionData> = selected
                    .iter()
                    .filter_map(|d| storage::store::load_session(d).ok())
                    .collect();

                if sessions.is_empty() {
                    println!("No valid sessions found.");
                    return Ok(());
                }

                let reports = paths::reports_dir();
                std::fs::create_dir_all(&reports)?;

                let chart_path = reports.join(format!(
                    "report_{}.png",
                    chrono::Local::now().format("%Y-%m-%d")
                ));
                report::charts::generate_trend_chart(&sessions, &chart_path)?;
                println!("Chart saved to {}", style(chart_path.display()).green());

                let md = report::markdown::generate_report(&sessions, &app_config)?;
                let md_path = reports.join(format!(
                    "report_{}.md",
                    chrono::Local::now().format("%Y-%m-%d")
                ));
                std::fs::write(&md_path, &md)?;
                println!("Report saved to {}", style(md_path.display()).green());
            }

            Ok(())
        }

        Command::Compare { baseline, current } => {
            report::compare::compare_sessions(&baseline, &current)
        }

        Command::Explain { date, provider, model, fast, think, deep } => {
            let date = date.unwrap_or_else(|| {
                chrono::Local::now().format("%Y-%m-%d").to_string()
            });

            let tier = llm::provider::ModelTier::from_flags(fast, think);

            let current = storage::store::load_session(&date)
                .with_context(|| format!(
                    "No analyzed session for {date}. Run `voicevo analyze --date {date}` first."
                ))?;

            // Load all prior sessions as historical context
            let all_dates = storage::store::list_sessions()?;
            let history: Vec<storage::session_data::SessionData> = all_dates
                .iter()
                .filter(|d| d.as_str() < date.as_str())
                .filter_map(|d| storage::store::load_session(d).ok())
                .collect();

            // Load the latest trend report if available
            let trend_report = load_latest_report();

            if deep {
                println!(
                    "Deep analysis of session {} ({}) — querying Claude and GPT...",
                    style(&date).cyan(),
                    tier,
                );
                println!();

                let report = llm::deep_interpret(&current, &history, tier, trend_report.as_deref())?;

                println!("{}", style("--- Claude ---").blue().bold());
                println!();
                println!("{}", report.claude_response);
                println!();
                println!("{}", style("--- GPT ---").green().bold());
                println!();
                println!("{}", report.gpt_response);
                println!();
                println!("{}", style("--- Synthesis & Fact-Check ---").yellow().bold());
                println!();
                println!("{}", report.synthesis);
            } else {
                let provider = llm::provider::Provider::from_str_loose(&provider)?;
                let resolved_model = model.as_deref()
                    .unwrap_or_else(|| provider.model_for_tier(tier));

                println!(
                    "Interpreting session {} with {} ({})...",
                    style(&date).cyan(),
                    style(&provider).bold(),
                    resolved_model,
                );
                println!();

                let response = llm::interpret(
                    &provider,
                    Some(resolved_model),
                    &current,
                    &history,
                    trend_report.as_deref(),
                )?;

                println!("{response}");
            }

            Ok(())
        }

        Command::Discard { exercise, date } => {
            let date_str = date.unwrap_or_else(|| {
                chrono::Local::now().format("%Y-%m-%d").to_string()
            });
            let date_obj = util::resolve_date(Some(&date_str))?;

            let path = if let Some(ref ex) = exercise {
                paths::latest_attempt_path(&date_obj, ex)
            } else {
                // No exercise specified: find most recently modified WAV across all exercises
                ["sustained", "scale", "reading"]
                    .iter()
                    .filter_map(|ex| paths::latest_attempt_path(&date_obj, ex))
                    .filter_map(|p| {
                        std::fs::metadata(&p)
                            .and_then(|m| m.modified())
                            .ok()
                            .map(|t| (p, t))
                    })
                    .max_by_key(|(_, t)| *t)
                    .map(|(p, _)| p)
            };

            match path {
                Some(p) => {
                    std::fs::remove_file(&p)?;
                    println!("Discarded {}", style(p.display()).red());
                    println!();
                    println!(
                        "Re-record with {}, then {}.",
                        style("voicevo record <exercise>").cyan(),
                        style("voicevo analyze").cyan()
                    );
                }
                None => {
                    let target = exercise.as_deref().unwrap_or("any exercise");
                    println!(
                        "No recordings found for {} on {}.",
                        style(target).cyan(),
                        style(&date_str).cyan()
                    );
                }
            }

            Ok(())
        }

        Command::Browse => {
            let reports = paths::reports_dir();
            if !reports.exists() {
                anyhow::bail!(
                    "No reports found. Run `voicevo report --all` first."
                );
            }

            // Find the most recent PNG report
            let mut pngs: Vec<_> = std::fs::read_dir(&reports)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|ext| ext == "png")
                })
                .collect();

            if pngs.is_empty() {
                anyhow::bail!(
                    "No chart files found in {}. Run `voicevo report --all` first.",
                    reports.display()
                );
            }

            // Sort by name (report_YYYY-MM-DD.png) so last = newest
            pngs.sort_by_key(|e| e.file_name());
            let latest = pngs.last().unwrap().path();

            println!("Opening {}", style(latest.display()).green());
            std::process::Command::new("xdg-open")
                .arg(&latest)
                .spawn()
                .context("Failed to run xdg-open. Install xdg-utils or open the file manually.")?;

            Ok(())
        }

        Command::Paths => {
            println!("{}", style("voicevo paths").bold());
            println!();
            println!("  Config:     {}", style(paths::config_dir().display()).cyan());
            println!("  Data:       {}", style(paths::data_dir().display()).cyan());
            println!("  Recordings: {}", style(paths::recordings_dir().display()).cyan());
            println!("  Sessions:   {}", style(paths::sessions_dir().display()).cyan());
            println!("  Reports:    {}", style(paths::reports_dir().display()).cyan());
            Ok(())
        }
    }
}

/// Load the latest markdown trend report, if any exist.
fn load_latest_report() -> Option<String> {
    let reports = paths::reports_dir();
    let mut mds: Vec<_> = std::fs::read_dir(&reports)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "md")
        })
        .collect();

    if mds.is_empty() {
        return None;
    }

    mds.sort_by_key(|e| e.file_name());
    let latest = mds.last().unwrap().path();
    std::fs::read_to_string(latest).ok()
}

/// Find all dates that have recording directories.
fn find_recording_dates() -> Result<Vec<String>> {
    let dir = paths::recordings_dir();
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
