use super::pitch::PitchFrame;

/// Configuration for energy-based voice activity detection.
pub struct ActivityConfig {
    /// RMS level (dB) above which a frame is considered active.
    pub threshold_on_db: f32,
    /// RMS level (dB) below which an active frame becomes silent.
    /// Lower than threshold_on to create hysteresis and prevent toggling.
    pub threshold_off_db: f32,
    /// Minimum duration (ms) for an active segment to be kept.
    pub min_active_ms: f32,
    /// Minimum duration (ms) for a silent segment to be kept.
    pub min_silent_ms: f32,
    /// Frame duration (ms). Should match pitch hop_size for 1:1 alignment.
    pub frame_size_ms: f32,
}

impl Default for ActivityConfig {
    fn default() -> Self {
        Self {
            threshold_on_db: -45.0,
            threshold_off_db: -50.0,
            min_active_ms: 80.0,
            min_silent_ms: 120.0,
            frame_size_ms: 10.0,
        }
    }
}

/// Result of voice activity detection.
pub struct ActivityResult {
    /// Per-frame activity flag, aligned with pitch contour frames.
    pub active_frames: Vec<bool>,
    /// Fraction of frames that are active (0.0 to 1.0).
    pub active_fraction: f32,
}

/// Detect voice activity using RMS energy with hysteresis.
///
/// Algorithm:
/// 1. Compute RMS (dB) per frame
/// 2. Apply hysteresis: turn on above threshold_on, turn off below threshold_off
/// 3. Post-process: remove short active bursts and short silent gaps
pub fn detect_activity(
    samples: &[f32],
    sample_rate: u32,
    config: &ActivityConfig,
) -> ActivityResult {
    let sr = sample_rate as f32;
    let frame_size = (config.frame_size_ms / 1000.0 * sr) as usize;

    if frame_size == 0 || samples.len() < frame_size {
        return ActivityResult {
            active_frames: Vec::new(),
            active_fraction: 0.0,
        };
    }

    // Step 1: Compute RMS dB per frame
    let mut rms_db_values = Vec::new();
    let mut pos = 0;
    while pos + frame_size <= samples.len() {
        let frame = &samples[pos..pos + frame_size];
        let rms = frame_rms(frame);
        let db = if rms > 0.0 {
            20.0 * rms.log10()
        } else {
            f32::NEG_INFINITY
        };
        rms_db_values.push(db);
        pos += frame_size;
    }

    // Step 2: Apply hysteresis
    let mut active_frames: Vec<bool> = Vec::with_capacity(rms_db_values.len());
    let mut is_active = false;

    for &db in &rms_db_values {
        if is_active {
            // Stay active until we drop below the off threshold
            if db < config.threshold_off_db {
                is_active = false;
            }
        } else {
            // Become active when we exceed the on threshold
            if db >= config.threshold_on_db {
                is_active = true;
            }
        }
        active_frames.push(is_active);
    }

    // Step 3: Post-process — remove short segments
    let min_active_frames = (config.min_active_ms / config.frame_size_ms).ceil() as usize;
    let min_silent_frames = (config.min_silent_ms / config.frame_size_ms).ceil() as usize;

    remove_short_segments(&mut active_frames, true, min_active_frames);
    remove_short_segments(&mut active_frames, false, min_silent_frames);

    let active_count = active_frames.iter().filter(|&&a| a).count();
    let active_fraction = if active_frames.is_empty() {
        0.0
    } else {
        active_count as f32 / active_frames.len() as f32
    };

    ActivityResult {
        active_frames,
        active_fraction,
    }
}

/// Remove runs of `target_value` shorter than `min_length` by flipping them.
fn remove_short_segments(frames: &mut [bool], target_value: bool, min_length: usize) {
    if frames.is_empty() || min_length == 0 {
        return;
    }

    let mut i = 0;
    while i < frames.len() {
        if frames[i] == target_value {
            let start = i;
            while i < frames.len() && frames[i] == target_value {
                i += 1;
            }
            let run_len = i - start;
            if run_len < min_length {
                for frame in &mut frames[start..i] {
                    *frame = !target_value;
                }
            }
        } else {
            i += 1;
        }
    }
}

/// Compute the fraction of active frames that have pitched content.
/// `voiced_quality = pitched_frames_in_active / total_active_frames`
pub fn voiced_quality(contour: &[PitchFrame], active_frames: &[bool]) -> f32 {
    let len = contour.len().min(active_frames.len());
    if len == 0 {
        return 0.0;
    }

    let mut active_count = 0;
    let mut pitched_in_active = 0;

    for i in 0..len {
        if active_frames[i] {
            active_count += 1;
            if contour[i].frequency.is_some() {
                pitched_in_active += 1;
            }
        }
    }

    if active_count == 0 {
        return 0.0;
    }

    pitched_in_active as f32 / active_count as f32
}

