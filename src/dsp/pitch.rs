use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;

use super::windowing;

/// Minimum voiced fraction for the standard pitch contour to be usable.
const MIN_VOICED_FRACTION: f32 = 0.20;

/// Minimum voiced fraction after relaxed detection before falling back
/// to energy-based detection.
const MIN_VOICED_FRACTION_RELAXED: f32 = 0.10;

/// RMS threshold (dB) for the energy-based voiced frame detector.
const ENERGY_THRESHOLD_DB: f32 = -45.0;

/// Result of pitch contour extraction with fallback tiers.
pub struct ContourResult {
    /// The extracted pitch contour.
    pub contour: Vec<PitchFrame>,
    /// Which detection tier was used: "pitch", "relaxed_pitch", or "energy_fallback".
    /// When "energy_fallback", pitch values are estimated (not measured), so
    /// pitch-dependent metrics like jitter and voice break counts are unreliable.
    pub detection_quality: String,
    /// Whether the energy-based fallback (tier 3) was used.
    pub used_energy_fallback: bool,
}

/// Configuration for pitch extraction.
pub struct PitchConfig {
    /// Minimum detectable frequency in Hz.
    /// Set to 30 Hz to capture oktavist-range phonation.
    pub pitch_floor_hz: f32,

    /// Maximum detectable frequency in Hz.
    pub pitch_ceiling_hz: f32,

    /// Analysis window duration in milliseconds.
    /// 30ms gives good frequency resolution at low pitches.
    pub frame_size_ms: f32,

    /// How far to advance between frames, in milliseconds.
    /// 10ms hop = 3x overlap with 30ms windows, giving a smooth contour.
    pub hop_size_ms: f32,

    /// McLeod power threshold — filters out low-energy frames (noise).
    /// Higher = stricter. 5.0 is a reasonable starting point.
    pub power_threshold: f64,

    /// McLeod clarity threshold — how "confident" the detector must be.
    /// Range 0.0-1.0. Lower = more permissive. 0.5 works for breathy voices
    /// where clarity is naturally lower.
    pub clarity_threshold: f64,
}

impl Default for PitchConfig {
    fn default() -> Self {
        Self {
            pitch_floor_hz: 30.0,
            // 1000 Hz covers falsetto, head voice, and the elevated pitch
            // common in vocal cord paralysis recovery.
            pitch_ceiling_hz: 1000.0,
            frame_size_ms: 30.0,
            hop_size_ms: 10.0,
            // Low power threshold to handle quiet/breathy voices typical
            // in vocal cord paralysis — these signals are often 20-30 dB
            // below normal speech levels.
            power_threshold: 0.2,
            clarity_threshold: 0.2,
        }
    }
}

/// A single point in a pitch contour: a timestamp and an optional frequency.
/// `None` means the frame was unvoiced (no detectable pitch).
#[derive(Debug, Clone)]
pub struct PitchFrame {
    /// Time in seconds from the start of the audio.
    /// Used in tests and by callers inspecting the contour.
    #[allow(dead_code)]
    pub time: f32,

    /// Detected fundamental frequency, or None if unvoiced.
    pub frequency: Option<f32>,
}

