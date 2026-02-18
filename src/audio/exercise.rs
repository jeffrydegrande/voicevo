use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result};
use console::style;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::analysis::sustained;
use crate::config::AppConfig;
use crate::dsp::pitch::PitchConfig;
use crate::storage;
use crate::util;

/// Silence threshold in dB. RMS below this is considered silence.
const SILENCE_THRESHOLD_DB: f32 = -50.0;

/// Number of consecutive silent polls (~100ms each) before auto-stopping.
const SILENCE_POLL_COUNT: usize = 15;

/// Minimum recording duration in seconds before auto-stop can trigger.
const MIN_DURATION_SECS: f32 = 3.0;

/// Minimum duration to attempt DSP analysis.
const MIN_ANALYSIS_DURATION_SECS: f32 = 0.5;

/// Width of the volume meter bar in characters.
const METER_WIDTH: usize = 30;

/// Run the sustained phonation exercise with live feedback.
///
/// Three phases:
///   1. Setup — load reference MPT from session history, show instructions
///   2. Live recording — timer + volume meter, auto-stop on silence
///   3. Post-exercise — DSP analysis with comparison to reference
pub fn run_sustain_exercise(config: &AppConfig) -> Result<()> {
    // --- Phase 1: Setup ---
    let reference_mpt = load_reference_mpt();

    println!();
    println!(
        "{}",
        style("=== Sustained Phonation Exercise ===").bold()
    );
    println!();
    println!("  Take a deep breath, then hold {} as long and steady as you can.", style("\"AAAH\"").cyan());
    println!("  The exercise auto-stops when you go silent.");
    println!();

    if let Some(mpt) = reference_mpt {
        println!("  Your last MPT: {:.1}s — try to match or beat it.", mpt);
    } else {
        println!("  No previous sessions found — just hold as long as you can.");
    }

    println!();
    println!(
        "  Press {} when ready.",
        style("Enter").green().bold()
    );

    crate::audio::recorder::wait_for_enter()?;

    println!();
    println!(
        "  {} Hold your note!",
        style("*** GO ***").green().bold()
    );
    println!();

    // --- Phase 2: Live recording ---
    let (samples, sample_rate, duration_secs) = record_with_live_feedback(reference_mpt)?;

    println!();
    println!(
        "  {}",
        style("*** STOPPED ***").dim()
    );
    println!();

    // --- Phase 3: Post-exercise analysis ---
    let peak_db = util::peak_db(&samples);

    if duration_secs < MIN_ANALYSIS_DURATION_SECS {
        println!("  Recording too short ({:.1}s) — skipping analysis.", duration_secs);
        return Ok(());
    }

    if peak_db < -60.0 {
        println!(
            "  {} Recording appears silent (peak {:.1} dB). Check your microphone.",
            style("WARNING").red().bold(),
            peak_db
        );
        return Ok(());
    }

    let pitch_config: PitchConfig = (&config.analysis).into();
    match sustained::analyze(&samples, sample_rate, &pitch_config) {
        Ok(result) => {
            println!(
                "  {}",
                style("Results").bold()
            );
            println!();

            print_metric("MPT", &format!("{:.1}s", result.mpt_seconds), reference_mpt.map(|r| {
                format_comparison(result.mpt_seconds, r, "s", true)
            }));
            print_metric("Mean F0", &format!("{:.1} Hz", result.mean_f0_hz), None);
            print_metric("Jitter", &format!("{:.2}%", result.jitter_local_percent),
                Some(rate_jitter(result.jitter_local_percent, &config.analysis.thresholds)));
            print_metric("Shimmer", &format!("{:.2}%", result.shimmer_local_percent),
                Some(rate_shimmer(result.shimmer_local_percent, &config.analysis.thresholds)));
            print_metric("HNR", &format!("{:.1} dB", result.hnr_db),
                Some(rate_hnr(result.hnr_db, &config.analysis.thresholds)));

            println!();
        }
        Err(e) => {
            println!(
                "  {} Analysis failed: {e}",
                style("NOTE").yellow().bold()
            );
        }
    }

    Ok(())
}

