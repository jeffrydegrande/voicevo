# Plan: Rebuild DSP Foundation for Damaged Voice Detection

## Context

The current architecture is pitch-first: pitch detection is the gatekeeper for all metrics, with an energy-based fallback bolted on when pitch fails. For a severely damaged voice (left vocal cord paralysis), pitch detection fails most of the time — the signal has energy but no clean periodicity. This means most sessions hit the energy fallback tier, zeroing out jitter and voice breaks and producing unreliable shimmer/HNR.

The fix is architectural: make energy-based activity detection the foundation ("is the patient making sound?") and pitch detection a quality overlay on top ("is the sound periodic?"). Add CPPS as a pitch-independent periodicity metric. Add richer reliability metadata so downstream consumers (LLM synthesis, charts) know which numbers to trust. Tighten bridge thresholds, gate jitter/shimmer properly, and add new exercise protocols (S/Z ratio, fatigue slope) that answer "is this sustainable?"

Additionally, migrate from JSON files to SQLite for session storage. This enables versioned analysis data — old WAV files can be re-analyzed with the new pipeline and both versions kept side by side for comparison.

All 8 improvements from ideas.md + storage migration, implemented in 7 phases.

---

## Phase 0: Migrate Storage to SQLite

Foundation for versioned analysis. Add `rusqlite` dependency.

### Schema

```sql
CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    date TEXT NOT NULL UNIQUE,           -- "2026-02-18"
    sustained_path TEXT,
    scale_path TEXT,
    reading_path TEXT
);

CREATE TABLE analyses (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id),
    version INTEGER NOT NULL DEFAULT 1,  -- analysis pipeline version
    exercise TEXT NOT NULL,              -- "sustained" | "scale" | "reading" | "sz" | "fatigue"
    data TEXT NOT NULL,                  -- JSON blob of the analysis struct
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(session_id, version, exercise)
);
```

Version 1 = current pipeline. Version 2 = new pipeline (after all DSP phases). Each re-analysis creates new rows at the next version, old rows stay.

### New file: `src/storage/db.rs`

```rust
pub fn open_db() -> Result<Connection>          // opens/creates at paths::db_path()
pub fn init_schema(conn: &Connection) -> Result<()>
pub fn save_session(conn: &Connection, session: &SessionData) -> Result<()>
pub fn load_session(conn: &Connection, date: &str) -> Result<SessionData>
pub fn load_session_version(conn: &Connection, date: &str, version: u32) -> Result<SessionData>
pub fn list_sessions(conn: &Connection) -> Result<Vec<String>>
pub fn current_analysis_version() -> u32        // returns ANALYSIS_VERSION constant
pub fn list_versions(conn: &Connection, date: &str) -> Result<Vec<u32>>
```

The `save_session` function serializes each analysis struct to JSON and stores it as a blob in the `analyses` table. `load_session` loads the latest version by default.

### Migrate existing data

Add `voicevo migrate` command (one-time):
1. Scan `sessions/*.json` files
2. Parse each with current serde deserializer
3. Insert into SQLite as version=1
4. Print summary ("Migrated 4 sessions")
5. Don't delete the JSON files — user can do that manually

### Modify: `src/storage/mod.rs`
Add `pub mod db;`

### Modify: `src/paths.rs`
Add `pub fn db_path() -> PathBuf` — `$XDG_DATA_HOME/voicevo/voicevo.db`

### Modify: `src/storage/store.rs`
Replace file-based save/load with SQLite calls. Keep the same public API (`save_session`, `load_session`, `list_sessions`) so callers don't change. Add `load_session_version(date, version)`.

### Modify: `src/cli.rs`
Add `Migrate` command. Add `--version` flag to `Analyze` command for re-analysis at a specific version.

### Modify: `src/main.rs`
Wire up `Migrate` command. Pass DB connection through where needed.

### Modify: `src/storage/session_data.rs`
Add `pub const ANALYSIS_VERSION: u32 = 1;` — bumped to 2 after all DSP phases complete.

### Tests
- Create DB, insert session, load it back — roundtrip
- Load latest version when multiple exist
- Load specific version
- Migrate from JSON files
- `list_sessions` returns sorted dates
- Schema creation is idempotent

