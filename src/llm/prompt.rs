use crate::storage::session_data::SessionData;

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

### Chromatic scale (low to high and back)
- **Pitch floor/ceiling**: the usable range (5th-95th percentile of detected pitch)
- **Range**: total span in Hz and semitones. Healthy adults: 24-36 semitones. Reduced range suggests cord stiffness.

### Reading passage
- **Mean F0**: speaking pitch during connected speech
- **F0 std**: intonation variation (higher = more expressive)
- **Voice breaks**: pauses in voicing between 50-500ms. These indicate moments where the cord cannot sustain vibration.
- **Voiced fraction**: percentage of speech that is actually voiced. Healthy speakers: 60-80%. Low values indicate frequent voicing failures.

## Detection quality

Each exercise may include a `detection_quality` field indicating how voiced frames were identified:
- **pitch** (or absent): Standard pitch detection — metrics are reliable.
- **relaxed_pitch**: Pitch detection with lowered thresholds — still real pitch measurements but noisier. Metrics are usable but less precise.
- **energy_fallback**: Pitch detector found almost no voiced frames (very breathy voice). Voiced frames were identified by signal energy instead. Consequences:
  - **Jitter** is zeroed (meaningless without real pitch measurements)
  - **Voice breaks** are zeroed (energy gaps ≠ voicing gaps)
  - **Shimmer** and **HNR** use an estimated pitch for window sizing — interpret with caution
  - **F0 values** are estimated, not measured — do not draw conclusions about pitch

When comparing sessions, flag any that used energy_fallback — their metrics are not directly comparable to pitch-detected sessions.

## Your task

Interpret the data concisely. Use plain language — the patient is not a clinician. Structure your response as:
1. What the numbers mean in practical terms (how does the voice sound today?)
2. What to watch for in future sessions
3. **End with the central question: is the voice improving over time?** Compare trends across sessions — pitch stability, HNR, MPT, voice breaks, range. Be honest about the direction. If things are improving, say so clearly. If they're stagnating or regressing, say that too — the patient wants truth, not comfort.
   - If no historical data is available, say so and explain that tracking trends requires multiple sessions.

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

    if let Some(s) = &current.analysis.sustained {
        parts.push("### Sustained vowel".into());
        if let Some(ref dq) = s.detection_quality {
            parts.push(format!("- **Detection: {dq}** — pitch detector struggled; metrics below use estimated pitch and should be interpreted with caution"));
        }
        parts.push(format!("- MPT: {:.1} seconds", s.mpt_seconds));
        parts.push(format!("- Mean F0: {:.1} Hz", s.mean_f0_hz));
        parts.push(format!("- F0 std: {:.1} Hz", s.f0_std_hz));
        parts.push(format!("- Jitter: {:.2}%{}", s.jitter_local_percent,
            if s.detection_quality.as_deref() == Some("energy_fallback") { " (zeroed — unreliable with energy fallback)" } else { "" }));
        parts.push(format!("- Shimmer: {:.2}%", s.shimmer_local_percent));
        parts.push(format!("- HNR: {:.1} dB", s.hnr_db));
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
        if let Some(ref dq) = s.detection_quality {
            parts.push(format!("- **Detection: {dq}** — pitch detector struggled; metrics below should be interpreted with caution"));
        }
        parts.push(format!("- Mean F0: {:.1} Hz", s.mean_f0_hz));
        parts.push(format!("- F0 std: {:.1} Hz", s.f0_std_hz));
        parts.push(format!("- F0 range: {:.1} - {:.1} Hz", s.f0_range_hz.0, s.f0_range_hz.1));
        parts.push(format!("- Voice breaks: {}{}", s.voice_breaks,
            if s.detection_quality.as_deref() == Some("energy_fallback") { " (zeroed — unreliable with energy fallback)" } else { "" }));
        parts.push(format!("- Voiced fraction: {:.0}%", s.voiced_fraction * 100.0));
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

            if let Some(s) = &session.analysis.sustained {
                let dq_tag = match s.detection_quality.as_deref() {
                    Some(dq) => format!(" [detection: {dq}]"),
                    None => String::new(),
                };
                parts.push(format!(
                    "  Sustained: MPT={:.1}s, F0={:.1}Hz, Jitter={:.2}%, Shimmer={:.2}%, HNR={:.1}dB{dq_tag}",
                    s.mpt_seconds, s.mean_f0_hz, s.jitter_local_percent, s.shimmer_local_percent, s.hnr_db
                ));
            }

            if let Some(s) = &session.analysis.scale {
                parts.push(format!(
                    "  Scale: {:.1}-{:.1}Hz ({:.1} semitones)",
                    s.pitch_floor_hz, s.pitch_ceiling_hz, s.range_semitones
                ));
            }

            if let Some(s) = &session.analysis.reading {
                let dq_tag = match s.detection_quality.as_deref() {
                    Some(dq) => format!(" [detection: {dq}]"),
                    None => String::new(),
                };
                parts.push(format!(
                    "  Reading: F0={:.1}Hz, breaks={}, voiced={:.0}%{dq_tag}",
                    s.mean_f0_hz, s.voice_breaks, s.voiced_fraction * 100.0
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
                    detection_quality: None,
                }),
                scale: None,
                reading: None,
            },
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
}