/// Record audio with a live timer and volume meter display.
/// Returns (samples, sample_rate, duration_secs).
fn record_with_live_feedback(
    reference_mpt: Option<f32>,
) -> Result<(Vec<f32>, u32, f32)> {
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

    // Channel for sending audio data to writer
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    // Shared RMS value: audio callback writes, main thread reads
    let live_rms = Arc::new(AtomicU32::new(0_f32.to_bits()));

    // Stop signal
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_stream = Arc::clone(&stop);
    let rms_for_stream = Arc::clone(&live_rms);

    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !stop_for_stream.load(Ordering::Relaxed) {
                    let mono: Vec<f32> = if channels > 1 {
                        data.iter().step_by(channels).copied().collect()
                    } else {
                        data.to_vec()
                    };
                    // Compute chunk RMS and share via atomic
                    let chunk_rms = compute_rms(&mono);
                    rms_for_stream.store(chunk_rms.to_bits(), Ordering::Relaxed);
                    let _ = tx.send(mono);
                }
            },
            |err| eprintln!("Stream error: {err}"),
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                if !stop_for_stream.load(Ordering::Relaxed) {
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
                    let chunk_rms = compute_rms(&mono);
                    rms_for_stream.store(chunk_rms.to_bits(), Ordering::Relaxed);
                    let _ = tx.send(mono);
                }
            },
            |err| eprintln!("Stream error: {err}"),
            None,
        )?,
        other => anyhow::bail!("Unsupported sample format: {other:?}"),
    };

    stream.play().context("Failed to start audio stream")?;

    // Collector thread: gathers all samples from audio callback
    let collector_handle = thread::spawn(move || -> Vec<f32> {
        let mut all_samples = Vec::new();
        for chunk in rx.iter() {
            all_samples.extend(chunk);
        }
        all_samples
    });

    // Main thread: poll keyboard + render timer/meter
    crossterm::terminal::enable_raw_mode()?;

    // Print two placeholder lines for the timer and meter.
    // In raw mode \n is LF only (no carriage return), so use \r\n.
    print!("  Timer:  0.0s\r\n");
    print!("  Volume: {}\r\n", " ".repeat(METER_WIDTH));

    let start = Instant::now();
    let mut silent_polls: usize = 0;

    loop {
        // Poll for Enter keypress (100ms timeout)
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Enter {
                    break;
                }
            }
        }

        let elapsed = start.elapsed().as_secs_f32();

        // Read current RMS from audio thread
        let rms_linear = f32::from_bits(live_rms.load(Ordering::Relaxed));
        let rms_db = if rms_linear > 0.0 {
            20.0 * rms_linear.log10()
        } else {
            f32::NEG_INFINITY
        };

        // Auto-stop: silence detection after minimum duration
        if elapsed > MIN_DURATION_SECS && rms_db < SILENCE_THRESHOLD_DB {
            silent_polls += 1;
            if silent_polls >= SILENCE_POLL_COUNT {
                break;
            }
        } else {
            silent_polls = 0;
        }

        // Render: move cursor up 2 lines and redraw
        render_feedback(elapsed, rms_db, reference_mpt);
    }

    crossterm::terminal::disable_raw_mode()?;

    // Signal stop and clean up
    stop.store(true, Ordering::Relaxed);
    drop(stream);

    let all_samples = collector_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Collector thread panicked"))?;

    let duration_secs = all_samples.len() as f32 / sample_rate as f32;

    Ok((all_samples, sample_rate, duration_secs))
}

/// Render the timer and volume meter lines in-place.
fn render_feedback(elapsed: f32, rms_db: f32, reference_mpt: Option<f32>) {
    use std::io::Write;

    // Move cursor up 2 lines, then to column 0
    print!("\x1b[2A\r");

    // Timer line
    let timer_str = if let Some(target) = reference_mpt {
        format!("{:.1}s / {:.1}s target", elapsed, target)
    } else {
        format!("{:.1}s", elapsed)
    };
    print!("\x1b[2K  Timer:  {timer_str}\r\n");

    // Volume meter: map dB to bar width
    // Range: -60 dB (silent) to 0 dB (full scale)
    let bar = build_volume_bar(rms_db);
    print!("\x1b[2K  Volume: {bar}\r\n");

    let _ = std::io::stdout().flush();
}

/// Build a Unicode block-char volume bar from an RMS dB value.
fn build_volume_bar(rms_db: f32) -> String {
    // Map -60..0 dB to 0..METER_WIDTH
    let normalized = ((rms_db + 60.0) / 60.0).clamp(0.0, 1.0);
    let filled = (normalized * METER_WIDTH as f32) as usize;
    let empty = METER_WIDTH - filled;

    // Color: green for low, yellow for medium, red for hot
    let bar_chars: String = (0..filled)
        .map(|i| {
            let frac = i as f32 / METER_WIDTH as f32;
            if frac < 0.6 {
                '\u{2588}' // full block
            } else if frac < 0.85 {
                '\u{2588}'
            } else {
                '\u{2588}'
            }
        })
        .collect();

    let bar_str = if filled == 0 {
        format!("{:>width$}", "", width = METER_WIDTH)
    } else {
        // Apply color based on level
        let colored = if normalized < 0.6 {
            style(&bar_chars).green().to_string()
        } else if normalized < 0.85 {
            style(&bar_chars).yellow().to_string()
        } else {
            style(&bar_chars).red().to_string()
        };
        format!("{colored}{:>width$}", "", width = empty)
    };

    format!("[{bar_str}]")
}

