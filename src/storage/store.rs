use anyhow::Result;

use super::db;
use super::session_data::SessionData;

/// Save session data to the SQLite database at the current analysis version.
pub fn save_session(session: &SessionData) -> Result<()> {
    let conn = db::open_db()?;
    db::save_session(&conn, session)
}

/// Load session data for a given date (latest version).
pub fn load_session(date: &str) -> Result<SessionData> {
    let conn = db::open_db()?;
    db::load_session(&conn, date)
}

/// Load session data for a given date at a specific analysis version.
pub fn load_session_version(date: &str, version: u32) -> Result<SessionData> {
    let conn = db::open_db()?;
    db::load_session_version(&conn, date, version)
}

/// List all session dates, sorted chronologically.
pub fn list_sessions() -> Result<Vec<String>> {
    let conn = db::open_db()?;
    db::list_sessions(&conn)
}

/// List all analysis versions available for a given date.
pub fn list_versions(date: &str) -> Result<Vec<u32>> {
    let conn = db::open_db()?;
    db::list_versions(&conn, date)
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
                    cpps_db: None,
                    periodicity_mean: None,
                    detection_quality: None,
                    reliability: None,
                }),
                scale: None,
                reading: None,
                sz: None,
                fatigue: None,
            },
            conditions: None,
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
    }

    #[test]
    fn load_nonexistent() {
        assert!(load_session("1900-01-01").is_err());
    }

    #[test]
    fn session_path_format() {
        let path = crate::paths::session_path("2026-02-08");
        assert!(path.ends_with("sessions/2026-02-08.json"));
    }
}
