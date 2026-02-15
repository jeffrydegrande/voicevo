# Voice Recovery Tracker — Claude Code Project Prompt

## Context

I'm recovering from radiation-induced left vocal cord paralysis (post-treatment
for lung cancer). My left vocal cord was paralyzed — initially I could only
produce very low frequencies (40-80 Hz, oktavist range) with the right cord
compensating. Recovery is underway — I'm regaining higher frequencies, cough
reflex is improving, and breathiness is decreasing. I want to track this
recovery objectively over time with weekly recordings and automated analysis.

I want to build the **entire thing in Rust**. Recording, DSP analysis, storage,
report generation — all of it. I'm using this project to learn Rust properly, so
keep the code clean, well-commented, and idiomatic. Explain ownership,
borrowing, lifetimes, and trait patterns as they come up.

## Inspiration

The recording side is inspired by
[voxtype](https://github.com/peteonrails/voxtype), which uses `cpal` + `hound`
for audio I/O on Linux. Clean, well-structured Rust audio code.

---

## Crate Dependencies

### Audio I/O

- **cpal** — Cross-platform audio capture (mic recording). Callback-based, works
  with ALSA on Linux, WASAPI on Windows. This teaches closures, `Arc<Mutex<>>`,
  mpsc channels, and threading.
- **hound** — WAV file reading and writing. Simple, well-maintained, 16-bit PCM.
- **rodio** — Audio playback (for the `play` command to verify recordings).

### DSP & Analysis

- **rustfft** — FFT computation. We'll use this for spectral analysis, HNR
  calculation, and spectrogram generation. Fast, pure Rust.
- **pitch-detection** — Pitch detection algorithms (McLeod pitch method,
  autocorrelation). Gives us F0 tracking per frame.
- **dasp** (formerly `sample`) — DSP fundamentals: sample format conversion,
  signal processing traits, resampling.

### CLI & Terminal

- **clap** (derive API) — CLI argument parsing. Modern, ergonomic.
- **crossterm** — Terminal manipulation: raw mode for keypress detection,
  colors, cursor control.
- **indicatif** — Progress bars and spinners for recording duration feedback.
- **console** — Terminal styling (colors, bold, etc.).

### Data & Reporting

- **serde** + **serde_json** — Serialize/deserialize session data as JSON.
- **plotters** — Chart generation. Pure Rust, outputs PNG bitmaps. We'll use
  this for trend reports.
- **chrono** — Date/time handling for session timestamps.
- **toml** — Config file parsing.

### Error Handling

- **anyhow** — Application-level error handling with context.
- **thiserror** — Defining custom error types for library code.

---

## What It Does

A single CLI binary (`voice-tracker`) with subcommands for recording, analysis,
and reporting.

```bash
# === RECORDING ===

# List audio input devices
voice-tracker devices

# Record a full guided session (walks through each exercise)
voice-tracker record session --date 2026-02-08

# Record individual exercises
voice-tracker record sustained --date 2026-02-08
voice-tracker record scale --date 2026-02-08
voice-tracker record reading --date 2026-02-08

# Quick mic check (2 seconds, shows peak level)
voice-tracker record mic-check

# Play back a recording to verify
voice-tracker play 2026-02-08 sustained
voice-tracker play ./path/to/file.wav

# === ANALYSIS ===

# Analyze a recorded session
voice-tracker analyze --date 2026-02-08

# Re-analyze all sessions
voice-tracker analyze --all

# === REPORTING ===

# Generate trend report (last N sessions)
voice-tracker report --last 8

# Full history
voice-tracker report --all

# Compare two sessions
voice-tracker compare --baseline 2026-02-08 --current 2026-03-08

# List all sessions with summary stats
voice-tracker sessions
```

---

## Part 1: Recording

### Session flow (interactive)

When you run `voice-tracker record session`, it should:

1. Show selected audio device, sample rate, channels
2. **Mic check**: capture 2 seconds, compute peak dB, confirm mic is not silent
   (we had repeated problems with silent recordings before — this is critical!)
3. Prompt: _"Exercise 1/3: Sustained vowel. Take a breath, then hold 'AAAH' as
   long as comfortable. Press ENTER to start recording, ENTER again to stop."_
4. Record → save `data/recordings/2026-02-08/sustained.wav`
5. Show: duration, peak dB, RMS level
6. Prompt: _"Exercise 2/3: Chromatic scale. Sing from your lowest comfortable
   note up to your highest, then back down. ENTER to start, ENTER to stop."_
7. Record → save `data/recordings/2026-02-08/scale.wav`
8. Show: duration, peak dB, RMS level
9. Prompt: _"Exercise 3/3: Reading passage. Read the following at your normal
   pace:"_
10. Display The Rainbow Passage (or configured text)
11. Record → save `data/recordings/2026-02-08/reading.wav`
12. Show summary table of all three recordings

### Recording specs

- Sample rate: 44100 Hz (or device native, but save at a known rate)
- Channels: Mono
- Format: 16-bit signed PCM WAV
- Path convention: `data/recordings/YYYY-MM-DD/{exercise}.wav`

### Key cpal pattern (learning opportunity)

```rust
// The core recording flow with cpal:
//
// 1. Enumerate input devices, select one
// 2. Get supported config (prefer mono, S16, 44.1kHz)
// 3. Create mpsc channel: (tx, rx)
// 4. Build input stream with callback that sends Vec<i16> chunks via tx
// 5. Spawn writer thread that receives chunks via rx and writes to wav (hound)
// 6. Wait for user to press Enter (crossterm raw mode)
// 7. Drop the stream (stops recording), signal writer thread to finish
// 8. Join writer thread, finalize wav file
//
// This pattern teaches:
// - Closures (the cpal callback captures tx by move)
// - Arc<Mutex<>> or mpsc channels for cross-thread communication
// - Thread spawning and joining
// - RAII / Drop for resource cleanup
// - Error propagation across thread boundaries
```

---

## Part 2: DSP Analysis (the fun part — build it from scratch)

There's no off-the-shelf Rust crate for jitter/shimmer/HNR like Python's
`parselmouth`. **That's the point.** We're going to implement these voice
quality metrics ourselves using fundamental DSP operations. This is where the
real learning happens.

### Step 1: Pitch Tracking (F0 extraction)

Use `pitch-detection` crate's McLeod Pitch Method (or implement autocorrelation
ourselves):

