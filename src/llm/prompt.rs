use crate::storage::session_data::{ReliabilityInfo, SessionData};

/// The system prompt that gives the LLM medical and acoustic context.
/// This never changes between calls — it defines the role and domain knowledge.
pub fn system_prompt() -> String {
    r#"You are a voice recovery specialist assistant. You interpret objective acoustic measurements from a patient recovering from left vocal cord paralysis caused by radiation therapy.

## Medical context

Left vocal cord paralysis means the left vocal fold cannot adduct (close) fully during phonation. This causes:
- Breathy voice (air escapes through the glottal gap)
- Higher than normal pitch (the cord is thin and stiff)
- Reduced maximum phonation time (air escapes too fast)
- Voice breaks during connected speech
- Low harmonic-to-noise ratio (turbulent airflow adds noise)

Recovery is gradual over months. Signs of improvement include:
- Pitch dropping toward the patient's natural range
- HNR increasing (less breathiness)
- Jitter and shimmer decreasing (more stable vibration)
- MPT increasing (better cord closure retains air)
- Fewer voice breaks in connected speech
- Higher voiced fraction in reading passages

## Metric definitions

### Sustained vowel ("AAAH")
- **MPT** (Maximum Phonation Time): how long the patient can sustain a vowel. Healthy adults: 15-25 seconds. Below 10 seconds suggests significant cord dysfunction.
- **Mean F0**: average fundamental frequency. Healthy adult males: 85-180 Hz, females: 165-255 Hz. Vocal cord paralysis often pushes F0 above the normal range.
- **F0 std**: pitch stability. Lower is more stable. Below 5 Hz is very stable.
- **Jitter**: cycle-to-cycle pitch variation (%). Normal < 1.04%. Higher suggests irregular cord vibration.
- **Shimmer**: cycle-to-cycle amplitude variation (%). Normal < 3.81%. Higher suggests inconsistent cord closure.
- **HNR** (Harmonic-to-Noise Ratio): signal quality in dB. Normal > 20 dB. Below 7 dB is severely breathy.
- **CPPS** (Cepstral Peak Prominence Smoothed): pitch-independent measure of voice periodicity in dB. Normal ~5-10 dB. Below 3 dB indicates significant dysphonia. Unlike HNR, CPPS remains valid even when pitch detection fails, making it especially useful for severely damaged voices.
- **Periodicity**: mean normalized autocorrelation at the pitch period (0.0-1.0). Higher values mean more regular vocal fold vibration. Below 0.5 suggests highly aperiodic voice.

### Chromatic scale (low to high and back)
- **Pitch floor/ceiling**: the usable range (5th-95th percentile of detected pitch)
- **Range**: total span in Hz and semitones. Healthy adults: 24-36 semitones. Reduced range suggests cord stiffness.

### Reading passage
- **Mean F0**: speaking pitch during connected speech
- **F0 std**: intonation variation (higher = more expressive)
- **Voice breaks**: pauses in voicing between 50-250ms. These indicate moments where the cord cannot sustain vibration.
- **Voiced fraction**: percentage of speech that is actually voiced. Healthy speakers: 60-80%. Low values indicate frequent voicing failures.
- **CPPS**: same as sustained vowel — pitch-independent periodicity metric.

### S/Z ratio
- The patient sustains /s/ (voiceless) and /z/ (voiced) as long as possible. Since /z/ requires vocal fold vibration, the ratio of /s/ duration to /z/ duration indicates glottal efficiency.
- **Normal**: ratio close to 1.0 (both durations similar)
- **Elevated** (>1.4): suggests glottal air leak — the vocal folds cannot maintain closure during voiced sound, so /z/ duration is disproportionately short.

### Vocal fatigue
- The patient performs multiple sustained vowel trials with rest periods. We track MPT and CPPS across trials.
- **MPT slope**: negative slope means phonation time decreases with repetition (vocal fatigue). Stable or positive slope indicates good endurance.
- **CPPS slope**: declining CPPS across trials suggests voice quality degrades with use.
- **Effort rating**: patient-reported strain (1-10) per trial. Increasing effort with stable MPT suggests compensatory strategies.

## Detection quality and reliability

Each exercise includes reliability metadata indicating how trustworthy the measurements are:

### Analysis quality levels
- **good**: Standard pitch detection worked well (dominant tier 1, >50% pitched frames). All metrics are reliable.
- **ok**: Pitch detection needed relaxed thresholds for some frames (dominant tier 1-2, >30% pitched). Metrics are usable but less precise.
- **trend_only**: Pitch detection largely failed (dominant tier 3, or very few pitched frames). Only useful for tracking relative changes between sessions — absolute values are unreliable.

### Per-metric validity
Each session indicates which specific metrics are trustworthy:
- **Jitter**: requires tier 1-2 detection with >30% pitched frames
- **Shimmer**: requires tier 1-2 detection
- **HNR**: requires tier 1-2 detection
- **CPPS**: always valid when computed (pitch-independent)
- **Voice breaks**: "valid" (tier 1), "trend_only" (tier 2), or "unavailable" (tier 3)

### Legacy detection_quality field
Older sessions may show a `detection_quality` field instead:
- **pitch** (or absent): Standard pitch detection — metrics are reliable.
- **relaxed_pitch**: Lowered thresholds — usable but noisier.
- **energy_fallback**: Pitch failed entirely — jitter zeroed, voice breaks zeroed, shimmer/HNR/F0 are estimates only.

When comparing sessions, pay attention to quality levels. Do not compare a "good" session's jitter against a "trend_only" session's jitter — the latter is unreliable.

## Your task

Interpret the data concisely. Use plain language — the patient is not a clinician. Structure your response as:
1. What the numbers mean in practical terms (how does the voice sound today?)
2. What to watch for in future sessions
3. **End with the central question: is the voice improving over time?** Compare trends across sessions — pitch stability, HNR, MPT, voice breaks, range. Be honest about the direction. If things are improving, say so clearly. If they're stagnating or regressing, say that too — the patient wants truth, not comfort.
   - If no historical data is available, say so and explain that tracking trends requires multiple sessions.

## Recording conditions

Sessions may include self-reported recording conditions (time of day, fatigue, hydration, mucus, whether the throat was cleared). These significantly affect voice quality:
- Morning voice is typically rougher (vocal folds are dehydrated and stiff from sleep)
- High fatigue reduces vocal endurance and increases jitter/shimmer
- High mucus can dampen vibration but also add mass (lowers pitch)
- Low hydration increases friction and reduces phonation time
- Clearing the throat before recording can temporarily improve clarity

When conditions are available, factor them into your interpretation. For example, worse metrics on a high-fatigue morning session may not indicate regression — compare against sessions with similar conditions when possible.

Keep it to 2-3 short paragraphs. Don't repeat the raw numbers back — the patient already sees them in the terminal output."#
        .to_string()
}

