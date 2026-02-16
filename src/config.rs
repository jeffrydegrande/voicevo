use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dsp::pitch::PitchConfig;
use crate::paths;

/// Application configuration, loaded from data/config.toml.
///
/// serde's `default` attribute means: if a field is missing from the TOML file,
/// use the value from the Default implementation instead of failing to parse.
/// This makes the config file optional — every field has a sensible default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub recording: RecordingConfig,
    pub analysis: AnalysisConfig,
    pub session: SessionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub device: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    pub pitch_floor_hz: f32,
    pub pitch_ceiling_hz: f32,
    pub frame_size_ms: f32,
    pub hop_size_ms: f32,
    pub thresholds: ThresholdConfig,
}

/// Clinical thresholds for voice quality metrics.
/// These come from Praat's standard values (Boersma & Weenink).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThresholdConfig {
    /// Jitter above this is considered pathological (percent)
    pub jitter_pathological: f32,
    /// Shimmer above this is considered pathological (percent)
    pub shimmer_pathological: f32,
    /// HNR below this is concerning (dB)
    pub hnr_low: f32,
    /// HNR above this is healthy (dB)
    pub hnr_normal: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub reading_passage: String,
}

// --- Default implementations ---
// Each of these defines the "factory settings" for the application.

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            recording: RecordingConfig::default(),
            analysis: AnalysisConfig::default(),
            session: SessionConfig::default(),
        }
    }
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44100,
            channels: 1,
            device: "default".into(),
        }
    }
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            pitch_floor_hz: 30.0,
            pitch_ceiling_hz: 1000.0,
            frame_size_ms: 30.0,
            hop_size_ms: 10.0,
            thresholds: ThresholdConfig::default(),
        }
    }
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            jitter_pathological: 1.04,
            shimmer_pathological: 3.81,
            hnr_low: 7.0,
            hnr_normal: 20.0,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            reading_passage: "\
When the sunlight strikes raindrops in the air, they act as a prism \
and form a rainbow. The rainbow is a division of white light into \
many beautiful colors. These take the shape of a long round arch, \
with its path high above, and its two ends apparently beyond the horizon."
                .into(),
        }
    }
}

/// Convert our config into the PitchConfig that the DSP code expects.
/// This is a bridge between the user-facing config format and the internal
/// DSP parameters.
impl From<&AnalysisConfig> for PitchConfig {
    fn from(cfg: &AnalysisConfig) -> Self {
        PitchConfig {
            pitch_floor_hz: cfg.pitch_floor_hz,
            pitch_ceiling_hz: cfg.pitch_ceiling_hz,
            frame_size_ms: cfg.frame_size_ms,
            hop_size_ms: cfg.hop_size_ms,
            ..PitchConfig::default()
        }
    }
}

/// Load the application config from $XDG_CONFIG_HOME/voicevo/config.toml.
/// If the file doesn't exist, returns defaults.
pub fn load_config() -> Result<AppConfig> {
    let path = paths::config_file();

    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    toml::from_str(&contents)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.analysis.pitch_floor_hz, 30.0);
        assert_eq!(cfg.analysis.thresholds.jitter_pathological, 1.04);
        assert!(!cfg.session.reading_passage.is_empty());
    }

    #[test]
    fn parse_partial_toml() {
        // If the user only specifies some fields, the rest should use defaults
        let toml_str = r#"
[analysis]
pitch_floor_hz = 40.0
"#;
        let cfg: AppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.analysis.pitch_floor_hz, 40.0);
        // Unspecified fields should be defaults
        assert_eq!(cfg.analysis.pitch_ceiling_hz, 1000.0);
        assert_eq!(cfg.recording.sample_rate, 44100);
    }

    #[test]
    fn pitch_config_conversion() {
        let cfg = AnalysisConfig::default();
        let pitch_cfg: PitchConfig = (&cfg).into();
        assert_eq!(pitch_cfg.pitch_floor_hz, 30.0);
        assert_eq!(pitch_cfg.hop_size_ms, 10.0);
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = AppConfig::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let loaded: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(loaded.analysis.pitch_floor_hz, cfg.analysis.pitch_floor_hz);
    }
}
