# Voice Recovery Tracker — DSP Concepts Explained

A guide to the digital signal processing behind this tool. Written as we built
it, so each section mirrors a piece of code you can read alongside.

---

## 1. Windowing (src/dsp/windowing.rs)

When you analyze audio, you work on small chunks called **frames** — typically
30 milliseconds at a time. But if you just chop the signal abruptly, the
cut-off edges create artifacts. Imagine slicing a sine wave in the middle of a
cycle — that sudden discontinuity looks like high-frequency noise to any
frequency analysis algorithm.

A **window function** solves this by smoothly tapering the edges of each frame
to zero. The **Hanning window** (also called Hann) is the most common:

```
w(n) = 0.5 * (1 - cos(2π * n / (N-1)))
```

At the edges (n=0, n=N-1): w = 0.0 — the signal fades out.
At the center (n=N/2): w = 1.0 — the signal passes through unchanged.

You multiply each sample by its window coefficient: `output[i] = input[i] * w(i)`.
This gives the analysis algorithm a clean, tapered chunk to work with.

---

## 2. Pitch Tracking — F0 Extraction (src/dsp/pitch.rs)

**F0** (fundamental frequency) is the base frequency of your voice — the note
you're producing. When your vocal cords vibrate, they open and close
periodically, creating a sound wave. F0 is the rate of that vibration in Hertz
(cycles per second).

- A typical adult male speaks at 85–180 Hz.
- A typical adult female speaks at 165–255 Hz.

### Why the pitch floor matters

Most pitch detectors default to a minimum of 75 Hz, because that covers the
normal male speaking range. But with vocal cord paralysis, especially early in
recovery, the voice can be much lower. When only one cord is working and it's
compensating for the paralyzed one, the vibration rate drops. In this case, the
range goes down to 40–80 Hz (called the **oktavist range** — the territory of
the lowest bass singers in the world).

We set our pitch floor to **30 Hz**. If we used the standard 75 Hz floor, the
detector would classify your actual voice as "unvoiced" (noise/silence) and
you'd get no data.

### Voiced vs Unvoiced

Not every moment of speech has a clear pitch:

- **Voiced sounds** (vowels, "z", "m", "l"): vocal cords vibrating, pitch
  detectable
- **Unvoiced sounds** ("s", "f", "t", "sh"): turbulent airflow, no periodic
  vibration
- **Silence/pauses**: no sound at all

The pitch detector returns `None` for unvoiced frames and `Some(frequency)` for
voiced ones. The resulting time series of (timestamp, optional_frequency) pairs
is called a **pitch contour** — it's the foundation for almost every other
measurement.

### The McLeod Pitch Method

The algorithm we use works by computing a **normalized autocorrelation** of the
signal. Autocorrelation means comparing the signal with shifted copies of
itself. If you shift a periodic signal by exactly one period, it lines up
perfectly — high correlation. The McLeod method finds this peak efficiently and
is robust to harmonics (it won't get confused by overtones and report double the
actual pitch).

### Buffer sizing for low frequencies

The detector needs at least 2 full cycles of the lowest frequency to work.
At 30 Hz and 44100 Hz sample rate:
- One period = 44100 / 30 = 1470 samples
- Two periods = 2940 samples
- Rounded up to 4096 (next power of 2, for FFT efficiency)

This means each analysis frame is about 93ms of audio — much longer than the
typical 30ms used for speech at normal pitch ranges. That's the tradeoff for
detecting very low frequencies: you need more audio per frame.

---

## 3. Jitter — Pitch Stability (src/dsp/jitter.rs)

Imagine your vocal cords vibrating. In a healthy voice, each vibration cycle
takes almost exactly the same time. **Jitter** measures how much the cycle
length varies from one cycle to the next — it's a cycle-to-cycle pitch
perturbation metric.

### How it works

1. Take each detected F0 frequency and convert to a **period**: T = 1/F0
   (in seconds). At 100 Hz, T = 0.01 seconds = 10 milliseconds.

2. Compare consecutive periods: how much does T(i+1) differ from T(i)?

3. Average those absolute differences and normalize by the mean period:

```
jitter = mean(|T(i) - T(i+1)|) / mean(T) × 100%
```

### What it tells you

- **Normal voice**: jitter < 1.04% (the Praat threshold for pathological)
- **Pathological voice**: jitter > 1.04%

With vocal cord paralysis, the working cord can't maintain tension as
consistently because it's compensating for the paralyzed one. The vibration
rate fluctuates more from cycle to cycle, so jitter goes up.

