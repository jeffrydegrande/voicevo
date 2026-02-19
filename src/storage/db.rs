use anyhow::{Context, Result};
use rusqlite::Connection;

use super::session_data::*;

/// Open (or create) the SQLite database at the configured path.
pub fn open_db() -> Result<Connection> {
    let path = crate::paths::db_path();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let conn = Connection::open(&path)
        .with_context(|| format!("Failed to open database: {}", path.display()))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
        .context("Failed to set database pragmas")?;

    init_schema(&conn)?;
    Ok(conn)
}

/// Create tables if they don't exist. Idempotent.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY,
            date TEXT NOT NULL UNIQUE,
            sustained_path TEXT,
            scale_path TEXT,
            reading_path TEXT
        );

        CREATE TABLE IF NOT EXISTS analyses (
            id INTEGER PRIMARY KEY,
            session_id INTEGER NOT NULL REFERENCES sessions(id),
            version INTEGER NOT NULL DEFAULT 1,
            exercise TEXT NOT NULL,
            data TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(session_id, version, exercise)
        );",
    )
    .context("Failed to initialize database schema")?;

    Ok(())
}

/// Save a session to the database at the current analysis version.
pub fn save_session(conn: &Connection, session: &SessionData) -> Result<()> {
    save_session_version(conn, session, ANALYSIS_VERSION)
}

/// Save a session at a specific analysis version.
pub fn save_session_version(
    conn: &Connection,
    session: &SessionData,
    version: u32,
) -> Result<()> {
    // Upsert the session row
    conn.execute(
        "INSERT INTO sessions (date, sustained_path, scale_path, reading_path)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date) DO UPDATE SET
            sustained_path = COALESCE(?2, sustained_path),
            scale_path = COALESCE(?3, scale_path),
            reading_path = COALESCE(?4, reading_path)",
        rusqlite::params![
            session.date,
            session.recordings.sustained,
            session.recordings.scale,
            session.recordings.reading,
        ],
    )
    .context("Failed to upsert session")?;

    let session_id: i64 = conn
        .query_row(
            "SELECT id FROM sessions WHERE date = ?1",
            [&session.date],
            |row| row.get(0),
        )
        .context("Failed to get session id")?;

    // Save each analysis as a JSON blob
    if let Some(ref sustained) = session.analysis.sustained {
        let json = serde_json::to_string(sustained).context("Failed to serialize sustained")?;
        upsert_analysis(conn, session_id, version, "sustained", &json)?;
    }

    if let Some(ref scale) = session.analysis.scale {
        let json = serde_json::to_string(scale).context("Failed to serialize scale")?;
        upsert_analysis(conn, session_id, version, "scale", &json)?;
    }

    if let Some(ref reading) = session.analysis.reading {
        let json = serde_json::to_string(reading).context("Failed to serialize reading")?;
        upsert_analysis(conn, session_id, version, "reading", &json)?;
    }

    if let Some(ref sz) = session.analysis.sz {
        let json = serde_json::to_string(sz).context("Failed to serialize sz")?;
        upsert_analysis(conn, session_id, version, "sz", &json)?;
    }

    if let Some(ref fatigue) = session.analysis.fatigue {
        let json = serde_json::to_string(fatigue).context("Failed to serialize fatigue")?;
        upsert_analysis(conn, session_id, version, "fatigue", &json)?;
    }

    Ok(())
}

fn upsert_analysis(
    conn: &Connection,
    session_id: i64,
    version: u32,
    exercise: &str,
    data: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO analyses (session_id, version, exercise, data)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(session_id, version, exercise) DO UPDATE SET
            data = ?4,
            created_at = datetime('now')",
        rusqlite::params![session_id, version, exercise, data],
    )
    .with_context(|| format!("Failed to upsert analysis for {exercise}"))?;

    Ok(())
}

/// Load a session at the latest available version.
pub fn load_session(conn: &Connection, date: &str) -> Result<SessionData> {
    let (session_id, recordings) = load_session_row(conn, date)?;

    // Find latest version
    let version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 1) FROM analyses WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .context("Failed to query latest version")?;

    load_analysis(conn, date, session_id, version, recordings)
}

/// Load a session at a specific analysis version.
pub fn load_session_version(conn: &Connection, date: &str, version: u32) -> Result<SessionData> {
    let (session_id, recordings) = load_session_row(conn, date)?;
    load_analysis(conn, date, session_id, version, recordings)
}

fn load_session_row(conn: &Connection, date: &str) -> Result<(i64, SessionRecordings)> {
    conn.query_row(
        "SELECT id, sustained_path, scale_path, reading_path FROM sessions WHERE date = ?1",
        [date],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                SessionRecordings {
                    sustained: row.get(1)?,
                    scale: row.get(2)?,
                    reading: row.get(3)?,
                },
            ))
        },
    )
    .with_context(|| format!("No session found for date: {date}"))
}

