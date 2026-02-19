use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result};
use console::style;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::analysis::sz;
use crate::config::AppConfig;
use crate::storage;

/// Silence threshold in dB. RMS below this is considered silence.
const SILENCE_THRESHOLD_DB: f32 = -50.0;

/// Number of consecutive silent polls (~100ms each) before auto-stopping.
const SILENCE_POLL_COUNT: usize = 15;

/// Minimum recording duration in seconds before auto-stop can trigger.
const MIN_DURATION_SECS: f32 = 1.0;

/// Number of trials for each sound.
const TRIALS_PER_SOUND: usize = 2;

/// Run the S/Z ratio exercise.
///
/// The patient sustains /s/ (voiceless) and /z/ (voiced) multiple times.
/// We measure durations and compute the ratio.
pub fn run_sz_exercise(_config: &AppConfig) -> Result<()> {
    println!();
    println!(
        "{}",
        style("=== S/Z Ratio Test ===").bold()
    );
    println!();
    println!("  This test compares how long you can hold /s/ vs /z/.");
    println!("  A ratio close to 1.0 means healthy vocal cord function.");
    println!("  A ratio above 1.4 may indicate air leak through the vocal cords.");
    println!();

    let mut s_durations = Vec::new();
    let mut z_durations = Vec::new();

    // Record /s/ trials
    for trial in 1..=TRIALS_PER_SOUND {
        println!(
            "  {} Hold a steady {} sound as long as you can.",
            style(format!("/s/ trial {trial}/{TRIALS_PER_SOUND}:")).cyan().bold(),
            style("\"SSSSS\"").cyan(),
        );
        println!(
            "  Press {} when ready, press {} or go silent to stop.",
            style("Enter").green().bold(),
            style("Enter").green().bold(),
        );

        crate::audio::recorder::wait_for_enter()?;

        println!("  {} Hold /s/!", style("*** GO ***").green().bold());

        let duration = record_timed_sound()?;
        println!("  Duration: {:.1}s", duration);
        println!();
        s_durations.push(duration);
    }

    // Record /z/ trials
    for trial in 1..=TRIALS_PER_SOUND {
        println!(
            "  {} Hold a steady {} sound as long as you can.",
            style(format!("/z/ trial {trial}/{TRIALS_PER_SOUND}:")).cyan().bold(),
            style("\"ZZZZZ\"").cyan(),
        );
        println!(
            "  Press {} when ready, press {} or go silent to stop.",
            style("Enter").green().bold(),
            style("Enter").green().bold(),
        );

        crate::audio::recorder::wait_for_enter()?;

        println!("  {} Hold /z/!", style("*** GO ***").green().bold());

        let duration = record_timed_sound()?;
        println!("  Duration: {:.1}s", duration);
        println!();
        z_durations.push(duration);
    }

    // Compute results
    match sz::compute_sz(s_durations, z_durations) {
        Some(result) => {
            println!("{}", style("Results").bold());
            println!();
            println!("  Mean /s/: {:.1}s", result.mean_s);
            println!("  Mean /z/: {:.1}s", result.mean_z);
            println!(
                "  S/Z ratio: {:.2} {}",
                result.sz_ratio,
                if result.sz_ratio > 1.4 {
                    style("(elevated — may indicate glottal air leak)").red().to_string()
                } else {
                    style("(normal range)").green().to_string()
                }
            );

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
            session.analysis.sz = Some(result);
            storage::store::save_session(&session)?;
            println!();
            println!("  Results saved.");
        }
        None => {
            println!("  {} Insufficient data to compute S/Z ratio.", style("ERROR").red().bold());
        }
    }

    println!();
    Ok(())
}

/// Record audio until the patient goes silent or presses Enter.
/// Returns the duration of sound production in seconds.
fn record_timed_sound() -> Result<f32> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("No default input device found")?;

    let config = device
        .default_input_config()
        .context("Failed to get default input config")?;

    let _sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let format = config.sample_format();

    let live_rms = Arc::new(AtomicU32::new(0_f32.to_bits()));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_stream = Arc::clone(&stop);
    let rms_for_stream = Arc::clone(&live_rms);

    // We don't need to collect samples, just measure duration
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

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

    // Drain receiver in background
    let _collector = thread::spawn(move || {
        for _ in rx.iter() {}
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

        // Display timer
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
    // Subtract the silence detection tail (~1.5s)
    let adjusted = (duration - (SILENCE_POLL_COUNT as f32 * 0.1)).max(0.0);

    println!();
    Ok(if silent_polls >= SILENCE_POLL_COUNT {
        adjusted
    } else {
        duration
    })
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}