/// Build the user message from the current session and optional history.
/// Formats the data as readable text rather than raw JSON so the LLM
/// can focus on interpretation rather than parsing.
pub fn user_prompt(current: &SessionData, history: &[SessionData], trend_report: Option<&str>) -> String {
    let mut parts = Vec::new();

    parts.push(format!("## Current session: {}", current.date));
    parts.push(String::new());

    if let Some(c) = &current.conditions {
        parts.push("### Recording conditions".into());
        parts.push(format!("- Time of day: {}", c.time_of_day));
        parts.push(format!("- Fatigue: {}/10", c.fatigue_level));
        parts.push(format!("- Throat cleared: {}", if c.throat_cleared { "yes" } else { "no" }));
        parts.push(format!("- Mucus level: {}", c.mucus_level));
        parts.push(format!("- Hydration: {}", c.hydration));
        if let Some(ref notes) = c.notes {
            parts.push(format!("- Notes: {notes}"));
        }
        parts.push(String::new());
    }

    if let Some(s) = &current.analysis.sustained {
        parts.push("### Sustained vowel".into());
        push_reliability_header(&mut parts, s.reliability.as_ref(), s.detection_quality.as_deref());
        parts.push(format!("- MPT: {:.1} seconds", s.mpt_seconds));
        parts.push(format!("- Mean F0: {:.1} Hz", s.mean_f0_hz));
        parts.push(format!("- F0 std: {:.1} Hz", s.f0_std_hz));
        parts.push(format!("- Jitter: {:.2}%{}", s.jitter_local_percent,
            jitter_caveat(s.reliability.as_ref(), s.detection_quality.as_deref())));
        parts.push(format!("- Shimmer: {:.2}%", s.shimmer_local_percent));
        parts.push(format!("- HNR: {:.1} dB", s.hnr_db));
        if let Some(cpps) = s.cpps_db {
            parts.push(format!("- CPPS: {:.1} dB", cpps));
        }
        if let Some(p) = s.periodicity_mean {
            parts.push(format!("- Periodicity: {:.2}", p));
        }
        parts.push(String::new());
    }

    if let Some(s) = &current.analysis.scale {
        parts.push("### Chromatic scale".into());
        parts.push(format!("- Pitch floor: {:.1} Hz", s.pitch_floor_hz));
        parts.push(format!("- Pitch ceiling: {:.1} Hz", s.pitch_ceiling_hz));
        parts.push(format!("- Range: {:.1} Hz ({:.1} semitones)", s.range_hz, s.range_semitones));
        parts.push(String::new());
    }

    if let Some(s) = &current.analysis.reading {
        parts.push("### Reading passage".into());
        push_reliability_header(&mut parts, s.reliability.as_ref(), s.detection_quality.as_deref());
        parts.push(format!("- Mean F0: {:.1} Hz", s.mean_f0_hz));
        parts.push(format!("- F0 std: {:.1} Hz", s.f0_std_hz));
        parts.push(format!("- F0 range: {:.1} - {:.1} Hz", s.f0_range_hz.0, s.f0_range_hz.1));
        parts.push(format!("- Voice breaks: {}{}", s.voice_breaks,
            voice_breaks_caveat(s.reliability.as_ref(), s.detection_quality.as_deref())));
        parts.push(format!("- Voiced fraction: {:.0}%", s.voiced_fraction * 100.0));
        if let Some(cpps) = s.cpps_db {
            parts.push(format!("- CPPS: {:.1} dB", cpps));
        }
        parts.push(String::new());
    }

    if let Some(sz) = &current.analysis.sz {
        parts.push("### S/Z ratio".into());
        parts.push(format!("- Mean /s/: {:.1}s", sz.mean_s));
        parts.push(format!("- Mean /z/: {:.1}s", sz.mean_z));
        parts.push(format!("- S/Z ratio: {:.2}{}", sz.sz_ratio,
            if sz.sz_ratio > 1.4 { " (elevated — possible glottal air leak)" } else { " (normal)" }));
        parts.push(String::new());
    }

    if let Some(f) = &current.analysis.fatigue {
        parts.push("### Vocal fatigue".into());
        for (i, mpt) in f.mpt_per_trial.iter().enumerate() {
            let cpps_str = f.cpps_per_trial.get(i)
                .and_then(|c| *c)
                .map(|c| format!(", CPPS={c:.1}dB"))
                .unwrap_or_default();
            parts.push(format!("- Trial {}: MPT={:.1}s{}, effort={}",
                i + 1, mpt, cpps_str, f.effort_per_trial[i]));
        }
        parts.push(format!("- MPT slope: {:+.2} s/trial{}", f.mpt_slope,
            if f.mpt_slope < -0.3 { " (declining — vocal fatigue)" }
            else if f.mpt_slope > 0.3 { " (improving — warming up)" }
            else { " (stable — good endurance)" }));
        if f.cpps_slope != 0.0 {
            parts.push(format!("- CPPS slope: {:+.2} dB/trial", f.cpps_slope));
        }
        parts.push(String::new());
    }

    // Add pre-computed trend report if available
    if let Some(report) = trend_report {
        parts.push("## Trend Report (pre-computed)".into());
        parts.push(String::new());
        parts.push(report.to_string());
        parts.push(String::new());
    }

    // Add historical context if we have prior sessions
    if !history.is_empty() {
        parts.push(format!("## History ({} prior session{})", history.len(), if history.len() == 1 { "" } else { "s" }));
        parts.push(String::new());

        for session in history {
            parts.push(format!("### {}", session.date));

            if let Some(c) = &session.conditions {
                let mut cond_parts = vec![
                    c.time_of_day.clone(),
                    format!("fatigue={}", c.fatigue_level),
                    format!("mucus={}", c.mucus_level),
                    format!("hydration={}", c.hydration),
                ];
                if c.throat_cleared {
                    cond_parts.push("throat_cleared".into());
                }
                parts.push(format!("  Conditions: {}", cond_parts.join(", ")));
            }

            if let Some(s) = &session.analysis.sustained {
                let quality_tag = quality_tag(s.reliability.as_ref(), s.detection_quality.as_deref());
                parts.push(format!(
                    "  Sustained: MPT={:.1}s, F0={:.1}Hz, Jitter={:.2}%, Shimmer={:.2}%, HNR={:.1}dB{}{}",
                    s.mpt_seconds, s.mean_f0_hz, s.jitter_local_percent, s.shimmer_local_percent, s.hnr_db,
                    s.cpps_db.map(|c| format!(", CPPS={c:.1}dB")).unwrap_or_default(),
                    quality_tag,
                ));
            }

            if let Some(s) = &session.analysis.scale {
                parts.push(format!(
                    "  Scale: {:.1}-{:.1}Hz ({:.1} semitones)",
                    s.pitch_floor_hz, s.pitch_ceiling_hz, s.range_semitones
                ));
            }

            if let Some(s) = &session.analysis.reading {
                let quality_tag = quality_tag(s.reliability.as_ref(), s.detection_quality.as_deref());
                parts.push(format!(
                    "  Reading: F0={:.1}Hz, breaks={}, voiced={:.0}%{}{}",
                    s.mean_f0_hz, s.voice_breaks, s.voiced_fraction * 100.0,
                    s.cpps_db.map(|c| format!(", CPPS={c:.1}dB")).unwrap_or_default(),
                    quality_tag,
                ));
            }

            if let Some(sz) = &session.analysis.sz {
                parts.push(format!(
                    "  S/Z: ratio={:.2}, /s/={:.1}s, /z/={:.1}s",
                    sz.sz_ratio, sz.mean_s, sz.mean_z,
                ));
            }

            if let Some(f) = &session.analysis.fatigue {
                parts.push(format!(
                    "  Fatigue: MPT slope={:+.2}s/trial, CPPS slope={:+.2}dB/trial, {} trials",
                    f.mpt_slope, f.cpps_slope, f.mpt_per_trial.len(),
                ));
            }

            parts.push(String::new());
        }
    }

    parts.join("\n")
}