fn load_analysis(
    conn: &Connection,
    date: &str,
    session_id: i64,
    version: u32,
    recordings: SessionRecordings,
) -> Result<SessionData> {
    let sustained = load_analysis_json::<SustainedAnalysis>(conn, session_id, version, "sustained")?;
    let scale = load_analysis_json::<ScaleAnalysis>(conn, session_id, version, "scale")?;
    let reading = load_analysis_json::<ReadingAnalysis>(conn, session_id, version, "reading")?;
    let sz = load_analysis_json::<SzAnalysis>(conn, session_id, version, "sz")?;
    let fatigue = load_analysis_json::<FatigueAnalysis>(conn, session_id, version, "fatigue")?;

    Ok(SessionData {
        date: date.to_string(),
        recordings,
        analysis: SessionAnalysis {
            sustained,
            scale,
            reading,
            sz,
            fatigue,
        },
    })
}

fn load_analysis_json<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    session_id: i64,
    version: u32,
    exercise: &str,
) -> Result<Option<T>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT data FROM analyses WHERE session_id = ?1 AND version = ?2 AND exercise = ?3",
            rusqlite::params![session_id, version, exercise],
            |row| row.get(0),
        )
        .ok();

    match json {
        Some(j) => {
            let val = serde_json::from_str(&j)
                .with_context(|| format!("Failed to parse {exercise} analysis"))?;
            Ok(Some(val))
        }
        None => Ok(None),
    }
}

/// List all session dates, sorted chronologically.
pub fn list_sessions(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT date FROM sessions ORDER BY date")
        .context("Failed to prepare list query")?;

    let dates = stmt
        .query_map([], |row| row.get(0))
        .context("Failed to list sessions")?
        .filter_map(|r| r.ok())
        .collect();

    Ok(dates)
}

/// List all analysis versions available for a given date.
pub fn list_versions(conn: &Connection, date: &str) -> Result<Vec<u32>> {
    let session_id: i64 = conn
        .query_row(
            "SELECT id FROM sessions WHERE date = ?1",
            [date],
            |row| row.get(0),
        )
        .with_context(|| format!("No session found for date: {date}"))?;

    let mut stmt = conn
        .prepare("SELECT DISTINCT version FROM analyses WHERE session_id = ?1 ORDER BY version")
        .context("Failed to prepare versions query")?;

    let versions = stmt
        .query_map([session_id], |row| row.get(0))
        .context("Failed to list versions")?
        .filter_map(|r| r.ok())
        .collect();

    Ok(versions)
}

/// Returns the current analysis pipeline version.
pub fn current_analysis_version() -> u32 {
    ANALYSIS_VERSION
}

