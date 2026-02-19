use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result};
use console::style;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::analysis::fatigue;
use crate::config::AppConfig;
use crate::dsp::cpps;
use crate::storage;

/// Silence threshold in dB.
const SILENCE_THRESHOLD_DB: f32 = -50.0;

/// Number of consecutive silent polls (~100ms each) before auto-stopping.
const SILENCE_POLL_COUNT: usize = 15;

/// Minimum recording duration before auto-stop can trigger.
const MIN_DURATION_SECS: f32 = 3.0;

/// Number of sustained vowel trials.
const NUM_TRIALS: usize = 5;

/// Rest period between trials in seconds.
const REST_SECS: u64 = 45;

/// Run the fatigue slope exercise.
///
/// The patient performs NUM_TRIALS sustained vowel attempts with REST_SECS
/// rest between each. We measure MPT and CPPS for each trial, then compute
/// the slope to detect vocal fatigue.
pub fn run_fatigue_exercise(_config: &AppConfig) -> Result<()> {
    println!();
    println!(
        "{}",
        style("=== Vocal Fatigue Test ===").bold()
    );
    println!();
    println!("  You'll hold \"AAAH\" {} times with {}s rest between trials.", NUM_TRIALS, REST_SECS);
    println!("  This measures if your voice tires with repeated use.");
    println!();

    let mut mpt_per_trial = Vec::new();
    let mut cpps_per_trial = Vec::new();
    let mut effort_per_trial = Vec::new();

    for trial in 1..=NUM_TRIALS {
        println!(
            "  {}",
            style(format!("--- Trial {trial}/{NUM_TRIALS} ---")).cyan().bold(),
        );
        println!(
            "  Take a deep breath, then hold {} as long as you can.",
            style("\"AAAH\"").cyan(),
        );
        println!(
            "  Press {} when ready.",
            style("Enter").green().bold(),
        );

        crate::audio::recorder::wait_for_enter()?;

        println!("  {} Hold your note!", style("*** GO ***").green().bold());

        let (samples, sample_rate, duration) = record_with_timer()?;

        println!("  Duration: {:.1}s", duration);
        mpt_per_trial.push(duration);

        // Compute CPPS for this trial
        let trial_cpps = cpps::compute_cpps(&samples, sample_rate, &cpps::CppsConfig::default());
        if let Some(c) = trial_cpps {
            println!("  CPPS: {:.1} dB", c);
        }
        cpps_per_trial.push(trial_cpps);

        // Ask for effort rating
        println!();
        print!("  Effort (1=easy, 10=max strain)? ");
        crossterm::terminal::disable_raw_mode().ok();
        let effort = read_effort_rating();
        effort_per_trial.push(effort);
        println!();

        // Rest period (except after last trial)
        if trial < NUM_TRIALS {
            println!("  Rest for {}s...", REST_SECS);
            for remaining in (1..=REST_SECS).rev() {
                print!("\r  Rest: {}s   ", remaining);
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            println!("\r  Rest complete!    ");
            println!();
        }
    }

    // Compute results
    match fatigue::compute_fatigue(mpt_per_trial, cpps_per_trial, effort_per_trial) {
        Some(result) => {
            println!("{}", style("Results").bold());
            println!();

            // Show per-trial MPTs
            for (i, mpt) in result.mpt_per_trial.iter().enumerate() {
                let cpps_str = result.cpps_per_trial[i]
                    .map(|c| format!(", CPPS={c:.1}dB"))
                    .unwrap_or_default();
                println!(
                    "  Trial {}: {:.1}s{}, effort={}",
                    i + 1,
                    mpt,
                    cpps_str,
                    result.effort_per_trial[i],
                );
            }
            println!();

            // Show slopes
            let mpt_direction = if result.mpt_slope < -0.3 {
                style("declining (vocal fatigue)").red().to_string()
            } else if result.mpt_slope > 0.3 {
                style("improving (warming up)").green().to_string()
            } else {
                style("stable (good endurance)").green().to_string()
            };
            println!("  MPT slope: {:+.2}s/trial — {}", result.mpt_slope, mpt_direction);

            if result.cpps_slope != 0.0 {
                let cpps_direction = if result.cpps_slope < -0.2 {
                    "declining"
                } else if result.cpps_slope > 0.2 {
                    "improving"
                } else {
                    "stable"
                };
                println!("  CPPS slope: {:+.2}dB/trial — {}", result.cpps_slope, cpps_direction);
            }

            // Save to today's session
            let date = chrono::Local::now().format("%Y-%m-%d").to_string();
            let mut session = match storage::store::load_session(&date) {
                Ok(s) => s,
                Err(_) => crate::storage::session_data::SessionData {
                    date: date.clone(),
                    recordings: crate::storage::session_data::SessionRecordings {
                        sustained: None,
                        scale: None,
                        reading: None,
                    },
                    analysis: crate::storage::session_data::SessionAnalysis {
                        sustained: None,
                        scale: None,
                        reading: None,
                        sz: None,
                        fatigue: None,
                    },
                },
            };
            session.analysis.fatigue = Some(result);
            storage::store::save_session(&session)?;
            println!();
            println!("  Results saved.");
        }
        None => {
            println!("  {} Insufficient data to compute fatigue slope.", style("ERROR").red().bold());
        }
    }

    println!();
    Ok(())
}

/// Read an effort rating from stdin (1-10).
fn read_effort_rating() -> u8 {
    use std::io::{self, BufRead};
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        if let Ok(text) = line {
            if let Ok(n) = text.trim().parse::<u8>() {
                if (1..=10).contains(&n) {
                    return n;
                }
            }
            print!("  Please enter a number 1-10: ");
        }
    }
    5 // default if stdin closes
}