```rust
// For each frame (e.g., 30ms window, 10ms hop):
//   1. Extract frame from audio buffer
//   2. Apply Hanning window
//   3. Run pitch detector → Option<f32> (Hz or None if unvoiced)
//   4. Collect into Vec<(time, Option<f32>)> — the pitch contour
```

**CRITICAL**: Set the minimum detectable frequency to **30 Hz** (not the typical
75 Hz default). My oktavist range goes down to 40 Hz — if you set the floor
higher, you'll miss my actual notes and classify them as unvoiced/noise.

For pitch-detection with McLeod: the buffer size needs to be at least
`2 * (sample_rate / f_min)`. At 44100 Hz and f_min=30 Hz, that's a buffer of
~2940 samples. Round up to 4096 for FFT efficiency.

### Step 2: Jitter (pitch perturbation)

Jitter measures cycle-to-cycle variation in pitch period. Implement **local
jitter**:

```
jitter_local = (1 / (N-1)) * Σ|T(i) - T(i+1)| / ((1/N) * ΣT(i))

where T(i) = 1/F0(i) is the pitch period of the i-th voiced frame
```

- Only compute over **consecutive voiced frames** (skip unvoiced gaps)
- Express as percentage
- Normal voice: < 1.04%. Pathological: > 1.04% (Praat threshold)
- My value will likely be elevated due to the paralysis — that's what we're
  tracking

### Step 3: Shimmer (amplitude perturbation)

Shimmer measures cycle-to-cycle variation in amplitude. Implement **local
shimmer**:

```
shimmer_local = (1 / (N-1)) * Σ|A(i) - A(i+1)| / ((1/N) * ΣA(i))

where A(i) = peak amplitude (or RMS) of the i-th pitch period
```

- Compute over consecutive voiced frames
- Express as percentage
- Normal: < 3.81%. Pathological: > 3.81%