/// Detect voiced frames using RMS energy instead of pitch periodicity.
///
/// Fallback for breathy voices where the pitch detector finds too few frames.
/// Active frames (RMS above `rms_threshold_db`) are assigned `estimated_f0`.
/// The contour has the same frame positions as `extract_pitch_contour` so
/// downstream code (shimmer, HNR) can use it interchangeably.
///
/// Jitter is NOT meaningful on this contour since the pitch values are
/// estimated, not measured. Shimmer and HNR still work because they only
/// use the pitch to size their analysis windows.
pub fn energy_based_contour(
    samples: &[f32],
    sample_rate: u32,
    config: &PitchConfig,
    estimated_f0: f32,
    rms_threshold_db: f32,
) -> Vec<PitchFrame> {
    let sr = sample_rate as f32;
    let frame_size = (config.frame_size_ms / 1000.0 * sr) as usize;
    let hop_size = (config.hop_size_ms / 1000.0 * sr) as usize;

    // Match the same stepping as extract_pitch_contour so frame indices
    // map to the same audio positions for shimmer/HNR.
    let min_buffer = (2.0 * sr / config.pitch_floor_hz).ceil() as usize;
    let detector_size = min_buffer.next_power_of_two().max(frame_size);

    let mut contour = Vec::new();
    let mut pos = 0;

    while pos + detector_size <= samples.len() {
        let time = pos as f32 / sr;
        let end = (pos + frame_size).min(samples.len());
        let frame = &samples[pos..end];

        let rms = frame_rms(frame);
        let rms_db = if rms > 0.0 {
            20.0 * rms.log10()
        } else {
            f32::NEG_INFINITY
        };

        let frequency = if rms_db > rms_threshold_db {
            Some(estimated_f0)
        } else {
            None
        };

        contour.push(PitchFrame { time, frequency });
        pos += hop_size;
    }

    contour
}

/// RMS of a sample buffer (linear, not dB).
fn frame_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Extract a pitch contour with three-tier fallback for breathy voices.
///
///   Tier 1: Standard pitch detection (power=0.2, clarity=0.2)
///   Tier 2: Relaxed pitch detection (power=0.01, clarity=0.05)
///   Tier 3: Energy-based frame detection with estimated F0
///
/// Returns the contour and whether the energy fallback was used.
pub fn extract_contour_with_fallback(
    samples: &[f32],
    sample_rate: u32,
    config: &PitchConfig,
) -> ContourResult {
    // Tier 1: Standard pitch detection
    let contour = extract_pitch_contour(samples, sample_rate, config);
    let voiced_frac = voiced_fraction(&contour);

    if voiced_frac >= MIN_VOICED_FRACTION {
        return ContourResult {
            contour,
            detection_quality: "pitch".into(),
            used_energy_fallback: false,
        };
    }

    // Tier 2: Retry with relaxed thresholds
    let relaxed = PitchConfig {
        pitch_floor_hz: config.pitch_floor_hz,
        pitch_ceiling_hz: config.pitch_ceiling_hz,
        frame_size_ms: config.frame_size_ms,
        hop_size_ms: config.hop_size_ms,
        power_threshold: 0.01,
        clarity_threshold: 0.05,
    };
    let relaxed_contour = extract_pitch_contour(samples, sample_rate, &relaxed);
    let relaxed_frac = voiced_fraction(&relaxed_contour);

    if relaxed_frac >= MIN_VOICED_FRACTION_RELAXED {
        return ContourResult {
            contour: relaxed_contour,
            detection_quality: "relaxed_pitch".into(),
            used_energy_fallback: false,
        };
    }

    // Tier 3: Energy-based fallback with estimated F0
    let frequencies = voiced_frequencies(&relaxed_contour);
    let estimated_f0 = if !frequencies.is_empty() {
        let mut sorted = frequencies;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted[sorted.len() / 2] // median
    } else {
        100.0 // default male speaking pitch
    };

    let energy_contour =
        energy_based_contour(samples, sample_rate, &relaxed, estimated_f0, ENERGY_THRESHOLD_DB);

    ContourResult {
        contour: energy_contour,
        detection_quality: "energy_fallback".into(),
        used_energy_fallback: true,
    }
}