/// Record audio with a live timer, auto-stopping on silence.
/// Returns (samples, sample_rate, duration_secs).
fn record_with_timer() -> Result<(Vec<f32>, u32, f32)> {
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
                    let rms = compute_rms(&mono);
                    rms_for_stream.store(rms.to_bits(), Ordering::Relaxed);
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
                    let rms = compute_rms(&mono);
                    rms_for_stream.store(rms.to_bits(), Ordering::Relaxed);
                    let _ = tx.send(mono);
                }
            },
            |err| eprintln!("Stream error: {err}"),
            None,
        )?,
        other => anyhow::bail!("Unsupported sample format: {other:?}"),
    };

    stream.play().context("Failed to start audio stream")?;

    let collector_handle = thread::spawn(move || -> Vec<f32> {
        let mut all_samples = Vec::new();
        for chunk in rx.iter() {
            all_samples.extend(chunk);
        }
        all_samples
    });

    crossterm::terminal::enable_raw_mode()?;

    let start = Instant::now();
    let mut silent_polls: usize = 0;

    loop {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Enter {
                    break;
                }
            }
        }

        let elapsed = start.elapsed().as_secs_f32();
        let rms_linear = f32::from_bits(live_rms.load(Ordering::Relaxed));
        let rms_db = if rms_linear > 0.0 {
            20.0 * rms_linear.log10()
        } else {
            f32::NEG_INFINITY
        };

        print!("\r  Timer: {:.1}s  ", elapsed);

        if elapsed > MIN_DURATION_SECS && rms_db < SILENCE_THRESHOLD_DB {
            silent_polls += 1;
            if silent_polls >= SILENCE_POLL_COUNT {
                break;
            }
        } else {
            silent_polls = 0;
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    stop.store(true, Ordering::Relaxed);
    drop(stream);

    let duration = start.elapsed().as_secs_f32();
    let adjusted = if silent_polls >= SILENCE_POLL_COUNT {
        (duration - (SILENCE_POLL_COUNT as f32 * 0.1)).max(0.0)
    } else {
        duration
    };

    let samples = collector_handle.join().unwrap_or_default();

    println!();
    Ok((samples, sample_rate, adjusted))
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}