### Important detail: gaps

We only measure jitter over **consecutive voiced frames**. If there's an
unvoiced gap between two voiced segments (like a pause or a consonant), we
don't compare across that gap — the change in pitch would be meaningless. We
reset the comparison chain at every gap.

---

## 4. Shimmer — Amplitude Stability (src/dsp/shimmer.rs)

Shimmer is the amplitude counterpart of jitter. Instead of measuring how much
the pitch period varies, it measures how much the **loudness** varies from one
cycle to the next.

### How it works

1. For each voiced frame, we know the pitch period T = 1/F0
2. Extract one pitch period's worth of audio starting at that frame's position
3. Measure the peak amplitude of that period
4. Compare consecutive amplitudes:

```
shimmer = mean(|A(i) - A(i+1)|) / mean(A) × 100%
```

### What it tells you

- **Normal voice**: shimmer < 3.81%
- **Pathological voice**: shimmer > 3.81%

With incomplete vocal cord closure (as in paralysis), the glottis can't
maintain consistent pressure. Some cycles push more air through the gap than
others, causing the amplitude to fluctuate. As recovery progresses and the
cord regains movement, closure improves and shimmer decreases.

---

## 5. HNR — Harmonic-to-Noise Ratio (src/dsp/hnr.rs)

This is probably the most clinically relevant metric for vocal cord recovery.
**HNR** measures how much of your voice is clean periodic vibration (harmonics)
versus turbulent noise (air escaping through the glottis).

### The two components of voice

Every voice signal is a mix of:

- **Harmonics**: The clean, tonal part — the actual vocal cord vibration
  producing a fundamental frequency and its overtones (multiples of F0).
  These are what make your voice sound like a *note*.

- **Noise**: The breathy, airy part — turbulent airflow through the glottis.
  In a healthy voice, the cords close fully during each cycle, so there's very
  little turbulence. With paralysis, air leaks through the gap continuously,
  adding a "hhhh" sound to everything.

### The math

We use the **autocorrelation method** (Boersma, 1993 — what Praat uses):

1. We already know F0 from pitch tracking, so we know the pitch period T0
2. Compute the **normalized autocorrelation** at lag T0:
   - Take the signal
   - Shift it by exactly one period
   - Compute the correlation between original and shifted versions
3. The correlation value r(T0) ranges from 0 to 1:
   - r = 1.0: signal is perfectly periodic (pure tone, no noise)
   - r = 0.5: equal harmonic and noise energy
   - r = 0.0: completely random (pure noise)
4. Convert to decibels: HNR = 10 × log₁₀(r / (1 - r)) dB

### What the numbers mean

| HNR (dB) | Interpretation |
|----------|---------------|
| > 20 dB  | Healthy voice — strong harmonics, minimal breathiness |
| 12–20 dB | Mild breathiness — early recovery |
| 7–12 dB  | Moderate breathiness — significant air leak |
| < 7 dB   | Severe breathiness — very incomplete cord closure |

As recovery progresses, HNR is expected to climb: the paralyzed cord regains
movement → the glottis closes better → less air leak → less noise → higher HNR.

### Why autocorrelation works

A periodic signal, when shifted by exactly one period, lines up perfectly with
itself — high correlation. The noisy component, being random, doesn't correlate
with anything. So the autocorrelation at the pitch period lag isolates the
periodic (harmonic) energy from the noise.

---

## 6. Maximum Phonation Time (src/dsp/mpt.rs)

MPT is the simplest metric conceptually: **how long can you sustain a vowel
sound?** You take a deep breath and hold "AAAH" for as long as possible.

### What it measures

MPT reflects how efficiently your vocal cords use air. With healthy, fully
closing cords:
- Air is used efficiently (small puffs per cycle)
- You can sustain phonation for 15–25 seconds (adult male)

With paralysis:
- Air leaks through the gap continuously
- Your air supply depletes much faster
- MPT drops to maybe 3–8 seconds

### How we compute it

We don't use a stopwatch — we compute it from the pitch contour:

1. Take the pitch contour from the sustained vowel recording
2. Find all runs of consecutive voiced frames (periods where pitch is detected)
3. The longest run × frame hop duration = MPT

This is more accurate than manual timing because it only counts actual voicing,
not the silence before or after.

---

## 7. Voice Breaks (src/dsp/voice_breaks.rs)

During continuous speech, a healthy voice transitions smoothly between sounds.
**Voice breaks** are moments where voicing drops out unexpectedly — the cord
stops vibrating briefly and then restarts.

