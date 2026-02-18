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
use cli::{Cli, Command, ExerciseCommand, RecordCommand};
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
                audio::recorder::record_exercise("sustained", &date, &app_config)
            }

            RecordCommand::Scale { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("scale", &date, &app_config)
            }

            RecordCommand::Reading { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("reading", &date, &app_config)
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

        Command::Exercise { exercise } => match exercise {
            ExerciseCommand::Sustain => audio::exercise::run_sustain_exercise(&app_config),
        },

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

        Command::Dump => {
            let sessions = load_all_sessions()?;
            if sessions.is_empty() {
                println!("No analyzed sessions found. Run `voicevo record session` first.");
                return Ok(());
            }

            let trend_report = load_latest_report();
            let md = build_dump_markdown(&sessions, &app_config, trend_report.as_deref());

            copy_to_clipboard(&md)?;

            let kb = md.len() as f64 / 1024.0;
            println!(
                "Copied {:.1} KB of markdown to clipboard ({} session{}, ~{} tokens).",
                kb,
                sessions.len(),
                if sessions.len() == 1 { "" } else { "s" },
                md.len() / 4, // rough token estimate
            );

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

/// Load all sessions sorted chronologically.
fn load_all_sessions() -> Result<Vec<storage::session_data::SessionData>> {
    let dates = storage::store::list_sessions()?;
    Ok(dates
        .iter()
        .filter_map(|d| storage::store::load_session(d).ok())
        .collect())
}

/// Build a comprehensive markdown document with all voice recovery data,
/// formatted for easy consumption by any LLM.
fn build_dump_markdown(
    sessions: &[storage::session_data::SessionData],
    config: &config::AppConfig,
    trend_report: Option<&str>,
) -> String {
    let mut md = String::with_capacity(8192);

    // Header
    md.push_str("# Voice Recovery Data — Full Context\n\n");
    md.push_str(&format!(
        "Exported: {}  \n",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    ));
    md.push_str(&format!("Sessions: {} ({} to {})\n\n",
        sessions.len(),
        sessions.first().map(|s| s.date.as_str()).unwrap_or("?"),
        sessions.last().map(|s| s.date.as_str()).unwrap_or("?"),
    ));

    // Patient context
    md.push_str("## Background\n\n");
    md.push_str("Patient recovering from left vocal cord paralysis caused by radiation therapy. ");
    md.push_str("The left vocal fold cannot fully close during phonation, causing breathy voice, ");
    md.push_str("elevated pitch, reduced phonation time, voice breaks, and low harmonic-to-noise ratio. ");
    md.push_str("Recovery is tracked weekly through three exercises: sustained vowel, chromatic scale, ");
    md.push_str("and reading passage.\n\n");

    // Metric reference
    md.push_str("## Metric Reference\n\n");
    md.push_str("### Sustained Vowel (\"AAAH\")\n");
    md.push_str("- **MPT** (Maximum Phonation Time): healthy 15-25s, <10s = significant dysfunction\n");
    md.push_str("- **Mean F0**: fundamental frequency. Males 85-180 Hz, females 165-255 Hz\n");
    md.push_str("- **Jitter**: cycle-to-cycle pitch variation. Normal <1.04%\n");
    md.push_str("- **Shimmer**: cycle-to-cycle amplitude variation. Normal <3.81%\n");
    md.push_str("- **HNR**: harmonic-to-noise ratio. Normal >20 dB, <7 dB = severely breathy\n\n");
    md.push_str("### Chromatic Scale\n");
    md.push_str("- **Pitch floor/ceiling**: 5th-95th percentile of detected F0\n");
    md.push_str("- **Range**: healthy adults 24-36 semitones\n\n");
    md.push_str("### Reading Passage\n");
    md.push_str("- **Voice breaks**: voicing pauses 50-500ms indicating cord failure\n");
    md.push_str("- **Voiced fraction**: healthy speakers 60-80%\n\n");

    // Clinical thresholds
    let t = &config.analysis.thresholds;
    md.push_str("## Clinical Thresholds\n\n");
    md.push_str(&format!("- Jitter pathological: >{:.2}%\n", t.jitter_pathological));
    md.push_str(&format!("- Shimmer pathological: >{:.2}%\n", t.shimmer_pathological));
    md.push_str(&format!("- HNR concerning: <{:.1} dB\n", t.hnr_low));
    md.push_str(&format!("- HNR normal: >{:.1} dB\n\n", t.hnr_normal));

    md.push_str("---\n\n");

    // All session data
    md.push_str("## All Session Data\n\n");

    for session in sessions {
        md.push_str(&format!("### {}\n\n", session.date));

        if let Some(ref s) = session.analysis.sustained {
            md.push_str("**Sustained Vowel**\n");
            md.push_str(&format!("- MPT: {:.1}s\n", s.mpt_seconds));
            md.push_str(&format!("- Mean F0: {:.1} Hz (std: {:.1} Hz)\n", s.mean_f0_hz, s.f0_std_hz));
            md.push_str(&format!("- Jitter: {:.2}%{}\n",
                s.jitter_local_percent,
                if s.jitter_local_percent > t.jitter_pathological { " ⚠" } else { "" },
            ));
            md.push_str(&format!("- Shimmer: {:.2}%{}\n",
                s.shimmer_local_percent,
                if s.shimmer_local_percent > t.shimmer_pathological { " ⚠" } else { "" },
            ));
            md.push_str(&format!("- HNR: {:.1} dB{}\n",
                s.hnr_db,
                if s.hnr_db < t.hnr_low { " ⚠" } else { "" },
            ));
            md.push_str("\n");
        }

        if let Some(ref s) = session.analysis.scale {
            md.push_str("**Pitch Range (Scale)**\n");
            md.push_str(&format!("- Floor: {:.1} Hz\n", s.pitch_floor_hz));
            md.push_str(&format!("- Ceiling: {:.1} Hz\n", s.pitch_ceiling_hz));
            md.push_str(&format!("- Range: {:.1} Hz ({:.1} semitones)\n", s.range_hz, s.range_semitones));
            md.push_str("\n");
        }

        if let Some(ref s) = session.analysis.reading {
            md.push_str("**Reading Passage**\n");
            md.push_str(&format!("- Mean F0: {:.1} Hz (std: {:.1} Hz)\n", s.mean_f0_hz, s.f0_std_hz));
            md.push_str(&format!("- F0 range: {:.1}-{:.1} Hz\n", s.f0_range_hz.0, s.f0_range_hz.1));
            md.push_str(&format!("- Voice breaks: {}\n", s.voice_breaks));
            md.push_str(&format!("- Voiced fraction: {:.0}%\n", s.voiced_fraction * 100.0));
            md.push_str("\n");
        }
    }

    // Trend report
    if let Some(report) = trend_report {
        md.push_str("---\n\n");
        md.push_str("## Trend Report\n\n");
        // Strip the report's own header — it starts with "# Voice Recovery — Trend Report"
        // Include from the first "---" onward to avoid duplicate headers
        if let Some(pos) = report.find("---") {
            md.push_str(&report[pos + 4..]);
        } else {
            md.push_str(report);
        }
    }

    md
}

/// Copy text to the system clipboard. Tries wl-copy (Wayland), then xclip (X11).
fn copy_to_clipboard(text: &str) -> Result<()> {
    use std::io::Write;
    use std::process::{Command as Cmd, Stdio};

    let tools: &[&[&str]] = &[
        &["wl-copy"],
        &["xclip", "-selection", "clipboard"],
    ];

    for tool in tools {
        let name = tool[0];
        if let Ok(mut child) = Cmd::new(name)
            .args(&tool[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(text.as_bytes());
            }
            let status = child.wait()?;
            if status.success() {
                return Ok(());
            }
        }
    }

    anyhow::bail!(
        "No clipboard tool found. Install wl-copy (Wayland) or xclip (X11)."
    );
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