/// Extract a pitch contour from audio samples.
///
/// This slides a window across the audio, runs the McLeod pitch detector on
/// each frame, and returns a sequence of (time, optional_frequency) pairs.
///
/// The McLeod Pitch Method works by computing a normalized autocorrelation
/// of the signal — essentially comparing the signal with shifted copies of
/// itself to find the period of repetition. It's robust to harmonics and
/// works well with voice.
pub fn extract_pitch_contour(
    samples: &[f32],
    sample_rate: u32,
    config: &PitchConfig,
) -> Vec<PitchFrame> {
    let sr = sample_rate as f32;

    // Convert milliseconds to samples.
    // e.g., 30ms at 44100 Hz = 1323 samples
    let frame_size = (config.frame_size_ms / 1000.0 * sr) as usize;
    let hop_size = (config.hop_size_ms / 1000.0 * sr) as usize;

    // The McLeod detector needs a buffer large enough to capture at least
    // 2 full cycles of the lowest frequency we want to detect.
    // At 30 Hz and 44100 Hz sample rate: period = 44100/30 = 1470 samples.
    // We need 2x that = 2940. Round up to next power of 2 for FFT efficiency.
    let min_buffer = (2.0 * sr / config.pitch_floor_hz).ceil() as usize;
    let detector_size = min_buffer.next_power_of_two().max(frame_size);

    // Padding helps with edge effects in the autocorrelation.
    // Half the detector size is standard.
    let padding = detector_size / 2;

    let mut contour = Vec::new();
    let mut pos = 0;

    while pos + detector_size <= samples.len() {
        let time = pos as f32 / sr;

        // Extract a full detector_size chunk of real audio and window it.
        // For low pitches (30-80 Hz), the detector needs a large buffer to
        // find the long pitch period. Using real samples (not zero-padding)
        // gives the autocorrelation actual signal to work with.
        let frame = &samples[pos..pos + detector_size];
        let windowed = windowing::hanning(frame);

        let padded: Vec<f64> = windowed.iter().map(|&s| s as f64).collect();

        // Run the pitch detector
        let mut detector = McLeodDetector::new(detector_size, padding);
        let pitch = detector.get_pitch(
            &padded,
            sample_rate as usize,
            config.power_threshold,
            config.clarity_threshold,
        );

        // Filter: only accept pitches within our expected range.
        // This rejects sub-bass rumble and high-frequency artifacts.
        let frequency = pitch
            .map(|p| p.frequency as f32)
            .filter(|&f| f >= config.pitch_floor_hz && f <= config.pitch_ceiling_hz);

        contour.push(PitchFrame { time, frequency });

        pos += hop_size;
    }

    contour
}

/// Extract only the voiced frequencies from a pitch contour.
/// Useful for computing statistics where you only care about frames
/// where pitch was actually detected.
pub fn voiced_frequencies(contour: &[PitchFrame]) -> Vec<f32> {
    contour
        .iter()
        .filter_map(|frame| frame.frequency)
        .collect()
}