---

## Phase 1: Energy-Based Activity Detection + Lower Pitch Ceilings (#2, #8)

Foundation for everything else. Activity detection becomes ground truth.

### New file: `src/dsp/activity.rs`

```rust
pub struct ActivityConfig {
    pub threshold_on_db: f32,   // -45.0
    pub threshold_off_db: f32,  // -50.0
    pub min_active_ms: f32,     // 80.0
    pub min_silent_ms: f32,     // 120.0
    pub frame_size_ms: f32,     // 10.0 (matches pitch hop for 1:1 alignment)
}

pub struct ActivityResult {
    pub active_frames: Vec<bool>,  // aligned with pitch contour frames
    pub active_fraction: f32,
}

pub fn detect_activity(samples: &[f32], sample_rate: u32, config: &ActivityConfig) -> ActivityResult
pub fn voiced_quality(contour: &[PitchFrame], active_frames: &[bool]) -> f32
```

Algorithm: slide frame, compute RMS dB, apply hysteresis (on/off thresholds), post-process to enforce min durations. `voiced_quality = pitched_frames_in_active / total_active_frames`.

### Modify: `src/config.rs`

Add per-exercise pitch ceilings to `AnalysisConfig`:
```rust
pub sustained_ceiling_hz: f32,  // 500.0
pub reading_ceiling_hz: f32,    // 600.0
```

Add method:
```rust
impl AnalysisConfig {
    pub fn pitch_config_for(&self, exercise: &str) -> PitchConfig
}
```
Returns ceiling 500 for sustained, 600 for reading, 1000 (global) for scale.

### Modify: `src/dsp/mod.rs`
Add `pub mod activity;`

### Modify: `src/analysis/analyzer.rs`
Replace single `pitch_config` with per-exercise configs via `pitch_config_for()`.

### Modify: `src/analysis/sustained.rs`, `src/analysis/reading.rs`
Add activity detection call alongside pitch detection. Both now produce `ActivityResult` for downstream use (Phase 3+).

### Tests
- Sine wave: active_fraction ~1.0
- Silence: active_fraction ~0.0
- Signal with 200ms gap: two active segments
- Threshold hovering: hysteresis prevents toggling
- 50ms transient: filtered by min_active_ms=80ms
- Config: `pitch_config_for("sustained").pitch_ceiling_hz == 500.0`

---

## Phase 2: CPPS (#1)

Pitch-independent periodicity metric. Uses `rustfft` (already in Cargo.toml).

### New file: `src/dsp/cpps.rs`

```rust
pub struct CppsConfig {
    pub frame_size_ms: f32,      // 40.0
    pub hop_size_ms: f32,        // 10.0
    pub quefrency_min_ms: f32,   // 2.5 (400 Hz)
    pub quefrency_max_ms: f32,   // 16.7 (60 Hz)
    pub energy_gate_db: f32,     // -45.0
}

pub fn compute_cpps(samples: &[f32], sample_rate: u32, config: &CppsConfig) -> Option<f32>
```

Algorithm per frame: Hanning window -> FFT -> |X|^2 -> log power spectrum -> IFFT (cepstrum) -> find peak in quefrency range -> subtract linear regression at peak quefrency -> average across gated frames.

### Modify: `src/dsp/mod.rs`
Add `pub mod cpps;`

### Modify: `src/storage/session_data.rs`
Add `#[serde(default)] pub cpps_db: Option<f32>` to `SustainedAnalysis` and `ReadingAnalysis`.

### Modify: `src/analysis/sustained.rs`, `src/analysis/reading.rs`
Compute CPPS and store in result.

### Tests
- 100 Hz sine: high CPPS (strong cepstral peak at quefrency ~10ms)
- White noise: CPPS near 0 or negative
- Silence: returns None (all frames gated)

---

## Phase 3: Session Reliability Header (#7)

Replace `detection_quality: Option<String>` with richer metadata.

### Modify: `src/storage/session_data.rs`