/// System prompt for the synthesis/fact-check pass.
/// This model sees the raw data AND both interpretations, and must
/// find consensus, flag disagreements, and verify claims against the numbers.
pub fn synthesis_system_prompt() -> String {
    r#"You are a medical data analyst reviewing two independent AI interpretations of voice recovery data. You have access to the raw measurements.

Your job:
1. **Consensus**: What do both interpretations agree on? Lead with this.
2. **Disagreements**: Where do they differ? Who's more accurate given the data?
3. **Fact-check**: Verify any specific claims against the raw numbers. Flag anything that's wrong or misleading.
4. **The central question — is the voice improving over time?** End with a clear, honest verdict based on the data trends. If historical data is available, compare key metrics across sessions. If not, state that a single session can't answer this question yet.

Be concise — 2-3 short paragraphs. Use plain language. Don't hedge excessively. If one interpretation is clearly better, say so."#
        .to_string()
}

/// Build the user message for the synthesis pass.
/// Includes the raw data and both interpretations so the synthesizer
/// can fact-check against the actual numbers.
pub fn synthesis_user_prompt(
    current: &SessionData,
    history: &[SessionData],
    claude_response: &str,
    gpt_response: &str,
) -> String {
    let data = user_prompt(current, history, None);

    format!(
        "## Raw measurement data\n\n{data}\n\n\
         ---\n\n\
         ## Interpretation A (Claude)\n\n{claude_response}\n\n\
         ---\n\n\
         ## Interpretation B (GPT)\n\n{gpt_response}"
    )
}

