# voicevo

A command-line tool for tracking vocal cord recovery through objective acoustic measurements. Records voice exercises, runs DSP analysis (pitch tracking, jitter, shimmer, HNR), and generates trend reports over time.

Built entirely in Rust. No external analysis tools required.

## Why

After radiation-induced vocal cord paralysis, recovery is slow and hard to gauge subjectively. This tool replaces guesswork with numbers: weekly recordings go through the same analysis pipeline, producing comparable metrics that show whether things are actually improving.

## Install

Requires ALSA development headers on Linux:

```bash
# Debian/Ubuntu
sudo apt install libasound2-dev pkg-config

# Arch
sudo pacman -S alsa-lib pkg-config
```

Then build from source:

```bash
cargo install --path .
```

## Quick start

```bash
# Check your microphone
voicevo record mic-check

# Run a full guided session (sustained vowel, scale, reading passage)
voicevo record session

# Analyze the recordings
voicevo analyze --date 2026-02-08

# Get an LLM interpretation of the results
voicevo explain --date 2026-02-08

# Generate a trend report across sessions
voicevo report --all
```

## Commands

| Command | Description |
|---------|-------------|
| `voicevo devices` | List audio input devices |
| `voicevo record session` | Guided session: mic check + all three exercises |
| `voicevo record sustained` | Record a sustained vowel |
| `voicevo record scale` | Record a chromatic scale (low to high and back) |
| `voicevo record reading` | Record a reading passage |
| `voicevo record mic-check` | Quick 2-second mic level check |
| `voicevo play <date> <exercise>` | Play back a recording |
| `voicevo analyze --date <date>` | Analyze a session's recordings |
| `voicevo analyze --all` | Re-analyze all sessions |
| `voicevo explain --date <date>` | LLM interpretation of analysis results |
| `voicevo report --last 8` | Trend report for recent sessions |
| `voicevo report --all` | Trend report for all sessions |
| `voicevo compare --baseline <date> --current <date>` | Side-by-side session comparison |
| `voicevo sessions` | List all analyzed sessions |
| `voicevo browse` | Open the latest report chart |
| `voicevo paths` | Show config and data directories |

## What it measures

**Sustained vowel** (hold "AAAH"):
- Maximum phonation time (MPT)
- Mean fundamental frequency (F0)
- Jitter (pitch stability, cycle-to-cycle)
- Shimmer (amplitude stability, cycle-to-cycle)
- Harmonics-to-noise ratio (HNR, breathiness)

**Chromatic scale** (low to high and back):
- Pitch floor and ceiling
- Total range in Hz and semitones

**Reading passage**:
- Speaking F0 statistics
- Voice break count
- Voiced fraction

Clinical thresholds follow Praat standards (Boersma & Weenink). Jitter below 1.04% and shimmer below 3.81% are considered normal. HNR above 20 dB indicates healthy phonation.

## Configuration

Config lives at `~/.config/voicevo/config.toml` (XDG on Linux, `~/Library/Application Support/voicevo` on macOS). All fields are optional and fall back to sensible defaults.

```toml
[recording]
sample_rate = 44100
channels = 1
device = "default"

[analysis]
pitch_floor_hz = 30       # low enough for oktavist range
pitch_ceiling_hz = 1000
frame_size_ms = 30
hop_size_ms = 10

[analysis.thresholds]
jitter_pathological = 1.04
shimmer_pathological = 3.81
hnr_low = 7.0
hnr_normal = 20.0

[session]
reading_passage = "When the sunlight strikes raindrops in the air..."
```

## Data storage

Recordings and analysis results are stored under `~/.local/share/voicevo/` (XDG on Linux):

```
~/.local/share/voicevo/
  recordings/
    2026-02-08/
      sustained.wav
      scale.wav
      reading.wav
  sessions/
    2026-02-08.json
  reports/
    report_2026-02-08.png
    report_2026-02-08.md
```

## License

MIT
