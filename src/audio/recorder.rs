use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use console::style;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::audio::wav;
use crate::paths;
use crate::util;

/// Stats returned after a recording completes.
pub struct RecordingStats {
    pub duration_secs: f32,
    pub peak_db: f32,
    pub rms_db: f32,
    pub sample_count: usize,
}

/// Record a named exercise for a given date.
/// Creates a new numbered attempt: {exercise}_001.wav, _002.wav, etc.
pub fn record_exercise(exercise: &str, date: &NaiveDate) -> Result<()> {
    let path = paths::next_attempt_path(date, exercise);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    println!(
        "{} {}",
        style("Exercise:").bold(),
        style(exercise).cyan()
    );
    println!(
        "{} {}",
        style("Date:").bold(),
        date
    );
    println!(
        "{} {}",
        style("Output:").bold(),
        path.display()
    );
    println!();
    println!("Press {} to start recording.", style("Enter").green().bold());

    wait_for_enter()?;

    println!(
        "Recording... press {} to stop.",
        style("Enter").red().bold()
    );

    let stats = record_to_file(&path)?;

    println!();
    println!(
        "  Duration:  {:.1}s",
        stats.duration_secs
    );
    println!("  Samples:   {}", stats.sample_count);
    println!("  Peak:      {:.1} dB", stats.peak_db);
    println!("  RMS:       {:.1} dB", stats.rms_db);

    if stats.peak_db < -60.0 {
        eprintln!();
        eprintln!(
            "  {} Recording appears silent. Check your microphone.",
            style("WARNING").red().bold()
        );
    }

    println!();
    println!(
        "  Saved to {}",
        style(path.display()).green()
    );

    Ok(())
}

/// Core recording function: captures from default input device and writes WAV.
///
/// Architecture:
///   cpal audio callback (runs on audio thread)
///     → sends f32 sample chunks via mpsc channel
///       → writer thread receives chunks and writes to WAV file via hound
///   AtomicBool stop signal ← main thread (crossterm Enter keypress)
pub fn record_to_file(path: &std::path::Path) -> Result<RecordingStats> {
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

    // Channel for sending audio data from cpal callback to writer thread
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    // Stop signal: set to true when user presses Enter
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_stream = Arc::clone(&stop);

    // Build the input stream. The closure captures tx by move.
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !stop_for_stream.load(Ordering::Relaxed) {
                    // Downmix to mono if multi-channel
                    let mono: Vec<f32> = if channels > 1 {
                        data.iter().step_by(channels).copied().collect()
                    } else {
                        data.to_vec()
                    };
                    let _ = tx.send(mono);
                }
            },
            |err| eprintln!("Stream error: {err}"),
            None,
        )?,
        SampleFormat::I16 => {
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !stop_for_stream.load(Ordering::Relaxed) {
                        let floats: Vec<f32> = if channels > 1 {
                            data.iter()
                                .step_by(channels)
                                .map(|&s| s as f32 / i16::MAX as f32)
                                .collect()
                        } else {
                            data.iter()
                                .map(|&s| s as f32 / i16::MAX as f32)
                                .collect()
                        };
                        let _ = tx.send(floats);
                    }
                },
                |err| eprintln!("Stream error: {err}"),
                None,
            )?
        }
        other => anyhow::bail!("Unsupported sample format: {other:?}"),
    };

    stream.play().context("Failed to start audio stream")?;

    // Writer thread: receives mono f32 chunks and writes 16-bit WAV
    let wav_path = path.to_path_buf();
    let writer_handle = thread::spawn(move || -> Result<Vec<f32>> {
        let spec = wav::recording_spec(sample_rate);
        let mut writer = wav::create_writer(&wav_path, spec)?;
        let mut all_samples = Vec::new();

        // rx.iter() blocks until the channel is closed (tx is dropped)
        for chunk in rx.iter() {
            for &sample in &chunk {
                let s16 = (sample * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                writer.write_sample(s16)?;
            }
            all_samples.extend(chunk);
        }

        writer.finalize().context("Failed to finalize WAV file")?;
        Ok(all_samples)
    });

    // Main thread: wait for Enter keypress to stop recording
    wait_for_enter()?;

    // Signal stop and clean up.
    // Setting stop causes the callback to stop sending new data.
    stop.store(true, Ordering::Relaxed);

    // Dropping the stream stops cpal from calling our callback.
    // This also drops the tx clone inside the closure, which closes the channel
    // and causes the writer thread's rx.iter() to end.
    drop(stream);

    // Wait for writer thread to finish flushing
    let all_samples = writer_handle
        .join()
        .map_err(|_| anyhow::anyhow!("Writer thread panicked"))??;

    let sample_count = all_samples.len();
    let duration_secs = sample_count as f32 / sample_rate as f32;
    let peak_db = util::peak_db(&all_samples);
    let rms_db = util::rms_db(&all_samples);

    Ok(RecordingStats {
        duration_secs,
        peak_db,
        rms_db,
        sample_count,
    })
}

/// Block until the user presses Enter, using crossterm raw mode.
pub fn wait_for_enter() -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;

    loop {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Enter {
                    break;
                }
            }
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}

/// User's choice after reviewing a recording.
pub enum PostRecordChoice {
    Keep,
    Rerecord,
}

/// Block until the user presses Enter (keep) or 'r' (re-record).
pub fn wait_for_keep_or_rerecord() -> Result<PostRecordChoice> {
    crossterm::terminal::enable_raw_mode()?;

    let choice = loop {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Enter => break PostRecordChoice::Keep,
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            break PostRecordChoice::Rerecord
                        }
                        _ => {}
                    }
                }
            }
        }
    };

    crossterm::terminal::disable_raw_mode()?;
    Ok(choice)
}
