use std::path::Path;

use anyhow::{Context, Result};

use super::session_data::SessionData;
use crate::paths;

/// Save session data to a JSON file.
pub fn save_session(session: &SessionData) -> Result<()> {
    let path = paths::session_path(&session.date);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(session)
        .context("Failed to serialize session data")?;

    std::fs::write(&path, json)
        .with_context(|| format!("Failed to write session file: {}", path.display()))?;

    Ok(())
}

/// Load session data from a JSON file for a given date.
pub fn load_session(date: &str) -> Result<SessionData> {
    let path = paths::session_path(date);
    load_session_from_path(&path)
}

/// Load session data from a specific path.
fn load_session_from_path(path: &Path) -> Result<SessionData> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read session file: {}", path.display()))?;

    serde_json::from_str(&json)
        .with_context(|| format!("Failed to parse session file: {}", path.display()))
}

/// List all session dates, sorted chronologically.
/// Scans the sessions directory for .json files.
pub fn list_sessions() -> Result<Vec<String>> {
    let dir = paths::sessions_dir();

    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut dates: Vec<String> = std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read sessions directory: {}", dir.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            // Strip .json extension to get the date
            name.strip_suffix(".json").map(|s| s.to_string())
        })
        .collect();

    dates.sort();
    Ok(dates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::session_data::*;

    fn temp_session() -> SessionData {
        SessionData {
            date: "2099-01-01".into(),
            recordings: SessionRecordings {
                sustained: Some("test.wav".into()),
                scale: None,
                reading: None,
            },
            analysis: SessionAnalysis {
                sustained: Some(SustainedAnalysis {
                    mpt_seconds: 5.0,
                    mean_f0_hz: 100.0,
                    f0_std_hz: 2.0,
                    jitter_local_percent: 1.5,
                    shimmer_local_percent: 4.0,
                    hnr_db: 10.0,
                }),
                scale: None,
                reading: None,
            },
        }
    }

    #[test]
    fn save_and_load_session() {
        let session = temp_session();
        save_session(&session).unwrap();

        let loaded = load_session("2099-01-01").unwrap();
        assert_eq!(loaded.date, "2099-01-01");

        let sustained = loaded.analysis.sustained.unwrap();
        assert!((sustained.mpt_seconds - 5.0).abs() < 0.01);

        // Cleanup
        let _ = std::fs::remove_file(paths::session_path("2099-01-01"));
    }

    #[test]
    fn load_nonexistent() {
        assert!(load_session("1900-01-01").is_err());
    }

    #[test]
    fn session_path_format() {
        let path = paths::session_path("2026-02-08");
        assert!(path.ends_with("sessions/2026-02-08.json"));
    }
}
