use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use console::style;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use indicatif::{ProgressBar, ProgressStyle};

use crate::util;

use super::recorder;

const CAPTURE_SECONDS: u64 = 2;

/// Run a quick 2-second mic check: capture audio and report peak/RMS levels.
///
/// Waits for the user to press Enter before capturing, so they know exactly
/// when the mic is live. Shows the device name prominently so there's no
/// ambiguity about which input is being used.
pub fn run() -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("No default input device found")?;

    let device_name = device.name().unwrap_or_else(|_| "<unknown>".into());
    let config = device
        .default_input_config()
        .context("Failed to get default input config")?;

    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let format = config.sample_format();

    println!(
        "  Device:  {}",
        style(&device_name).cyan().bold()
    );
    println!("  Config:  {channels}ch, {sample_rate} Hz, {format:?}");
    println!();
    println!(
        "  Press {} to capture a 2-second sample.",
        style("Enter").green().bold()
    );

    recorder::wait_for_enter()?;

    println!();

    // Channel to send captured samples from audio thread to main thread
    let (tx, rx) = mpsc::channel::<Vec<f32>>();

    // Build input stream based on sample format.
    // PipeWire typically uses F32, ALSA often uses I16.
    let stream = match format {
        SampleFormat::F32 => {
            let tx = tx.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let _ = tx.send(data.to_vec());
                },
                |err| eprintln!("Stream error: {err}"),
                None,
            )?
        }
        SampleFormat::I16 => {
            let tx = tx.clone();
            device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    // Convert i16 samples to f32 [-1.0, 1.0]
                    let floats: Vec<f32> =
                        data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let _ = tx.send(floats);
                },
                |err| eprintln!("Stream error: {err}"),
                None,
            )?
        }
        other => anyhow::bail!("Unsupported sample format: {other:?}"),
    };

    // Drop our copy of tx so rx will close when the stream's copy is dropped
    drop(tx);

    stream.play().context("Failed to start audio stream")?;

    // Show progress bar during capture
    let pb = ProgressBar::new(CAPTURE_SECONDS * 10);
    pb.set_style(
        ProgressStyle::with_template("  Listening {bar:30.green/dim} {elapsed_precise}")
            .unwrap(),
    );

    let capture_duration = Duration::from_secs(CAPTURE_SECONDS);
    let tick = Duration::from_millis(100);
    let start = std::time::Instant::now();

    let mut all_samples = Vec::new();

    while start.elapsed() < capture_duration {
        // Drain available audio chunks
        while let Ok(chunk) = rx.try_recv() {
            all_samples.extend(chunk);
        }
        std::thread::sleep(tick);
        pb.set_position((start.elapsed().as_millis() / 100) as u64);
    }

    // Stop the stream and collect remaining samples
    drop(stream);
    while let Ok(chunk) = rx.try_recv() {
        all_samples.extend(chunk);
    }

    pb.finish_and_clear();

    // Downmix stereo (or multi-channel) to mono by taking every Nth sample
    let mono: Vec<f32> = if channels > 1 {
        all_samples.iter().step_by(channels).copied().collect()
    } else {
        all_samples
    };

    if mono.is_empty() {
        eprintln!(
            "  {} No samples captured. Check your microphone connection.",
            style("WARNING").red().bold()
        );
        return Ok(());
    }

    let peak = util::peak_db(&mono);
    let rms = util::rms_db(&mono);

    println!("  Peak level:  {peak:.1} dB");
    println!("  RMS level:   {rms:.1} dB");
    println!();

    if peak < -60.0 {
        eprintln!(
            "  {} Peak is below -60 dB — mic may be muted or disconnected.",
            style("WARNING").red().bold()
        );
        eprintln!("  Run `voicevo devices` to check available inputs.");
    } else if peak < -30.0 {
        println!(
            "  {} Signal detected but quiet. Consider increasing mic gain.",
            style("NOTE").yellow().bold()
        );
    } else {
        println!(
            "  {} Mic is working.",
            style("OK").green().bold()
        );
    }

    Ok(())
}
