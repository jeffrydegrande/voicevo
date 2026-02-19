use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;

/// Silence threshold in dB. RMS below this is considered silence.
pub const SILENCE_THRESHOLD_DB: f32 = -50.0;

/// Number of consecutive silent polls (~33ms each at 30fps) before auto-stopping.
pub const SILENCE_POLL_COUNT: usize = 45; // ~1.5s at 30fps

/// Minimum recording duration in seconds before auto-stop can trigger.
pub const MIN_DURATION_SECS: f32 = 3.0;

/// Size of the waveform ring buffer (recent RMS values for sparkline).
const WAVEFORM_BUFFER_SIZE: usize = 200;

/// Shared state between the audio capture thread and the TUI render loop.
pub struct AudioState {
    /// Current RMS level (linear, not dB). Updated by audio callback.
    pub live_rms: Arc<AtomicU32>,
    /// Signal to stop recording.
    pub stop: Arc<AtomicBool>,
    /// Ring buffer of recent RMS values for waveform sparkline.
    pub waveform_buffer: Arc<Mutex<VecDeque<f32>>>,
    /// Live pitch in Hz (stored as f32 bits). Only updated when pitch detection is enabled.
    pub live_pitch: Arc<AtomicU32>,
    /// The sample rate of the audio stream.
    pub sample_rate: u32,
}

impl AudioState {
    /// Read the current RMS in dB.
    pub fn rms_db(&self) -> f32 {
        let rms = f32::from_bits(self.live_rms.load(Ordering::Relaxed));
        if rms > 0.0 {
            20.0 * rms.log10()
        } else {
            f32::NEG_INFINITY
        }
    }

    /// Read the current waveform buffer snapshot.
    pub fn waveform_snapshot(&self) -> Vec<f32> {
        self.waveform_buffer
            .lock()
            .map(|buf| buf.iter().copied().collect())
            .unwrap_or_default()
    }

    /// Read the current live pitch in Hz, if available.
    pub fn pitch_hz(&self) -> Option<f32> {
        let bits = self.live_pitch.load(Ordering::Relaxed);
        if bits == 0 {
            None
        } else {
            let hz = f32::from_bits(bits);
            if hz > 0.0 { Some(hz) } else { None }
        }
    }

    /// Check if current audio is below the silence threshold.
    pub fn is_silent(&self) -> bool {
        self.rms_db() < SILENCE_THRESHOLD_DB
    }
}

/// Start audio capture.
///
/// Returns the shared state, the cpal stream (must be kept alive), and a
/// join handle for the sample collector thread.
///
/// If `enable_pitch` is true, a background thread runs per-frame pitch
/// detection and updates `AudioState::live_pitch`.
pub fn start_capture(
    enable_pitch: bool,
) -> Result<(AudioState, cpal::Stream, JoinHandle<Vec<f32>>)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("No default input device found")?;

    let config = device
        .default_input_config()
        .context("Failed to get default input config")?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let format = config.sample_format();

    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    let live_rms = Arc::new(AtomicU32::new(0_f32.to_bits()));
    let stop = Arc::new(AtomicBool::new(false));
    let waveform_buffer = Arc::new(Mutex::new(VecDeque::with_capacity(WAVEFORM_BUFFER_SIZE)));
    let live_pitch = Arc::new(AtomicU32::new(0));

    let stop_stream = Arc::clone(&stop);
    let rms_stream = Arc::clone(&live_rms);
    let waveform_stream = Arc::clone(&waveform_buffer);

    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if stop_stream.load(Ordering::Relaxed) {
                    return;
                }
                let mono: Vec<f32> = if channels > 1 {
                    data.iter().step_by(channels).copied().collect()
                } else {
                    data.to_vec()
                };
                let rms = compute_rms(&mono);
                rms_stream.store(rms.to_bits(), Ordering::Relaxed);
                // Push RMS to waveform buffer (non-blocking)
                if let Ok(mut buf) = waveform_stream.try_lock() {
                    while buf.len() >= WAVEFORM_BUFFER_SIZE {
                        buf.pop_front();
                    }
                    buf.push_back(rms);
                }
                let _ = tx.send(mono);
            },
            |err| eprintln!("Stream error: {err}"),
            None,
        )?,
        SampleFormat::I16 => {
            let stop_stream = Arc::clone(&stop);
            let rms_stream = Arc::clone(&live_rms);
            let waveform_stream = Arc::clone(&waveform_buffer);
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if stop_stream.load(Ordering::Relaxed) {
                        return;
                    }
                    let mono: Vec<f32> = if channels > 1 {
                        data.iter()
                            .step_by(channels)
                            .map(|&s| s as f32 / i16::MAX as f32)
                            .collect()
                    } else {
                        data.iter()
                            .map(|&s| s as f32 / i16::MAX as f32)
                            .collect()
                    };
                    let rms = compute_rms(&mono);
                    rms_stream.store(rms.to_bits(), Ordering::Relaxed);
                    if let Ok(mut buf) = waveform_stream.try_lock() {
                        while buf.len() >= WAVEFORM_BUFFER_SIZE {
                            buf.pop_front();
                        }
                        buf.push_back(rms);
                    }
                    let _ = tx.send(mono);
                },
                |err| eprintln!("Stream error: {err}"),
                None,
            )?
        }
        other => anyhow::bail!("Unsupported sample format: {other:?}"),
    };

    stream.play().context("Failed to start audio stream")?;

    // Pitch detection thread (optional)
    let pitch_for_thread = Arc::clone(&live_pitch);
    let stop_for_pitch = Arc::clone(&stop);

    // Collector thread: gathers all samples and optionally runs pitch detection
    let collector_handle = if enable_pitch {
        let (pitch_tx, pitch_rx) = mpsc::channel::<Vec<f32>>();

        // Collector receives from rx, forwards to pitch thread, collects samples
        let collector = std::thread::spawn(move || {
            let mut all_samples = Vec::new();
            for chunk in rx.iter() {
                let _ = pitch_tx.send(chunk.clone());
                all_samples.extend(chunk);
            }
            all_samples
        });

        // Pitch detection thread
        std::thread::spawn(move || {
            use crate::dsp::pitch;

            let mut pitch_buffer: Vec<f32> = Vec::with_capacity(4096);

            for chunk in pitch_rx.iter() {
                if stop_for_pitch.load(Ordering::Relaxed) {
                    break;
                }
                pitch_buffer.extend_from_slice(&chunk);

                // Process when we have enough samples (~1024 for 80Hz floor at 44.1kHz)
                let window_size = 2048;
                while pitch_buffer.len() >= window_size {
                    if let Some(hz) = pitch::detect_pitch_frame(
                        &pitch_buffer[..window_size],
                        sample_rate,
                    ) {
                        pitch_for_thread.store(hz.to_bits(), Ordering::Relaxed);
                    }
                    // Advance by half a window for overlap
                    let advance = window_size / 2;
                    pitch_buffer.drain(..advance);
                }
            }
        });

        collector
    } else {
        std::thread::spawn(move || {
            let mut all_samples = Vec::new();
            for chunk in rx.iter() {
                all_samples.extend(chunk);
            }
            all_samples
        })
    };

    let state = AudioState {
        live_rms,
        stop,
        waveform_buffer,
        live_pitch,
        sample_rate,
    };

    Ok((state, stream, collector_handle))
}

