use serde::{Deserialize, Serialize};

/// Analysis pipeline version. Bump when the DSP pipeline changes fundamentally.
/// v2: tighter bridge thresholds, gated jitter/shimmer, periodicity score,
///     CPPS, per-exercise pitch ceilings, reliability metadata.
pub const ANALYSIS_VERSION: u32 = 2;

/// Self-reported conditions at the time of recording.
/// These help the LLM distinguish genuine recovery progress from day-to-day
/// variation caused by fatigue, hydration, mucus, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingConditions {
    /// "morning", "afternoon", or "evening"
    pub time_of_day: String,
    /// 0-10 scale (0 = none, 10 = extreme)
    pub fatigue_level: u8,
    /// Whether the patient cleared their throat before recording
    pub throat_cleared: bool,
    /// "low", "moderate", or "high"
    pub mucus_level: String,
    /// "low", "normal", or "high"
    pub hydration: String,
    /// Free-text for anything else
    pub notes: Option<String>,
}

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
    /// Self-reported recording conditions. None for older sessions or CLI analyze.
    #[serde(default)]
    pub conditions: Option<RecordingConditions>,
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
    /// S/Z ratio exercise (glottal efficiency test).
    #[serde(default)]
    pub sz: Option<SzAnalysis>,
    /// Fatigue slope exercise (endurance test).
    #[serde(default)]
    pub fatigue: Option<FatigueAnalysis>,
}

/// Which metrics are trustworthy given the detection quality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsValidity {
    pub jitter: bool,
    pub shimmer: bool,
    pub hnr: bool,
    pub cpps: bool,
    /// "valid", "trend_only", or "unavailable"
    pub voice_breaks: String,
}

/// Richer reliability metadata replacing the flat detection_quality string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReliabilityInfo {
    /// Fraction of frames with energy above the activity threshold.
    pub active_fraction: f32,
    /// Fraction of active frames that have pitch (pitched / active).
    pub pitched_fraction: f32,
    /// Which detection tier dominated: 1 (standard), 2 (relaxed), 3 (energy).
    pub dominant_tier: u8,
    /// Overall quality: "good", "ok", or "trend_only".
    pub analysis_quality: String,
    /// Per-metric validity flags.
    pub metrics_validity: MetricsValidity,
}

impl ReliabilityInfo {
    /// Compute reliability from tier counts and activity data.
    pub fn compute(
        tier_counts: [usize; 3],
        active_fraction: f32,
        pitched_fraction: f32,
        has_cpps: bool,
    ) -> Self {
        let dominant_tier = if tier_counts[0] >= tier_counts[1] && tier_counts[0] >= tier_counts[2] {
            1
        } else if tier_counts[1] >= tier_counts[2] {
            2
        } else {
            3
        };

        let analysis_quality = if dominant_tier == 1 && pitched_fraction > 0.5 {
            "good"
        } else if dominant_tier <= 2 && pitched_fraction > 0.3 {
            "ok"
        } else {
            "trend_only"
        }
        .to_string();

        let metrics_validity = MetricsValidity {
            jitter: dominant_tier <= 2 && pitched_fraction > 0.3,
            shimmer: dominant_tier <= 2,
            hnr: dominant_tier <= 2,
            cpps: has_cpps,
            voice_breaks: if dominant_tier == 1 {
                "valid".to_string()
            } else if dominant_tier == 2 {
                "trend_only".to_string()
            } else {
                "unavailable".to_string()
            },
        };

        Self {
            active_fraction,
            pitched_fraction,
            dominant_tier,
            analysis_quality,
            metrics_validity,
        }
    }
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
    /// Cepstral Peak Prominence Smoothed — pitch-independent periodicity metric.
    /// Normal ~5-10 dB, < 3 dB = significant dysphonia.
    #[serde(default)]
    pub cpps_db: Option<f32>,
    /// How voiced frames were detected: "pitch" (normal), "relaxed_pitch"
    /// (lower thresholds), or "energy_fallback" (RMS-based, pitch estimated).
    /// When "energy_fallback", jitter is zeroed and shimmer/HNR use estimated pitch.
    #[serde(default)]
    pub detection_quality: Option<String>,
    /// Mean periodicity score (0.0-1.0) across voiced active frames.
    /// Based on normalized autocorrelation at the pitch period.
    #[serde(default)]
    pub periodicity_mean: Option<f32>,
    /// Rich reliability metadata. Replaces detection_quality for new analyses.
    #[serde(default)]
    pub reliability: Option<ReliabilityInfo>,
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
    /// Cepstral Peak Prominence Smoothed — pitch-independent periodicity metric.
    #[serde(default)]
    pub cpps_db: Option<f32>,
    /// How voiced frames were detected. See SustainedAnalysis::detection_quality.
    /// When "energy_fallback", voice_breaks is zeroed.
    #[serde(default)]
    pub detection_quality: Option<String>,
    /// Rich reliability metadata.
    #[serde(default)]
    pub reliability: Option<ReliabilityInfo>,
}

