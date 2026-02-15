use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::{Context, Result};
use console::style;
use rodio::{Decoder, OutputStream, Sink};

use crate::util;

/// Play back a recording. `target` is either a YYYY-MM-DD date or a direct file path.
/// If it's a date, `exercise` must be provided (e.g., "sustained").
pub fn play(target: &str, exercise: Option<&str>) -> Result<()> {
    let path = resolve_play_path(target, exercise)?;

    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    println!(
        "Playing {}",
        style(path.display()).green()
    );

    // OutputStream::try_default() opens the default output device.
    // We must keep `_stream` alive — it's an RAII guard. If we used `_` instead
    // of `_stream`, Rust would drop it immediately (underscore = "I don't need this").
    // `_stream` (with a name) keeps it alive until the end of this function scope.
    let (_stream, stream_handle) =
        OutputStream::try_default().context("Failed to open audio output device")?;

    // Sink gives us playback control (pause, stop, wait).
    // It runs on a background thread managed by rodio.
    let sink = Sink::try_new(&stream_handle).context("Failed to create audio sink")?;

    let file = File::open(&path)
        .with_context(|| format!("Failed to open: {}", path.display()))?;
    let reader = BufReader::new(file);

    // Decoder figures out the audio format (WAV in our case) and produces samples.
    let source = Decoder::new(reader)
        .with_context(|| format!("Failed to decode: {}", path.display()))?;

    // append() queues audio into the sink. It starts playing immediately since
    // the sink isn't paused.
    sink.append(source);

    // Block until playback finishes. Without this, the function would return,
    // dropping `_stream` and cutting off audio mid-play.
    sink.sleep_until_end();

    println!("Done.");
    Ok(())
}

/// Figure out which file to play based on user input.
fn resolve_play_path(target: &str, exercise: Option<&str>) -> Result<PathBuf> {
    // If target looks like a file path (contains a dot or slash), use it directly
    if target.contains('/') || target.contains('.') {
        return Ok(PathBuf::from(target));
    }

    // Otherwise treat it as a date — exercise name is required
    let exercise = exercise.context(
        "When playing by date, you must specify an exercise name.\n\
         Usage: voice-tracker play 2026-02-08 sustained",
    )?;

    let date = util::resolve_date(Some(target))?;
    Ok(util::recording_path(&date, exercise))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_direct_path() {
        let path = resolve_play_path("./test.wav", None).unwrap();
        assert_eq!(path, PathBuf::from("./test.wav"));
    }

    #[test]
    fn resolve_absolute_path() {
        let path = resolve_play_path("/tmp/test.wav", None).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.wav"));
    }

    #[test]
    fn resolve_date_with_exercise() {
        let path = resolve_play_path("2026-02-08", Some("sustained")).unwrap();
        assert_eq!(
            path,
            PathBuf::from("data/recordings/2026-02-08/sustained.wav")
        );
    }

    #[test]
    fn resolve_date_without_exercise_fails() {
        let result = resolve_play_path("2026-02-08", None);
        assert!(result.is_err());
    }
}