/// Compute the fraction of frames that are voiced (have a detected pitch).
/// Returns 0.0 if contour is empty.
pub fn voiced_fraction(contour: &[PitchFrame]) -> f32 {
    if contour.is_empty() {
        return 0.0;
    }
    let voiced = contour.iter().filter(|f| f.frequency.is_some()).count();
    voiced as f32 / contour.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Generate a pure sine wave at a known frequency.
    /// This is our ground truth for testing pitch detection.
    fn sine_wave(freq_hz: f32, sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * PI * freq_hz * t).sin()
            })
            .collect()
    }

    #[test]
    fn detects_100hz_sine() {
        let samples = sine_wave(100.0, 44100, 0.5);
        let config = PitchConfig::default();
        let contour = extract_pitch_contour(&samples, 44100, &config);

        let frequencies = voiced_frequencies(&contour);
        assert!(
            !frequencies.is_empty(),
            "Should detect pitch in a pure sine wave"
        );

        let mean: f32 = frequencies.iter().sum::<f32>() / frequencies.len() as f32;
        assert!(
            (mean - 100.0).abs() < 5.0,
            "Mean pitch should be ~100 Hz, got {mean:.1} Hz"
        );
    }

    #[test]
    fn detects_low_pitch_50hz() {
        // This tests our 30 Hz floor — most detectors would miss this
        let samples = sine_wave(50.0, 44100, 1.0);
        let config = PitchConfig::default();
        let contour = extract_pitch_contour(&samples, 44100, &config);

        let frequencies = voiced_frequencies(&contour);
        assert!(
            !frequencies.is_empty(),
            "Should detect 50 Hz with our low floor"
        );

        let mean: f32 = frequencies.iter().sum::<f32>() / frequencies.len() as f32;
        assert!(
            (mean - 50.0).abs() < 5.0,
            "Mean pitch should be ~50 Hz, got {mean:.1} Hz"
        );
    }

    #[test]
    fn silence_is_unvoiced() {
        let samples = vec![0.0; 44100]; // 1 second of silence
        let config = PitchConfig::default();
        let contour = extract_pitch_contour(&samples, 44100, &config);

        let vf = voiced_fraction(&contour);
        assert!(
            vf < 0.1,
            "Silence should be mostly unvoiced, got {vf:.2}"
        );
    }

    #[test]
    fn voiced_fraction_empty() {
        assert_eq!(voiced_fraction(&[]), 0.0);
    }

    #[test]
    fn contour_timestamps_increase() {
        let samples = sine_wave(100.0, 44100, 0.5);
        let config = PitchConfig::default();
        let contour = extract_pitch_contour(&samples, 44100, &config);

        for pair in contour.windows(2) {
            assert!(
                pair[1].time > pair[0].time,
                "Timestamps should be strictly increasing"
            );
        }
    }

    #[test]
    fn energy_contour_detects_signal() {
        let samples = sine_wave(100.0, 44100, 0.5);
        let config = PitchConfig::default();
        let contour = energy_based_contour(&samples, 44100, &config, 100.0, -45.0);

        let vf = voiced_fraction(&contour);
        assert!(
            vf > 0.8,
            "Energy detector should find most of a sine wave, got {vf:.2}"
        );
    }

    #[test]
    fn energy_contour_rejects_silence() {
        let samples = vec![0.0; 44100];
        let config = PitchConfig::default();
        let contour = energy_based_contour(&samples, 44100, &config, 100.0, -45.0);

        let vf = voiced_fraction(&contour);
        assert!(
            vf < 0.01,
            "Energy detector should reject silence, got {vf:.2}"
        );
    }

    #[test]
    fn energy_contour_matches_pitch_contour_length() {
        let samples = sine_wave(100.0, 44100, 1.0);
        let config = PitchConfig::default();
        let pitch_contour = extract_pitch_contour(&samples, 44100, &config);
        let energy_contour = energy_based_contour(&samples, 44100, &config, 100.0, -45.0);

        assert_eq!(
            pitch_contour.len(),
            energy_contour.len(),
            "Energy and pitch contours should have the same number of frames"
        );
    }

    #[test]
    fn fallback_uses_tier1_for_clean_signal() {
        // A strong sine wave should be detected by tier 1 — no energy fallback.
        let samples = sine_wave(100.0, 44100, 1.0);
        let config = PitchConfig::default();
        let result = extract_contour_with_fallback(&samples, 44100, &config);

        assert!(
            !result.used_energy_fallback,
            "Clean sine wave should not trigger energy fallback"
        );
        let vf = voiced_fraction(&result.contour);
        assert!(vf > 0.5, "Should detect most frames, got {vf:.2}");
    }

    #[test]
    fn fallback_uses_energy_for_silence() {
        // Near-silence should exhaust tiers 1+2 and land on energy fallback.
        // With all-zero samples the energy detector also finds nothing,
        // so used_energy_fallback=true but voiced fraction is ~0.
        let samples = vec![0.0; 44100];
        let config = PitchConfig::default();
        let result = extract_contour_with_fallback(&samples, 44100, &config);

        assert!(
            result.used_energy_fallback,
            "Silence should trigger energy fallback"
        );
    }
}