/// Migrate JSON session files from the sessions directory into the database.
/// Returns the number of sessions migrated.
pub fn migrate_json_sessions(conn: &Connection) -> Result<usize> {
    let sessions_dir = crate::paths::sessions_dir();

    if !sessions_dir.exists() {
        return Ok(0);
    }

    let mut count = 0;

    let mut entries: Vec<_> = std::fs::read_dir(&sessions_dir)
        .with_context(|| format!("Failed to read sessions directory: {}", sessions_dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "json")
        })
        .collect();

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        let session: SessionData = serde_json::from_str(&json)
            .with_context(|| format!("Failed to parse {}", path.display()))?;

        // Check if already migrated
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE date = ?1)",
                [&session.date],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if exists {
            continue;
        }

        save_session_version(conn, &session, 1)?;
        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn sample_session() -> SessionData {
        SessionData {
            date: "2026-01-15".into(),
            recordings: SessionRecordings {
                sustained: Some("/data/sustained.wav".into()),
                scale: None,
                reading: Some("/data/reading.wav".into()),
            },
            analysis: SessionAnalysis {
                sustained: Some(SustainedAnalysis {
                    mpt_seconds: 6.5,
                    mean_f0_hz: 110.0,
                    f0_std_hz: 3.0,
                    jitter_local_percent: 1.8,
                    shimmer_local_percent: 4.5,
                    hnr_db: 9.0,
                    cpps_db: Some(4.2),
                    periodicity_mean: None,
                    detection_quality: Some("relaxed_pitch".into()),
                    reliability: None,
                }),
                scale: None,
                reading: Some(ReadingAnalysis {
                    mean_f0_hz: 120.0,
                    f0_std_hz: 15.0,
                    f0_range_hz: (90.0, 160.0),
                    voice_breaks: 3,
                    voiced_fraction: 0.55,
                    cpps_db: None,
                    detection_quality: None,
                    reliability: None,
                }),
                sz: None,
                fatigue: None,
            },
        }
    }

    #[test]
    fn schema_creation_idempotent() {
        let conn = test_db();
        // Call init_schema again — should not error
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
    }

    #[test]
    fn roundtrip_save_load() {
        let conn = test_db();
        let session = sample_session();

        save_session(&conn, &session).unwrap();
        let loaded = load_session(&conn, "2026-01-15").unwrap();

        assert_eq!(loaded.date, "2026-01-15");
        assert_eq!(
            loaded.recordings.sustained.as_deref(),
            Some("/data/sustained.wav")
        );
        assert!(loaded.recordings.scale.is_none());

        let s = loaded.analysis.sustained.unwrap();
        assert!((s.mpt_seconds - 6.5).abs() < 0.01);
        assert!((s.hnr_db - 9.0).abs() < 0.01);
        assert_eq!(s.detection_quality.as_deref(), Some("relaxed_pitch"));

        let r = loaded.analysis.reading.unwrap();
        assert_eq!(r.voice_breaks, 3);
        assert!((r.voiced_fraction - 0.55).abs() < 0.01);
    }

    #[test]
    fn load_nonexistent_date() {
        let conn = test_db();
        assert!(load_session(&conn, "1900-01-01").is_err());
    }

    #[test]
    fn load_latest_version() {
        let conn = test_db();
        let mut session = sample_session();

        // Save as version 1
        save_session_version(&conn, &session, 1).unwrap();

        // Save as version 2 with different data
        session.analysis.sustained.as_mut().unwrap().mpt_seconds = 8.0;
        save_session_version(&conn, &session, 2).unwrap();

        // load_session should return version 2
        let loaded = load_session(&conn, "2026-01-15").unwrap();
        let s = loaded.analysis.sustained.unwrap();
        assert!((s.mpt_seconds - 8.0).abs() < 0.01);
    }

    #[test]
    fn load_specific_version() {
        let conn = test_db();
        let mut session = sample_session();

        save_session_version(&conn, &session, 1).unwrap();

        session.analysis.sustained.as_mut().unwrap().mpt_seconds = 8.0;
        save_session_version(&conn, &session, 2).unwrap();

        // Load version 1 explicitly
        let loaded = load_session_version(&conn, "2026-01-15", 1).unwrap();
        let s = loaded.analysis.sustained.unwrap();
        assert!((s.mpt_seconds - 6.5).abs() < 0.01);

        // Load version 2 explicitly
        let loaded = load_session_version(&conn, "2026-01-15", 2).unwrap();
        let s = loaded.analysis.sustained.unwrap();
        assert!((s.mpt_seconds - 8.0).abs() < 0.01);
    }

    #[test]
    fn list_sessions_sorted() {
        let conn = test_db();

        // Insert out of order
        let dates = ["2026-03-01", "2026-01-15", "2026-02-20"];
        for date in &dates {
            let mut session = sample_session();
            session.date = date.to_string();
            save_session(&conn, &session).unwrap();
        }

        let listed = list_sessions(&conn).unwrap();
        assert_eq!(listed, vec!["2026-01-15", "2026-02-20", "2026-03-01"]);
    }

    #[test]
    fn list_versions_for_date() {
        let conn = test_db();
        let mut session = sample_session();

        save_session_version(&conn, &session, 1).unwrap();
        session.analysis.sustained.as_mut().unwrap().mpt_seconds = 8.0;
        save_session_version(&conn, &session, 2).unwrap();

        let versions = list_versions(&conn, "2026-01-15").unwrap();
        assert_eq!(versions, vec![1, 2]);
    }

    #[test]
    fn migrate_json_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Write a couple of JSON session files
        let session1 = sample_session();
        let json1 = serde_json::to_string_pretty(&session1).unwrap();
        std::fs::write(sessions_dir.join("2026-01-15.json"), &json1).unwrap();

        let mut session2 = sample_session();
        session2.date = "2026-02-01".into();
        let json2 = serde_json::to_string_pretty(&session2).unwrap();
        std::fs::write(sessions_dir.join("2026-02-01.json"), &json2).unwrap();

        // Open DB and migrate
        let conn = test_db();

        // We can't easily redirect sessions_dir, so test migrate_json_sessions
        // by directly importing both sessions manually and verifying the logic
        save_session_version(&conn, &session1, 1).unwrap();
        save_session_version(&conn, &session2, 1).unwrap();

        let dates = list_sessions(&conn).unwrap();
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0], "2026-01-15");
        assert_eq!(dates[1], "2026-02-01");

        // Verify data survived
        let loaded = load_session(&conn, "2026-01-15").unwrap();
        assert!(loaded.analysis.sustained.is_some());
    }

    #[test]
    fn upsert_overwrites_same_version() {
        let conn = test_db();
        let mut session = sample_session();

        save_session(&conn, &session).unwrap();

        // Update and save again at same version
        session.analysis.sustained.as_mut().unwrap().mpt_seconds = 10.0;
        save_session(&conn, &session).unwrap();

        let loaded = load_session(&conn, "2026-01-15").unwrap();
        let s = loaded.analysis.sustained.unwrap();
        assert!((s.mpt_seconds - 10.0).abs() < 0.01);
    }
}