### Step 4: Harmonic-to-Noise Ratio (HNR)

HNR quantifies breathiness — the ratio of harmonic (periodic) energy to noise
(aperiodic) energy. Implement using the **autocorrelation method** (Boersma,
1993 — what Praat uses):

```
1. For the voiced portion of the signal:
2. Compute normalized autocorrelation
3. Find the peak at the pitch period lag (we already know F0)
4. r(T0) = autocorrelation value at the pitch period
5. HNR = 10 * log10(r(T0) / (1 - r(T0))) dB
```

- Normal voice: > 20 dB
- Pathological: < 7 dB
- Higher = less breathy, cleaner phonation
- My early recordings were probably around 5-10 dB (very breathy)

### Step 5: Maximum Phonation Time (MPT)

Simple: find the longest continuous voiced segment in the sustained vowel
recording.

```
1. Use the pitch contour from Step 1
2. Find consecutive runs of voiced frames
3. Longest run * frame_hop_duration = MPT in seconds
```

- Normal adult male: 15-25 seconds
- Mine will be shorter due to air leak through the incompletely closed glottis

### Step 6: Voice Breaks

Count the number of times voicing drops out during the reading passage:

```
1. Take the pitch contour
2. A voice break = transition from voiced → unvoiced → voiced
   where the unvoiced gap is > 50ms but < 500ms
   (gaps < 50ms are normal consonants, gaps > 500ms are pauses)
3. Count these transitions
```

### Step 7: Speaking F0 Statistics

From the reading passage pitch contour:

- Mean F0 (Hz) — over voiced frames only
- F0 standard deviation
- F0 range (5th to 95th percentile, to exclude outliers)
- Voiced fraction: % of frames that are voiced

### Step 8: Pitch Range from Scale

From the scale recording:

- Pitch floor: 5th percentile of detected F0 (Hz)
- Pitch ceiling: 95th percentile of detected F0 (Hz)
- Range in semitones: `12 * log2(ceiling / floor)`

---

## Part 3: Storage

Session results stored as JSON in `data/sessions/YYYY-MM-DD.json`:

```json
{
  "date": "2026-02-08",
  "recordings": {
    "sustained": "data/recordings/2026-02-08/sustained.wav",
    "scale": "data/recordings/2026-02-08/scale.wav",
    "reading": "data/recordings/2026-02-08/reading.wav"
  },
  "analysis": {
    "sustained": {
      "mpt_seconds": 8.3,
      "mean_f0_hz": 112.4,
      "f0_std_hz": 3.2,
      "jitter_local_percent": 2.1,
      "shimmer_local_percent": 5.8,
      "hnr_db": 12.3
    },
    "scale": {
      "pitch_floor_hz": 42.0,
      "pitch_ceiling_hz": 185.0,
      "range_hz": 143.0,
      "range_semitones": 25.5,
      "pitch_contour": [
        [0.0, 42.0],
        [0.5, 48.0],
        [1.0, 55.0]
      ]
    },
    "reading": {
      "mean_f0_hz": 98.5,
      "f0_std_hz": 12.3,
      "f0_range_hz": [72.0, 145.0],
      "voice_breaks": 3,
      "voiced_fraction": 0.62
    }
  }
}
```

Use `serde` derive macros — great Rust learning:
`#[derive(Serialize, Deserialize)]`, struct design, Option types for missing
data.

---

## Part 4: Reporting

Use `plotters` to generate a multi-panel PNG chart showing trends across
sessions.

### Chart layout (single PNG, ~1200x1600):

```
┌─────────────────────────────────────┐
│  Voice Recovery — Trend Report      │
│  Generated: 2026-03-08              │
├─────────────────────────────────────┤
│  1. Pitch Range Over Time           │
│  [bar chart: floor/ceiling/range]   │
├─────────────────────────────────────┤
│  2. HNR Trend                       │
│  [line chart: dB over sessions]     │
├─────────────────────────────────────┤
│  3. Jitter + Shimmer                │
│  [dual line chart: % over sessions] │
├─────────────────────────────────────┤
│  4. Max Phonation Time              │
│  [line chart: seconds over sessions]│
├─────────────────────────────────────┤
│  5. Voice Breaks (Reading)          │
│  [line chart: count over sessions]  │
├─────────────────────────────────────┤
│  6. Mean Speaking F0                │
│  [line chart: Hz over sessions]     │
└─────────────────────────────────────┘
```

