use std::path::PathBuf;
use std::sync::OnceLock;

/// XDG-compliant directory layout for voicevo.
///
/// On Linux this follows the XDG Base Directory Specification:
///   Config:  $XDG_CONFIG_HOME/voicevo  (~/.config/voicevo)
///   Data:    $XDG_DATA_HOME/voicevo    (~/.local/share/voicevo)
///
/// On macOS:
///   Config:  ~/Library/Application Support/voicevo
///   Data:    ~/Library/Application Support/voicevo
///
/// The `dirs` crate handles platform detection. We cache the resolved
/// base paths in static OnceLock cells so directory lookup only happens once.

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Root data directory: $XDG_DATA_HOME/voicevo
pub fn data_dir() -> &'static PathBuf {
    DATA_DIR.get_or_init(|| {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voicevo")
    })
}

/// Root config directory: $XDG_CONFIG_HOME/voicevo
pub fn config_dir() -> &'static PathBuf {
    CONFIG_DIR.get_or_init(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voicevo")
    })
}

/// Config file path: <config_dir>/config.toml
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Recordings directory: <data_dir>/recordings
pub fn recordings_dir() -> PathBuf {
    data_dir().join("recordings")
}

/// Sessions directory: <data_dir>/sessions
pub fn sessions_dir() -> PathBuf {
    data_dir().join("sessions")
}

/// Reports directory: <data_dir>/reports
pub fn reports_dir() -> PathBuf {
    data_dir().join("reports")
}

/// Database path: <data_dir>/voicevo.db
pub fn db_path() -> PathBuf {
    data_dir().join("voicevo.db")
}

/// List all attempt files for an exercise on a date, sorted ascending.
/// Includes old-format `{exercise}.wav` (as lowest priority) then numbered
/// files `{exercise}_001.wav`, `{exercise}_002.wav`, etc.
pub fn list_attempts(date: &chrono::NaiveDate, exercise: &str) -> Vec<PathBuf> {
    let dir = recordings_dir().join(date.to_string());
    list_attempts_in(&dir, exercise)
}

/// Path for the next recording attempt.
/// Returns `{exercise}_001.wav` if no attempts exist, otherwise increments
/// the highest existing attempt number.
pub fn next_attempt_path(date: &chrono::NaiveDate, exercise: &str) -> PathBuf {
    let dir = recordings_dir().join(date.to_string());
    next_attempt_in(&dir, exercise)
}

/// Path to the latest (highest-numbered) attempt, or None if no recordings exist.
pub fn latest_attempt_path(date: &chrono::NaiveDate, exercise: &str) -> Option<PathBuf> {
    list_attempts(date, exercise).into_iter().last()
}

/// Internal: list attempts within a given directory.
fn list_attempts_in(dir: &std::path::Path, exercise: &str) -> Vec<PathBuf> {
    let mut attempts = Vec::new();

    // Check old format: {exercise}.wav
    let old_path = dir.join(format!("{exercise}.wav"));
    if old_path.exists() {
        attempts.push(old_path);
    }

    // Check numbered format: {exercise}_001.wav, {exercise}_002.wav, etc.
    if let Ok(entries) = std::fs::read_dir(dir) {
        let prefix = format!("{exercise}_");
        let mut numbered: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|ext| ext == "wav")
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| {
                            s.starts_with(&prefix)
                                && s[prefix.len()..].parse::<u32>().is_ok()
                        })
            })
            .collect();
        numbered.sort();
        attempts.extend(numbered);
    }

    attempts
}

/// Internal: compute the next attempt path within a directory.
fn next_attempt_in(dir: &std::path::Path, exercise: &str) -> PathBuf {
    let attempts = list_attempts_in(dir, exercise);
    let prefix = format!("{exercise}_");
    let max_num = attempts
        .iter()
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix(&prefix))
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);

    dir.join(format!("{exercise}_{:03}.wav", max_num + 1))
}

/// Path to a session JSON file: <data_dir>/sessions/YYYY-MM-DD.json
pub fn session_path(date: &str) -> PathBuf {
    sessions_dir().join(format!("{date}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn data_dir_ends_with_voice_tracker() {
        let dir = data_dir();
        assert!(dir.ends_with("voicevo"));
    }

    #[test]
    fn config_dir_ends_with_voice_tracker() {
        let dir = config_dir();
        assert!(dir.ends_with("voicevo"));
    }

    #[test]
    fn session_path_structure() {
        let path = session_path("2026-02-15");
        assert!(path.ends_with("sessions/2026-02-15.json"));
    }

    #[test]
    fn config_file_structure() {
        let path = config_file();
        assert!(path.ends_with("config.toml"));
    }

    #[test]
    fn list_attempts_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let attempts = list_attempts_in(tmp.path(), "sustained");
        assert!(attempts.is_empty());
    }

    #[test]
    fn list_attempts_old_format_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("sustained.wav"), b"fake").unwrap();
        let attempts = list_attempts_in(tmp.path(), "sustained");
        assert_eq!(attempts.len(), 1);
        assert!(attempts[0].ends_with("sustained.wav"));
    }

    #[test]
    fn list_attempts_numbered_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("sustained_001.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("sustained_003.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("sustained_002.wav"), b"fake").unwrap();
        let attempts = list_attempts_in(tmp.path(), "sustained");
        assert_eq!(attempts.len(), 3);
        assert!(attempts[0].ends_with("sustained_001.wav"));
        assert!(attempts[1].ends_with("sustained_002.wav"));
        assert!(attempts[2].ends_with("sustained_003.wav"));
    }

    #[test]
    fn list_attempts_old_plus_numbered() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("sustained.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("sustained_001.wav"), b"fake").unwrap();
        let attempts = list_attempts_in(tmp.path(), "sustained");
        assert_eq!(attempts.len(), 2);
        assert!(attempts[0].ends_with("sustained.wav"));
        assert!(attempts[1].ends_with("sustained_001.wav"));
    }

    #[test]
    fn list_attempts_ignores_other_exercises() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("sustained_001.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("scale_001.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("reading.wav"), b"fake").unwrap();
        let attempts = list_attempts_in(tmp.path(), "sustained");
        assert_eq!(attempts.len(), 1);
    }

    #[test]
    fn next_attempt_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let path = next_attempt_in(tmp.path(), "sustained");
        assert!(path.ends_with("sustained_001.wav"));
    }

    #[test]
    fn next_attempt_after_existing() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("sustained_001.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("sustained_002.wav"), b"fake").unwrap();
        let path = next_attempt_in(tmp.path(), "sustained");
        assert!(path.ends_with("sustained_003.wav"));
    }

    #[test]
    fn next_attempt_with_old_format_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("sustained.wav"), b"fake").unwrap();
        let path = next_attempt_in(tmp.path(), "sustained");
        // Old format has no number, so max_num=0, next=001
        assert!(path.ends_with("sustained_001.wav"));
    }

    #[test]
    fn latest_attempt_returns_highest() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("sustained.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("sustained_001.wav"), b"fake").unwrap();
        fs::write(tmp.path().join("sustained_002.wav"), b"fake").unwrap();
        let latest = list_attempts_in(tmp.path(), "sustained")
            .into_iter()
            .last();
        assert!(latest.unwrap().ends_with("sustained_002.wav"));
    }
}
