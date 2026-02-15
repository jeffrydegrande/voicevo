use std::path::Path;

use anyhow::{Context, Result};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};

/// Standard WAV spec for our recordings: mono 16-bit PCM.
pub fn recording_spec(sample_rate: u32) -> WavSpec {
    WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

/// Create a WavWriter at the given path, creating parent directories as needed.
pub fn create_writer(path: &Path, spec: WavSpec) -> Result<WavWriter<std::io::BufWriter<std::fs::File>>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    WavWriter::create(path, spec)
        .with_context(|| format!("Failed to create WAV file: {}", path.display()))
}

/// Load all samples from a WAV file as f32 in [-1.0, 1.0].
/// Returns (samples, spec) so callers can read the sample rate.
pub fn load_samples(path: &Path) -> Result<(Vec<f32>, WavSpec)> {
    let mut reader = WavReader::open(path)
        .with_context(|| format!("Failed to open WAV file: {}", path.display()))?;

    let spec = reader.spec();

    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Int => {
            let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_val))
                .collect::<hound::Result<Vec<_>>>()
                .context("Failed to read WAV samples")?
        }
        SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<hound::Result<Vec<_>>>()
            .context("Failed to read WAV samples")?,
    };

    Ok((samples, spec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_wav_path() -> PathBuf {
        // Use a temp dir so tests don't pollute the working directory
        let dir = std::env::temp_dir().join("voice-tracker-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("roundtrip.wav")
    }

    #[test]
    fn wav_roundtrip() {
        let path = test_wav_path();
        let spec = recording_spec(44100);

        // Write a known signal: a short ramp
        let original: Vec<f32> = (0..1000).map(|i| (i as f32 / 1000.0) * 2.0 - 1.0).collect();

        {
            let mut writer = create_writer(&path, spec).unwrap();
            for &sample in &original {
                // Convert f32 to i16 for writing
                let s16 = (sample * i16::MAX as f32) as i16;
                writer.write_sample(s16).unwrap();
            }
            writer.finalize().unwrap();
        }

        // Read back and verify
        let (loaded, loaded_spec) = load_samples(&path).unwrap();
        assert_eq!(loaded_spec.sample_rate, 44100);
        assert_eq!(loaded_spec.channels, 1);
        assert_eq!(loaded.len(), original.len());

        // Verify samples match within quantization error (16-bit → ~0.00003 tolerance)
        for (orig, loaded) in original.iter().zip(loaded.iter()) {
            assert!(
                (orig - loaded).abs() < 0.001,
                "Sample mismatch: original={orig}, loaded={loaded}"
            );
        }

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recording_spec_values() {
        let spec = recording_spec(48000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.sample_rate, 48000);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_format, SampleFormat::Int);
    }

    #[test]
    fn load_nonexistent_file() {
        let result = load_samples(Path::new("/tmp/does-not-exist-voice-tracker.wav"));
        assert!(result.is_err());
    }
}