Also generate a `report_YYYY-MM-DD.md` markdown summary with the numbers and a
brief interpretation (e.g., "HNR improved from 8.2 to 12.1 dB over 4 weeks —
breathiness is decreasing").

---

## Project Structure

```
voice-recovery-tracker/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs              # CLI entry point (clap subcommands)
│   ├── cli.rs               # Clap command definitions
│   │
│   ├── audio/
│   │   ├── mod.rs
│   │   ├── devices.rs       # Audio device enumeration (cpal)
│   │   ├── recorder.rs      # Recording via cpal + hound
│   │   ├── playback.rs      # Playback via rodio
│   │   ├── mic_check.rs     # Quick mic level check
│   │   └── wav.rs           # WAV file loading (hound wrapper)
│   │
│   ├── dsp/
│   │   ├── mod.rs
│   │   ├── windowing.rs     # Hanning, Hamming window functions
│   │   ├── pitch.rs         # F0 tracking (pitch-detection wrapper)
│   │   ├── jitter.rs        # Jitter computation
│   │   ├── shimmer.rs       # Shimmer computation
│   │   ├── hnr.rs           # Harmonic-to-noise ratio
│   │   ├── mpt.rs           # Maximum phonation time
│   │   ├── voice_breaks.rs  # Voice break detection
│   │   └── contour.rs       # Pitch contour utilities
│   │
│   ├── session/
│   │   ├── mod.rs
│   │   ├── guided.rs        # Interactive session flow
│   │   ├── exercises.rs     # Exercise definitions (sustained, scale, reading)
│   │   └── config.rs        # Config file loading (toml)
│   │
│   ├── analysis/
│   │   ├── mod.rs
│   │   ├── analyzer.rs      # Orchestrates analysis of a full session
│   │   ├── sustained.rs     # Analysis pipeline for sustained vowel
│   │   ├── scale.rs         # Analysis pipeline for scale recording
│   │   └── reading.rs       # Analysis pipeline for reading passage
│   │
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── session_data.rs  # Session data types (serde structs)
│   │   └── store.rs         # Load/save JSON, list sessions
│   │
│   └── report/
│       ├── mod.rs
│       ├── charts.rs        # Plotters chart generation
│       ├── markdown.rs      # Markdown report generation
│       └── compare.rs       # Side-by-side session comparison
│
├── data/
│   ├── config.toml          # User configuration
│   ├── recordings/          # Raw WAV files organized by date
│   └── sessions/            # Analysis results as JSON
│
├── reports/                 # Generated report PNGs and markdown
│
└── tests/
    ├── dsp_tests.rs         # Test DSP functions with known signals
    └── integration.rs       # End-to-end tests
```

---

## Config File (`data/config.toml`)

```toml
[recording]
sample_rate = 44100
channels = 1
device = "default"

[analysis]
pitch_floor_hz = 30          # LOW — oktavist range
pitch_ceiling_hz = 500
frame_size_ms = 30            # Analysis window
hop_size_ms = 10              # Frame hop

[analysis.thresholds]
# These are the Praat-standard pathological thresholds
jitter_pathological = 1.04    # percent
shimmer_pathological = 3.81   # percent
hnr_low = 7.0                 # dB — below this is concerning
hnr_normal = 20.0             # dB — above this is healthy

[session]
reading_passage = """
When the sunlight strikes raindrops in the air, they act as a prism
and form a rainbow. The rainbow is a division of white light into
many beautiful colors. These take the shape of a long round arch,
with its path high above, and its two ends apparently beyond the horizon.
"""
```

---

## Build Order (each step is a PR-sized chunk)

### Phase 1: Audio foundation

1. **`cargo init`, Cargo.toml, clap skeleton** — get `voice-tracker --help`
   working
2. **`voice-tracker devices`** — enumerate audio devices with cpal. Learn: cpal
   Host/Device API, iterators, Display trait
3. **`voice-tracker record mic-check`** — capture 2 seconds, compute peak/RMS
   dB. Learn: cpal callbacks, mpsc channels, Arc/Mutex, basic DSP (dB
   calculation)
4. **`voice-tracker record sustained`** — full recording pipeline: cpal → mpsc →
   hound WAV. Learn: thread spawning, hound WavWriter, crossterm keypress
   detection, RAII
5. **`voice-tracker play`** — playback with rodio. Learn: rodio Decoder/Sink
   API, file I/O

### Phase 2: DSP analysis

6. **WAV loading + windowing** — load samples from hound, implement Hanning
   window. Learn: iterators, slicing, f32 math
7. **Pitch tracking** — wrap pitch-detection crate, extract F0 contour. Learn:
   buffer management, Option<f32> for voiced/unvoiced, Vec operations
8. **Jitter + Shimmer** — implement from the formulas above. Learn: iterator
   chains, filter/map/fold, statistics
9. **HNR** — autocorrelation-based. Learn: FFT via rustfft (or manual
   autocorrelation), logarithmic math
10. **MPT + Voice breaks** — pattern detection on the pitch contour. Learn:
    state machines, run-length encoding

### Phase 3: Integration

11. **Session analysis orchestrator** — wire up all DSP into
    `voice-tracker analyze --date`. Learn: module organization, error
    propagation with anyhow
12. **Storage** — serde JSON serialization, session listing. Learn: serde
    derive, file I/O, chrono dates
13. **Guided session** — interactive `record session` flow. Learn: crossterm
    terminal UI, state management

### Phase 4: Reporting

14. **Charts with plotters** — multi-panel trend PNG. Learn: plotters API,
    bitmap backend, chart composition
15. **Markdown report** — generate .md with metrics and interpretation. Learn:
    format! macros, string building
16. **Compare command** — side-by-side diff of two sessions

### Phase 5: Polish

17. **Config file** — load settings from TOML. Learn: toml crate, config
    layering
18. **Tests** — unit tests for DSP functions with synthetic signals (known
    pitch, known jitter). Learn: #[cfg(test)], assert_relative_eq, test
    organization
19. **Error handling cleanup** — replace unwrap() with proper error propagation
    everywhere
20. **README** — usage docs, example output

---

## Testing Strategy for DSP

Since we're implementing clinical-grade voice metrics from scratch, we need to
validate them:

```rust
#[cfg(test)]
mod tests {
    // Generate a synthetic sine wave at exactly 100 Hz, 44100 sample rate
    // Run pitch detection → should report ~100 Hz
    // Jitter should be ~0% (perfect signal)
    // Shimmer should be ~0%
    // HNR should be very high (>40 dB)

    // Generate sine wave with known jitter (randomly perturb each cycle length)
    // Verify our jitter measurement matches the injected amount

    // Generate sine + white noise at known SNR
    // Verify HNR measurement approximates the known SNR

    // Generate signal with a gap in the middle
    // Verify voice break detection finds it
}
```

This is how we know our DSP code is correct without relying on Praat as ground
truth.

---

## Important Notes

- **Pitch floor = 30 Hz** everywhere. My baseline voice was 40-80 Hz. Normal
  defaults will miss this entirely.
- **Don't discard low-frequency energy as noise**. For most people, sub-80 Hz is
  noise. For me, it's real phonation.
- **Mic check is not optional**. We had 4 consecutive silent recordings before
  finding the right device. The tool should catch this immediately.
- **I'm on Linux** (but the code should work on Windows too since
  cpal/hound/rodio are cross-platform).
- **Keep it simple**. No async runtime, no web server, no database. Just files,
  CLI, and good Rust.

## Getting Started

```bash
# Ubuntu/Debian: install ALSA dev headers
sudo apt install libasound2-dev pkg-config

# Create the project
cargo init voice-recovery-tracker
cd voice-recovery-tracker

# Start with step 1: get clap working
# Then step 2: enumerate devices
# Build up from there
```