/// Push a reliability or detection_quality header line into the prompt parts.
fn push_reliability_header(parts: &mut Vec<String>, rel: Option<&ReliabilityInfo>, dq: Option<&str>) {
    if let Some(r) = rel {
        let validity_notes: Vec<&str> = [
            (!r.metrics_validity.jitter).then_some("jitter unreliable"),
            (!r.metrics_validity.shimmer).then_some("shimmer unreliable"),
            (!r.metrics_validity.hnr).then_some("HNR unreliable"),
            (r.metrics_validity.voice_breaks != "valid").then_some("voice breaks approximate"),
        ]
        .into_iter()
        .flatten()
        .collect();

        let note = if validity_notes.is_empty() {
            String::new()
        } else {
            format!(" ({})", validity_notes.join(", "))
        };
        parts.push(format!(
            "- **Quality: {}** — {:.0}% active, {:.0}% pitched, tier {}{}",
            r.analysis_quality,
            r.active_fraction * 100.0,
            r.pitched_fraction * 100.0,
            r.dominant_tier,
            note,
        ));
    } else if let Some(dq) = dq {
        parts.push(format!("- **Detection: {dq}** — pitch detector struggled; metrics should be interpreted with caution"));
    }
}

/// Caveat for jitter values based on reliability.
fn jitter_caveat(rel: Option<&ReliabilityInfo>, dq: Option<&str>) -> &'static str {
    if let Some(r) = rel {
        if !r.metrics_validity.jitter {
            return " (unreliable — insufficient pitched frames)";
        }
    } else if dq == Some("energy_fallback") {
        return " (zeroed — unreliable with energy fallback)";
    }
    ""
}

