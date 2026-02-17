pub mod charts;
pub mod compare;
pub mod markdown;

use anyhow::Result;
use console::style;

use crate::config::AppConfig;
use crate::paths;
use crate::storage::{session_data::SessionData, store};

/// Generate the full trend report (chart PNG + markdown) from all stored sessions.
///
/// This is the shared logic used by both `voicevo report` and the guided session flow.
/// Returns the loaded sessions for further use (e.g., passing to explain).
pub fn generate_full_report(config: &AppConfig) -> Result<Vec<SessionData>> {
    let dates = store::list_sessions()?;
    if dates.is_empty() {
        println!("No analyzed sessions found.");
        return Ok(Vec::new());
    }

    let sessions: Vec<SessionData> = dates
        .iter()
        .filter_map(|d| store::load_session(d).ok())
        .collect();

    if sessions.is_empty() {
        println!("No valid sessions found.");
        return Ok(sessions);
    }

    let reports = paths::reports_dir();
    std::fs::create_dir_all(&reports)?;

    // Generate chart PNG
    let chart_path =
        reports.join(format!("report_{}.png", chrono::Local::now().format("%Y-%m-%d")));
    charts::generate_trend_chart(&sessions, &chart_path)?;
    println!(
        "Chart saved to {}",
        style(chart_path.display()).green()
    );

    // Generate markdown report
    let md = markdown::generate_report(&sessions, config)?;
    let md_path =
        reports.join(format!("report_{}.md", chrono::Local::now().format("%Y-%m-%d")));
    std::fs::write(&md_path, &md)?;
    println!(
        "Report saved to {}",
        style(md_path.display()).green()
    );

    Ok(sessions)
}