/// Compute RMS of a sample buffer (linear, not dB).
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Load the most recent MPT from session history as a reference target.
fn load_reference_mpt() -> Option<f32> {
    let dates = storage::store::list_sessions().ok()?;
    // Iterate from newest to oldest
    for date in dates.iter().rev() {
        if let Ok(session) = storage::store::load_session(date) {
            if let Some(ref s) = session.analysis.sustained {
                return Some(s.mpt_seconds);
            }
        }
    }
    None
}

/// Format a comparison string (e.g., "+1.2s" or "-0.5s").
fn format_comparison(current: f32, reference: f32, unit: &str, higher_is_better: bool) -> String {
    let diff = current - reference;
    let sign = if diff >= 0.0 { "+" } else { "" };
    let color_good = diff > 0.0 && higher_is_better || diff < 0.0 && !higher_is_better;

    let text = format!("{sign}{diff:.1}{unit}");
    if color_good {
        style(text).green().to_string()
    } else if diff.abs() < 0.1 {
        style(text).dim().to_string()
    } else {
        style(text).red().to_string()
    }
}

/// Print a metric line with optional annotation.
fn print_metric(label: &str, value: &str, annotation: Option<String>) {
    match annotation {
        Some(ann) => println!("  {:12} {:>12}  {}", style(label).bold(), value, ann),
        None => println!("  {:12} {:>12}", style(label).bold(), value),
    }
}

/// Rate jitter quality.
fn rate_jitter(jitter: f32, thresholds: &crate::config::ThresholdConfig) -> String {
    if jitter < thresholds.jitter_pathological {
        style("normal").green().to_string()
    } else {
        style("elevated").yellow().to_string()
    }
}

/// Rate shimmer quality.
fn rate_shimmer(shimmer: f32, thresholds: &crate::config::ThresholdConfig) -> String {
    if shimmer < thresholds.shimmer_pathological {
        style("normal").green().to_string()
    } else {
        style("elevated").yellow().to_string()
    }
}

/// Rate HNR quality.
fn rate_hnr(hnr: f32, thresholds: &crate::config::ThresholdConfig) -> String {
    if hnr >= thresholds.hnr_normal {
        style("healthy").green().to_string()
    } else if hnr >= thresholds.hnr_low {
        style("fair").yellow().to_string()
    } else {
        style("low").red().to_string()
    }
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
        // Constant 0.5 → RMS = 0.5
        let rms = compute_rms(&[0.5, 0.5, 0.5, 0.5]);
        assert!((rms - 0.5).abs() < 0.001);
    }

    #[test]
    fn compute_rms_known_value() {
        // [1, -1] → RMS = 1.0
        let rms = compute_rms(&[1.0, -1.0]);
        assert!((rms - 1.0).abs() < 0.001);
    }

    #[test]
    fn build_volume_bar_silent() {
        let bar = build_volume_bar(f32::NEG_INFINITY);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
    }

    #[test]
    fn build_volume_bar_full_scale() {
        let bar = build_volume_bar(0.0);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
        // Should contain block characters
        assert!(bar.contains('\u{2588}'));
    }

    #[test]
    fn build_volume_bar_mid_range() {
        // -30 dB is halfway
        let bar = build_volume_bar(-30.0);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
    }

    #[test]
    fn format_comparison_positive() {
        let result = format_comparison(10.0, 8.0, "s", true);
        // Should contain "+2.0s" (with ANSI color codes)
        assert!(result.contains("2.0s"));
    }

    #[test]
    fn format_comparison_negative() {
        let result = format_comparison(6.0, 8.0, "s", true);
        assert!(result.contains("2.0s"));
    }

    #[test]
    fn format_comparison_equal() {
        let result = format_comparison(8.0, 8.0, "s", true);
        assert!(result.contains("0.0s"));
    }

    #[test]
    fn rate_jitter_normal() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_jitter(0.5, &thresholds);
        assert!(result.contains("normal"));
    }

    #[test]
    fn rate_jitter_elevated() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_jitter(2.0, &thresholds);
        assert!(result.contains("elevated"));
    }

    #[test]
    fn rate_shimmer_normal() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_shimmer(2.0, &thresholds);
        assert!(result.contains("normal"));
    }

    #[test]
    fn rate_shimmer_elevated() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_shimmer(5.0, &thresholds);
        assert!(result.contains("elevated"));
    }

    #[test]
    fn rate_hnr_healthy() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_hnr(25.0, &thresholds);
        assert!(result.contains("healthy"));
    }

    #[test]
    fn rate_hnr_fair() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_hnr(12.0, &thresholds);
        assert!(result.contains("fair"));
    }

    #[test]
    fn rate_hnr_low() {
        let thresholds = crate::config::ThresholdConfig::default();
        let result = rate_hnr(3.0, &thresholds);
        assert!(result.contains("low"));
    }

    #[test]
    fn load_reference_mpt_no_sessions() {
        // This test just verifies it doesn't panic when no sessions exist.
        // Actual value depends on disk state, so we just check it returns Some or None.
        let _ = load_reference_mpt();
    }
}