/// Caveat for voice breaks based on reliability.
fn voice_breaks_caveat(rel: Option<&ReliabilityInfo>, dq: Option<&str>) -> &'static str {
    if let Some(r) = rel {
        match r.metrics_validity.voice_breaks.as_str() {
            "trend_only" => return " (approximate — trend only)",
            "unavailable" => return " (unavailable — detection too noisy)",
            _ => {}
        }
    } else if dq == Some("energy_fallback") {
        return " (zeroed — unreliable with energy fallback)";
    }
    ""
}

/// Build a compact quality tag for history lines.
fn quality_tag(rel: Option<&ReliabilityInfo>, dq: Option<&str>) -> String {
    if let Some(r) = rel {
        format!(" [quality: {}]", r.analysis_quality)
    } else {
        match dq {
            Some(dq) => format!(" [detection: {dq}]"),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::session_data::*;

    fn sample_session(date: &str) -> SessionData {
        SessionData {
            date: date.into(),
            recordings: SessionRecordings {
                sustained: Some("test.wav".into()),
                scale: None,
                reading: None,
            },
            analysis: SessionAnalysis {
                sustained: Some(SustainedAnalysis {
                    mpt_seconds: 8.0,
                    mean_f0_hz: 645.0,
                    f0_std_hz: 11.0,
                    jitter_local_percent: 0.28,
                    shimmer_local_percent: 75.0,
                    hnr_db: -0.9,
                    cpps_db: None,
                    periodicity_mean: None,
                    detection_quality: None,
                    reliability: None,
                }),
                scale: None,
                reading: None,
                sz: None,
                fatigue: None,
            },
            conditions: None,
        }
    }

    #[test]
    fn system_prompt_contains_key_concepts() {
        let prompt = system_prompt();
        assert!(prompt.contains("vocal cord paralysis"));
        assert!(prompt.contains("HNR"));
        assert!(prompt.contains("Jitter"));
        assert!(prompt.contains("MPT"));
    }

    #[test]
    fn user_prompt_includes_current_data() {
        let session = sample_session("2026-02-15");
        let prompt = user_prompt(&session, &[], None);
        assert!(prompt.contains("2026-02-15"));
        assert!(prompt.contains("645.0 Hz"));
        assert!(prompt.contains("8.0 seconds"));
    }

    #[test]
    fn user_prompt_includes_history() {
        let current = sample_session("2026-02-22");
        let history = vec![sample_session("2026-02-15")];
        let prompt = user_prompt(&current, &history, None);
        assert!(prompt.contains("History (1 prior session)"));
        assert!(prompt.contains("2026-02-15"));
    }

    #[test]
    fn user_prompt_no_history_section_when_empty() {
        let current = sample_session("2026-02-15");
        let prompt = user_prompt(&current, &[], None);
        assert!(!prompt.contains("History"));
    }

    #[test]
    fn user_prompt_includes_trend_report() {
        let session = sample_session("2026-02-15");
        let prompt = user_prompt(&session, &[], Some("MPT improving +2.1s"));
        assert!(prompt.contains("Trend Report (pre-computed)"));
        assert!(prompt.contains("MPT improving +2.1s"));
    }

    #[test]
    fn user_prompt_no_trend_section_when_none() {
        let session = sample_session("2026-02-15");
        let prompt = user_prompt(&session, &[], None);
        assert!(!prompt.contains("Trend Report"));
    }

    #[test]
    fn synthesis_prompt_contains_instructions() {
        let prompt = synthesis_system_prompt();
        assert!(prompt.contains("Consensus"));
        assert!(prompt.contains("Fact-check"));
    }

    #[test]
    fn synthesis_user_prompt_includes_all_parts() {
        let session = sample_session("2026-02-15");
        let prompt = synthesis_user_prompt(
            &session,
            &[],
            "Claude says something",
            "GPT says something else",
        );
        assert!(prompt.contains("Raw measurement data"));
        assert!(prompt.contains("645.0 Hz"));
        assert!(prompt.contains("Claude says something"));
        assert!(prompt.contains("GPT says something else"));
        assert!(prompt.contains("Interpretation A"));
        assert!(prompt.contains("Interpretation B"));
    }

    #[test]
    fn user_prompt_includes_conditions_when_present() {
        let mut session = sample_session("2026-02-15");
        session.conditions = Some(RecordingConditions {
            time_of_day: "morning".into(),
            fatigue_level: 7,
            throat_cleared: true,
            mucus_level: "high".into(),
            hydration: "low".into(),
            notes: Some("bad night".into()),
        });
        let prompt = user_prompt(&session, &[], None);
        assert!(prompt.contains("### Recording conditions"));
        assert!(prompt.contains("Time of day: morning"));
        assert!(prompt.contains("Fatigue: 7/10"));
        assert!(prompt.contains("Throat cleared: yes"));
        assert!(prompt.contains("Mucus level: high"));
        assert!(prompt.contains("Hydration: low"));
        assert!(prompt.contains("Notes: bad night"));
    }

    #[test]
    fn user_prompt_omits_conditions_when_none() {
        let session = sample_session("2026-02-15");
        let prompt = user_prompt(&session, &[], None);
        assert!(!prompt.contains("Recording conditions"));
    }

    #[test]
    fn user_prompt_history_includes_conditions() {
        let current = sample_session("2026-02-22");
        let mut past = sample_session("2026-02-15");
        past.conditions = Some(RecordingConditions {
            time_of_day: "evening".into(),
            fatigue_level: 3,
            throat_cleared: false,
            mucus_level: "low".into(),
            hydration: "high".into(),
            notes: None,
        });
        let prompt = user_prompt(&current, &[past], None);
        assert!(prompt.contains("Conditions: evening, fatigue=3, mucus=low, hydration=high"));
        assert!(!prompt.contains("throat_cleared"));
    }

    #[test]
    fn system_prompt_mentions_conditions() {
        let prompt = system_prompt();
        assert!(prompt.contains("Recording conditions"));
        assert!(prompt.contains("fatigue"));
        assert!(prompt.contains("hydration"));
    }
}
