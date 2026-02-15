use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;

use super::windowing;

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
            pitch_ceiling_hz: 500.0,
            frame_size_ms: 30.0,
            hop_size_ms: 10.0,
            power_threshold: 2.0,
            clarity_threshold: 0.3,
        }
    }
}

/// A single point in a pitch contour: a timestamp and an optional frequency.
/// `None` means the frame was unvoiced (no detectable pitch).
#[derive(Debug, Clone)]
pub struct PitchFrame {
    /// Time in seconds from the start of the audio.
    pub time: f32,

    /// Detected fundamental frequency, or None if unvoiced.
    pub frequency: Option<f32>,
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
}