/// Compute RMS of a sample buffer (linear, not dB).
pub fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_rms_silence() {
        assert_eq!(compute_rms(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn compute_rms_empty() {
        assert_eq!(compute_rms(&[]), 0.0);
    }

    #[test]
    fn compute_rms_dc_signal() {
        let rms = compute_rms(&[0.5, 0.5, 0.5, 0.5]);
        assert!((rms - 0.5).abs() < 0.001);
    }

    #[test]
    fn compute_rms_known_value() {
        let rms = compute_rms(&[1.0, -1.0]);
        assert!((rms - 1.0).abs() < 0.001);
    }

    #[test]
    fn audio_state_rms_db_silence() {
        let state = AudioState {
            live_rms: Arc::new(AtomicU32::new(0_f32.to_bits())),
            stop: Arc::new(AtomicBool::new(false)),
            waveform_buffer: Arc::new(Mutex::new(VecDeque::new())),
            live_pitch: Arc::new(AtomicU32::new(0)),
            sample_rate: 44100,
        };
        assert!(state.rms_db().is_infinite());
        assert!(state.is_silent());
    }

    #[test]
    fn audio_state_rms_db_signal() {
        let rms: f32 = 0.1; // -20 dB
        let state = AudioState {
            live_rms: Arc::new(AtomicU32::new(rms.to_bits())),
            stop: Arc::new(AtomicBool::new(false)),
            waveform_buffer: Arc::new(Mutex::new(VecDeque::new())),
            live_pitch: Arc::new(AtomicU32::new(0)),
            sample_rate: 44100,
        };
        let db = state.rms_db();
        assert!((db - (-20.0)).abs() < 0.1);
        assert!(!state.is_silent());
    }

    #[test]
    fn audio_state_pitch_none_when_zero() {
        let state = AudioState {
            live_rms: Arc::new(AtomicU32::new(0_f32.to_bits())),
            stop: Arc::new(AtomicBool::new(false)),
            waveform_buffer: Arc::new(Mutex::new(VecDeque::new())),
            live_pitch: Arc::new(AtomicU32::new(0)),
            sample_rate: 44100,
        };
        assert!(state.pitch_hz().is_none());
    }

    #[test]
    fn audio_state_pitch_some_when_set() {
        let hz: f32 = 440.0;
        let state = AudioState {
            live_rms: Arc::new(AtomicU32::new(0_f32.to_bits())),
            stop: Arc::new(AtomicBool::new(false)),
            waveform_buffer: Arc::new(Mutex::new(VecDeque::new())),
            live_pitch: Arc::new(AtomicU32::new(hz.to_bits())),
            sample_rate: 44100,
        };
        assert_eq!(state.pitch_hz(), Some(440.0));
    }

    #[test]
    fn waveform_snapshot_empty() {
        let state = AudioState {
            live_rms: Arc::new(AtomicU32::new(0_f32.to_bits())),
            stop: Arc::new(AtomicBool::new(false)),
            waveform_buffer: Arc::new(Mutex::new(VecDeque::new())),
            live_pitch: Arc::new(AtomicU32::new(0)),
            sample_rate: 44100,
        };
        assert!(state.waveform_snapshot().is_empty());
    }

    #[test]
    fn waveform_snapshot_returns_data() {
        let mut buf = VecDeque::new();
        buf.push_back(0.1);
        buf.push_back(0.2);
        let state = AudioState {
            live_rms: Arc::new(AtomicU32::new(0_f32.to_bits())),
            stop: Arc::new(AtomicBool::new(false)),
            waveform_buffer: Arc::new(Mutex::new(buf)),
            live_pitch: Arc::new(AtomicU32::new(0)),
            sample_rate: 44100,
        };
        let snap = state.waveform_snapshot();
        assert_eq!(snap.len(), 2);
        assert!((snap[0] - 0.1).abs() < 0.001);
    }
}
