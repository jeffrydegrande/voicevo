use std::path::PathBuf;
use std::sync::OnceLock;

/// XDG-compliant directory layout for voice-tracker.
///
/// On Linux this follows the XDG Base Directory Specification:
///   Config:  $XDG_CONFIG_HOME/voice-tracker  (~/.config/voice-tracker)
///   Data:    $XDG_DATA_HOME/voice-tracker    (~/.local/share/voice-tracker)
///
/// On macOS:
///   Config:  ~/Library/Application Support/voice-tracker
///   Data:    ~/Library/Application Support/voice-tracker
///
/// The `dirs` crate handles platform detection. We cache the resolved
/// base paths in static OnceLock cells so directory lookup only happens once.

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Root data directory: $XDG_DATA_HOME/voice-tracker
pub fn data_dir() -> &'static PathBuf {
    DATA_DIR.get_or_init(|| {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voice-tracker")
    })
}

/// Root config directory: $XDG_CONFIG_HOME/voice-tracker
pub fn config_dir() -> &'static PathBuf {
    CONFIG_DIR.get_or_init(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voice-tracker")
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

/// Path to a specific recording: <data_dir>/recordings/YYYY-MM-DD/{exercise}.wav
pub fn recording_path(date: &chrono::NaiveDate, exercise: &str) -> PathBuf {
    recordings_dir()
        .join(date.to_string())
        .join(format!("{exercise}.wav"))
}

/// Path to a session JSON file: <data_dir>/sessions/YYYY-MM-DD.json
pub fn session_path(date: &str) -> PathBuf {
    sessions_dir().join(format!("{date}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_ends_with_voice_tracker() {
        let dir = data_dir();
        assert!(dir.ends_with("voice-tracker"));
    }

    #[test]
    fn config_dir_ends_with_voice_tracker() {
        let dir = config_dir();
        assert!(dir.ends_with("voice-tracker"));
    }

    #[test]
    fn recording_path_structure() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 2, 15).unwrap();
        let path = recording_path(&date, "sustained");
        assert!(path.ends_with("recordings/2026-02-15/sustained.wav"));
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
}