fn frame_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine_wave(freq_hz: f32, sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                0.5 * (2.0 * PI * freq_hz * t).sin()
            })
            .collect()
    }

    fn default_config() -> ActivityConfig {
        ActivityConfig::default()
    }

    #[test]
    fn sine_wave_fully_active() {
        let samples = sine_wave(100.0, 44100, 1.0);
        let result = detect_activity(&samples, 44100, &default_config());
        assert!(
            result.active_fraction > 0.9,
            "Sine wave should be mostly active, got {:.2}",
            result.active_fraction
        );
    }

    #[test]
    fn silence_fully_inactive() {
        let samples = vec![0.0; 44100];
        let result = detect_activity(&samples, 44100, &default_config());
        assert!(
            result.active_fraction < 0.01,
            "Silence should be inactive, got {:.2}",
            result.active_fraction
        );
    }

    #[test]
    fn signal_with_gap() {
        let sr = 44100u32;
        // 500ms tone + 200ms silence + 500ms tone
        let mut samples = sine_wave(100.0, sr, 0.5);
        samples.extend(vec![0.0; (sr as f32 * 0.2) as usize]);
        samples.extend(sine_wave(100.0, sr, 0.5));

        let result = detect_activity(&samples, sr, &default_config());

        // Should have two active segments with a gap
        let total_ms = result.active_frames.len() as f32 * 10.0;
        let active_ms = result.active_fraction * total_ms;

        // ~1000ms of signal, expect ~800-1000ms active (some ramp-up at edges)
        assert!(
            active_ms > 700.0,
            "Expected significant active time, got {active_ms:.0}ms"
        );
        assert!(
            result.active_fraction < 0.95,
            "Gap should cause some inactive frames, got {:.2}",
            result.active_fraction
        );
    }

    #[test]
    fn hysteresis_prevents_toggling() {
        let sr = 44100u32;
        // Signal hovering near threshold: alternate between -44 dB and -46 dB
        // This tests that hysteresis prevents rapid on/off toggling
        let config = ActivityConfig {
            threshold_on_db: -45.0,
            threshold_off_db: -50.0,
            ..default_config()
        };

        // Create a signal at ~-44 dB (just above on threshold)
        // amplitude for -44 dB: 10^(-44/20) ≈ 0.0063
        let amp = 0.0063;
        let samples: Vec<f32> = (0..sr)
            .map(|i| amp * (2.0 * PI * 100.0 * i as f32 / sr as f32).sin())
            .collect();

        let result = detect_activity(&samples, sr, &config);

        // Count transitions
        let transitions: usize = result
            .active_frames
            .windows(2)
            .filter(|w| w[0] != w[1])
            .count();

        // With hysteresis, there should be very few transitions (0 or 2 at most)
        assert!(
            transitions <= 4,
            "Hysteresis should prevent toggling, got {transitions} transitions"
        );
    }

    #[test]
    fn short_transient_filtered() {
        let sr = 44100u32;
        // Silence with a 50ms burst in the middle (should be filtered at min_active=80ms)
        let mut samples = vec![0.0; (sr as f32 * 0.5) as usize];
        let burst = sine_wave(100.0, sr, 0.05); // 50ms
        samples.extend(burst);
        samples.extend(vec![0.0; (sr as f32 * 0.5) as usize]);

        let result = detect_activity(&samples, sr, &default_config());
        assert!(
            result.active_fraction < 0.05,
            "50ms transient should be filtered by min_active_ms=80ms, got {:.2}",
            result.active_fraction
        );
    }

    #[test]
    fn voiced_quality_all_pitched() {
        let contour: Vec<PitchFrame> = (0..10)
            .map(|i| PitchFrame {
                time: i as f32 * 0.01,
                frequency: Some(100.0),
            })
            .collect();
        let active = vec![true; 10];
        assert!((voiced_quality(&contour, &active) - 1.0).abs() < 0.01);
    }

    #[test]
    fn voiced_quality_no_active() {
        let contour: Vec<PitchFrame> = (0..10)
            .map(|i| PitchFrame {
                time: i as f32 * 0.01,
                frequency: Some(100.0),
            })
            .collect();
        let active = vec![false; 10];
        assert!((voiced_quality(&contour, &active)).abs() < 0.01);
    }

    #[test]
    fn voiced_quality_half_pitched() {
        let contour: Vec<PitchFrame> = (0..10)
            .map(|i| PitchFrame {
                time: i as f32 * 0.01,
                frequency: if i < 5 { Some(100.0) } else { None },
            })
            .collect();
        let active = vec![true; 10];
        assert!((voiced_quality(&contour, &active) - 0.5).abs() < 0.01);
    }
}