### Distinguishing breaks from normal speech

Not every voicing gap is a break:
- **< 50ms**: Normal. Unvoiced consonants ("t", "s", "p") naturally create
  short gaps.
- **50–500ms**: Voice break. The cord lost its vibration pattern and had to
  restart. This is what we count.
- **> 500ms**: Intentional pause. The speaker stopped to breathe or think.

### What it tells you

More voice breaks = less stable phonation. As recovery progresses, the cord
can maintain vibration more consistently, and voice breaks decrease.

---

## 8. Pitch Range from Scale (src/analysis/scale.rs)

The scale exercise asks you to sing from your lowest comfortable note up to
your highest, then back down. This maps your **functional pitch range** — the
notes your voice can currently produce.

We use **percentiles** (5th and 95th) instead of min/max to define the range.
Why? A mic bump might produce a stray 30 Hz detection, or a harmonic artifact
might register at 400 Hz. By using the 5th and 95th percentiles, we get the
effective range while excluding outlier detections.

The range is expressed in both Hz and **semitones**:
- A semitone is the smallest step in Western music (one piano key)
- 12 semitones = 1 octave = doubling of frequency
- Formula: semitones = 12 × log₂(f₂ / f₁)

A healthy adult male might have a range of 2–3 octaves (24–36 semitones).
With vocal cord paralysis, the range narrows significantly — maybe 1 octave
or less early in recovery. Tracking range over time shows whether you're
regaining access to higher (or lower) notes.

---

## 9. Reading Passage Analysis (src/analysis/reading.rs)

The reading passage captures **connected speech** — the voice in its natural
speaking mode, not just sustained tones. The Rainbow Passage is a standard
text used in speech pathology because it contains a balanced mix of voiced and
unvoiced sounds, different vowels, and varying prosody.

From the reading, we extract:
- **Mean speaking F0**: Your average pitch while talking. With paralysis,
  this is often lower than normal and may rise as recovery progresses.
- **F0 standard deviation**: How much your pitch varies during speech. Low
  variation (monotone) can indicate limited cord control.
- **Voiced fraction**: What percentage of the recording has detectable pitch.
  In normal speech, this is ~60-70% (the rest is consonants and pauses).
  With severe breathiness, the detector may fail to find pitch even during
  vowels, dropping this number.

---

## Rust Concepts Encountered

### Closures and Move Semantics (recording)

In the recording code, the cpal audio callback is a **closure** — an anonymous
function that captures variables from its environment. The `move` keyword
transfers ownership of the captured variables into the closure:

```rust
let tx = tx.clone();
device.build_input_stream(
    &config.into(),
    move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let _ = tx.send(data.to_vec());
    },
    ...
)
```

The `move` is required because the closure runs on a different thread (the
audio thread). Without `move`, the closure would try to *borrow* `tx`, but
borrows can't outlive the function that created them. By moving `tx` into
the closure, the closure *owns* it and can use it for as long as it lives.

### Option<T> — Nullable Values Done Right

Rust has no null. Instead, `Option<T>` explicitly represents "might have a
value, might not":
- `Some(100.0)` — we detected a pitch of 100 Hz
- `None` — this frame was unvoiced

This forces you to handle both cases at compile time. You can't accidentally
use a null pitch frequency and get a runtime crash. The `filter_map`,
`unwrap_or`, and `if let Some(x) = ...` patterns make working with Options
ergonomic.

### Trait Bounds and Generics (analysis)

The `analyze_exercise` function uses generics:

```rust
fn analyze_exercise<T, F>(name: &str, path: &Path, analyze_fn: F) -> Result<T>
where
    F: FnOnce(&[f32], u32) -> Result<T>,
```

`T` is a generic return type (could be `SustainedAnalysis`, `ScaleAnalysis`,
etc.). `F` is a generic function type — any closure matching that signature.
The compiler generates specialized versions for each concrete type at compile
time. This is a zero-cost abstraction: the generic code compiles to the same
machine code as if you'd written three separate functions.

### serde Derive Macros

```rust
#[derive(Serialize, Deserialize)]
pub struct SustainedAnalysis {
    pub mpt_seconds: f32,
    pub mean_f0_hz: f32,
    ...
}
```

The `derive` attribute tells the Rust compiler to auto-generate serialization
code. Under the hood, serde creates a `Serialize` implementation that walks
each field and converts it to the target format (JSON, TOML, etc.). You get
type-safe serialization with zero boilerplate.
