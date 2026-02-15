use serde::{Deserialize, Serialize};

/// Complete session data for one recording date.
///
/// The `#[derive(Serialize, Deserialize)]` macro auto-generates code
/// to convert this struct to/from JSON. serde inspects each field's type
/// and handles everything — Strings become JSON strings, f32 becomes numbers,
/// Option<T> becomes null or the value, Vec<T> becomes arrays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub date: String,
    pub recordings: SessionRecordings,
    pub analysis: SessionAnalysis,
}

/// Paths to the WAV files for each exercise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecordings {
    pub sustained: Option<String>,
    pub scale: Option<String>,
    pub reading: Option<String>,
}

/// Analysis results for all exercises.
/// Each field is Option because not every exercise may have been recorded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAnalysis {
    pub sustained: Option<SustainedAnalysis>,
    pub scale: Option<ScaleAnalysis>,
    pub reading: Option<ReadingAnalysis>,
}

/// Analysis of the sustained vowel ("AAAH") recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SustainedAnalysis {
    /// Maximum phonation time in seconds
    pub mpt_seconds: f32,
    /// Mean fundamental frequency over voiced frames
    pub mean_f0_hz: f32,
    /// Standard deviation of F0
    pub f0_std_hz: f32,
    /// Local jitter as a percentage
    pub jitter_local_percent: f32,
    /// Local shimmer as a percentage
    pub shimmer_local_percent: f32,
    /// Harmonic-to-noise ratio in decibels
    pub hnr_db: f32,
}

/// Analysis of the chromatic scale recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleAnalysis {
    /// 5th percentile of detected F0 (effective lowest note)
    pub pitch_floor_hz: f32,
    /// 95th percentile of detected F0 (effective highest note)
    pub pitch_ceiling_hz: f32,
    /// Range in Hz (ceiling - floor)
    pub range_hz: f32,
    /// Range in semitones: 12 * log2(ceiling / floor)
    pub range_semitones: f32,
}

/// Analysis of the reading passage recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingAnalysis {
    /// Mean F0 over voiced frames
    pub mean_f0_hz: f32,
    /// Standard deviation of F0
    pub f0_std_hz: f32,
    /// F0 range as [5th percentile, 95th percentile]
    pub f0_range_hz: (f32, f32),
    /// Number of voice breaks detected
    pub voice_breaks: usize,
    /// Fraction of frames that are voiced (0.0 to 1.0)
    pub voiced_fraction: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_data_roundtrip() {
        let session = SessionData {
            date: "2026-02-08".into(),
            recordings: SessionRecordings {
                sustained: Some("data/recordings/2026-02-08/sustained.wav".into()),
                scale: Some("data/recordings/2026-02-08/scale.wav".into()),
                reading: None,
            },
            analysis: SessionAnalysis {
                sustained: Some(SustainedAnalysis {
                    mpt_seconds: 8.3,
                    mean_f0_hz: 112.4,
                    f0_std_hz: 3.2,
                    jitter_local_percent: 2.1,
                    shimmer_local_percent: 5.8,
                    hnr_db: 12.3,
                }),
                scale: Some(ScaleAnalysis {
                    pitch_floor_hz: 42.0,
                    pitch_ceiling_hz: 185.0,
                    range_hz: 143.0,
                    range_semitones: 25.5,
                }),
                reading: None,
            },
        };

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&session).unwrap();

        // Deserialize back
        let loaded: SessionData = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.date, "2026-02-08");
        assert!(loaded.recordings.reading.is_none());
        assert!(loaded.analysis.reading.is_none());

        let sustained = loaded.analysis.sustained.unwrap();
        assert!((sustained.mpt_seconds - 8.3).abs() < 0.01);
        assert!((sustained.hnr_db - 12.3).abs() < 0.01);
    }
}