```rust
pub struct MetricsValidity {
    pub jitter: bool,
    pub shimmer: bool,
    pub hnr: bool,
    pub cpps: bool,
    pub voice_breaks: String,  // "valid" | "trend_only" | "unavailable"
}

pub struct ReliabilityInfo {
    pub active_fraction: f32,
    pub pitched_fraction: f32,  // pitched / active
    pub dominant_tier: u8,      // 1, 2, or 3
    pub analysis_quality: String, // "good" | "ok" | "trend_only"
    pub metrics_validity: MetricsValidity,
}
```

Add `#[serde(default)] pub reliability: Option<ReliabilityInfo>` to `SustainedAnalysis` and `ReadingAnalysis`. Keep `detection_quality` for backward compat.

### Modify: `src/dsp/pitch.rs`

Add per-frame tier tracking to `ContourResult`:
```rust
pub frame_tiers: Vec<u8>,    // 1/2/3 per frame
pub tier_counts: [usize; 3], // [tier1_count, tier2_count, tier3_count]
```

### Modify: `src/analysis/sustained.rs`, `src/analysis/reading.rs`

Compute `ReliabilityInfo` from `ContourResult` + `ActivityResult`. Quality rules:
- dominant_tier=1 and pitched_fraction>0.5 -> "good"
- dominant_tier 1|2 and pitched_fraction>0.3 -> "ok"
- else -> "trend_only"

### Modify: `src/analysis/analyzer.rs`
Print reliability summary (quality, active%, pitched%).

### Modify: `src/report/markdown.rs`
Replace "Detection" column with "Quality" column.

### Modify: `src/llm/prompt.rs`
Replace detection_quality warnings with reliability header.

### Modify: `src/report/charts.rs`
Hollow circles for trend_only data points.

### Tests
- Known tier counts -> correct dominant_tier
- Quality threshold edge cases
- Backward compat (old JSON without reliability -> None)

---

## Phase 4: Tighter Bridges + Jitter/Shimmer Gating + Periodicity (#3, #4, #5)

### 4a. Bridge dropouts (#3)

**Modify: `src/dsp/mpt.rs`** — Add `max_bridge_ms` parameter (default 250.0).
**Modify: `src/dsp/voice_breaks.rs`** — Add `max_break_ms` parameter (default 250.0).
**Update callers:** `sustained.rs` and `reading.rs` pass 250.0.

### 4b. Jitter/shimmer gating (#4)

**Modify: `src/dsp/jitter.rs`** — Add `local_jitter_percent_gated()` using `frame_tiers`. Only uses tier 1/2 frames, requires >=15 consecutive frames and >=1.5s total.
**Same for `src/dsp/shimmer.rs`.**
**Modify: `src/analysis/sustained.rs`** — Use gated variants.

### 4c. Periodicity score (#5)

**New file: `src/dsp/periodicity.rs`**

```rust
pub fn compute_periodicity(
    samples: &[f32], sample_rate: u32,
    contour: &[PitchFrame], active_frames: &[bool],
    hop_size_ms: f32,
) -> Option<f32>  // mean normalized autocorrelation peak (0.0-1.0)
```

Reuse `normalized_autocorrelation` from `src/dsp/hnr.rs` (make `pub(crate)`).

**Modify: `src/storage/session_data.rs`** — Add `periodicity_mean: Option<f32>` to `SustainedAnalysis`.

### Bump ANALYSIS_VERSION to 2

After this phase, the analysis pipeline is fundamentally different. Bump `ANALYSIS_VERSION` in `session_data.rs` from 1 to 2. Running `voicevo analyze --all` now re-analyzes all sessions and stores results as version 2 alongside the original version 1 data.

### Tests
- MPT: 200ms bridged, 300ms not bridged
- Jitter gating: sufficient tier-1 data passes, insufficient fails
- Periodicity: sine ~1.0, noise ~0.0

---

## Phase 5: S/Z Ratio + Fatigue Slope (#6)

New exercise protocols. Fully additive.

### New files
- `src/audio/sz_exercise.rs` — S/Z recording (2x /s/, 2x /z/, timer-based)
- `src/audio/fatigue_exercise.rs` — 5x sustained with 45s rest, effort rating

### Modify: `src/cli.rs`
Add `Sz` and `Fatigue` to `ExerciseCommand`.