/// S/Z ratio analysis — glottal efficiency test.
///
/// The patient sustains /s/ (voiceless fricative) and /z/ (voiced fricative)
/// multiple times. The ratio of S duration to Z duration indicates glottal
/// competence: a ratio > 1.4 suggests air leak through the glottal gap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SzAnalysis {
    /// Durations of each /s/ trial in seconds.
    pub s_durations: Vec<f32>,
    /// Durations of each /z/ trial in seconds.
    pub z_durations: Vec<f32>,
    /// Mean /s/ duration.
    pub mean_s: f32,
    /// Mean /z/ duration.
    pub mean_z: f32,
    /// S/Z ratio. Normal ~1.0. Above 1.4 is concerning.
    pub sz_ratio: f32,
}

/// Fatigue slope analysis — vocal endurance test.
///
/// The patient performs multiple sustained vowel trials with rest periods.
/// Declining MPT or CPPS across trials indicates vocal fatigue (the cords
/// tire and can't maintain closure as long).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FatigueAnalysis {
    /// MPT for each trial in seconds.
    pub mpt_per_trial: Vec<f32>,
    /// CPPS for each trial in dB (if computable).
    pub cpps_per_trial: Vec<Option<f32>>,
    /// Subjective effort rating per trial (1-10, patient-reported).
    pub effort_per_trial: Vec<u8>,
    /// Slope of MPT across trials (negative = fatiguing).
    pub mpt_slope: f32,
    /// Slope of CPPS across trials (negative = fatiguing).
    pub cpps_slope: f32,
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
                    cpps_db: Some(6.5),
                    periodicity_mean: None,
                    detection_quality: None,
                    reliability: None,
                }),
                scale: Some(ScaleAnalysis {
                    pitch_floor_hz: 42.0,
                    pitch_ceiling_hz: 185.0,
                    range_hz: 143.0,
                    range_semitones: 25.5,
                }),
                reading: None,
                sz: None,
                fatigue: None,
            },
            conditions: None,
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

    #[test]
    fn reliability_good_quality() {
        let r = ReliabilityInfo::compute([80, 10, 10], 0.9, 0.7, true);
        assert_eq!(r.dominant_tier, 1);
        assert_eq!(r.analysis_quality, "good");
        assert!(r.metrics_validity.jitter);
        assert!(r.metrics_validity.cpps);
        assert_eq!(r.metrics_validity.voice_breaks, "valid");
    }

    #[test]
    fn reliability_ok_quality() {
        let r = ReliabilityInfo::compute([20, 60, 20], 0.8, 0.4, true);
        assert_eq!(r.dominant_tier, 2);
        assert_eq!(r.analysis_quality, "ok");
        assert!(r.metrics_validity.jitter);
        assert_eq!(r.metrics_validity.voice_breaks, "trend_only");
    }

    #[test]
    fn reliability_trend_only() {
        let r = ReliabilityInfo::compute([5, 5, 90], 0.7, 0.1, false);
        assert_eq!(r.dominant_tier, 3);
        assert_eq!(r.analysis_quality, "trend_only");
        assert!(!r.metrics_validity.jitter);
        assert!(!r.metrics_validity.cpps);
        assert_eq!(r.metrics_validity.voice_breaks, "unavailable");
    }

    #[test]
    fn reliability_backward_compat() {
        // Old JSON without reliability field should deserialize to None
        let json = r#"{"mpt_seconds":5.0,"mean_f0_hz":100.0,"f0_std_hz":2.0,
            "jitter_local_percent":1.0,"shimmer_local_percent":3.0,"hnr_db":10.0}"#;
        let s: SustainedAnalysis = serde_json::from_str(json).unwrap();
        assert!(s.reliability.is_none());
        assert!(s.cpps_db.is_none());
        assert!(s.detection_quality.is_none());
    }

    #[test]
    fn conditions_roundtrip() {
        let conditions = RecordingConditions {
            time_of_day: "morning".into(),
            fatigue_level: 7,
            throat_cleared: true,
            mucus_level: "high".into(),
            hydration: "low".into(),
            notes: Some("slept poorly".into()),
        };

        let json = serde_json::to_string(&conditions).unwrap();
        let loaded: RecordingConditions = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.time_of_day, "morning");
        assert_eq!(loaded.fatigue_level, 7);
        assert!(loaded.throat_cleared);
        assert_eq!(loaded.mucus_level, "high");
        assert_eq!(loaded.hydration, "low");
        assert_eq!(loaded.notes.as_deref(), Some("slept poorly"));
    }

    #[test]
    fn conditions_backward_compat() {
        // Old JSON without conditions field should deserialize to None
        let json = r#"{"date":"2026-01-01","recordings":{"sustained":null,"scale":null,"reading":null},"analysis":{"sustained":null,"scale":null,"reading":null}}"#;
        let s: SessionData = serde_json::from_str(json).unwrap();
        assert!(s.conditions.is_none());
    }
}