### Modify: `src/storage/session_data.rs`

```rust
pub struct SzAnalysis {
    pub s_durations: Vec<f32>,
    pub z_durations: Vec<f32>,
    pub mean_s: f32,
    pub mean_z: f32,
    pub sz_ratio: f32,  // > 1.4 = concerning
}

pub struct FatigueAnalysis {
    pub mpt_per_trial: Vec<f32>,
    pub cpps_per_trial: Vec<f32>,
    pub effort_per_trial: Vec<u8>,
    pub mpt_slope: f32,
    pub cpps_slope: f32,
}
```

Add `#[serde(default)] pub sz: Option<SzAnalysis>` and `fatigue: Option<FatigueAnalysis>` to `SessionAnalysis`.

### Helper: `src/util.rs`
Add `linear_regression(points: &[(f32, f32)]) -> (f32, f32)`.

### Modify: `src/main.rs`, `src/audio/mod.rs`
Wire up commands and modules.

### Tests
- S/Z: known durations -> correct ratio
- Fatigue: declining MPT series -> negative slope
- Linear regression: known slope/intercept

---

## Phase 6: Wire Up Output Layers

Final pass — all new metrics surfaced consistently.

### Modify: `src/analysis/analyzer.rs`
Print CPPS, periodicity, reliability summary, voiced_quality.

### Modify: `src/report/charts.rs`
- Add CPPS panel
- Add S/Z ratio panel (when data exists)
- Update VQI composite to include CPPS
- Hollow circles for trend_only points

### Modify: `src/report/markdown.rs`
- CPPS column in sustained/reading tables
- Periodicity column in sustained table
- Voiced_quality in reading table
- S/Z and fatigue tables

### Modify: `src/llm/prompt.rs`
- CPPS clinical reference (normal ~5-10 dB, <3 = significant dysphonia)
- Periodicity, voiced_quality, S/Z ratio, fatigue slope definitions
- Reliability header in user prompt

### Modify: `src/audio/exercise.rs`
Display CPPS in exercise output.

---

## File Impact Summary

| File | Phases |
|------|--------|
| `src/storage/db.rs` (NEW) | 0 |
| `src/dsp/activity.rs` (NEW) | 1 |
| `src/dsp/cpps.rs` (NEW) | 2 |
| `src/dsp/periodicity.rs` (NEW) | 4 |
| `src/audio/sz_exercise.rs` (NEW) | 5 |
| `src/audio/fatigue_exercise.rs` (NEW) | 5 |
| `src/storage/session_data.rs` | 0, 2, 3, 4, 5 |
| `src/storage/store.rs` | 0 |
| `src/config.rs` | 1 |
| `src/dsp/pitch.rs` | 3 |
| `src/dsp/mpt.rs` | 4 |
| `src/dsp/voice_breaks.rs` | 4 |
| `src/dsp/jitter.rs` | 4 |
| `src/dsp/shimmer.rs` | 4 |
| `src/dsp/hnr.rs` | 4 (pub(crate) autocorr) |
| `src/dsp/mod.rs` | 1, 2, 4 |
| `src/analysis/sustained.rs` | 1, 2, 3, 4 |
| `src/analysis/reading.rs` | 1, 2, 3 |
| `src/analysis/analyzer.rs` | 1, 3, 6 |
| `src/cli.rs` | 0, 5 |
| `src/main.rs` | 0, 5 |
| `src/paths.rs` | 0 |
| `src/report/charts.rs` | 3, 6 |
| `src/report/markdown.rs` | 3, 6 |
| `src/llm/prompt.rs` | 3, 6 |
| `src/audio/exercise.rs` | 6 |
| `src/util.rs` | 5 |

## Verification

After each phase:
1. `CARGO_INCREMENTAL=0 cargo test` — all pass
2. `cargo build` — compiles clean
3. `voicevo sessions` — existing data still loads

After Phase 0: `voicevo migrate` converts JSON to SQLite.
After Phase 4: `voicevo analyze --all` re-analyzes at version 2. Both v1 and v2 coexist in DB.
After all phases: `cargo install --path .`, full session, report, explain --deep.
