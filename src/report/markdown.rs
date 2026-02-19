use anyhow::Result;

use crate::config::AppConfig;
use crate::storage::session_data::SessionData;

/// Generate a markdown trend report from a list of sessions.
///
/// Returns the markdown content as a string. The caller decides where to save it.
pub fn generate_report(sessions: &[SessionData], config: &AppConfig) -> Result<String> {
    let mut md = String::new();

    md.push_str("# Voice Recovery — Trend Report\n\n");
    md.push_str(&format!(
        "Generated: {}  \n",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    ));
    md.push_str(&format!("Sessions: {}\n\n", sessions.len()));

    if sessions.is_empty() {
        md.push_str("No sessions to report.\n");
        return Ok(md);
    }

    md.push_str("---\n\n");

    // Sustained vowel metrics table
    md.push_str("## Sustained Vowel Metrics\n\n");
    md.push_str("| Date | MPT (s) | Mean F0 (Hz) | Jitter (%) | Shimmer (%) | HNR (dB) | CPPS (dB) | Periodicity | Quality |\n");
    md.push_str("|------|---------|-------------|-----------|------------|----------|----------|------------|----------|\n");

    let thresholds = &config.analysis.thresholds;

    for session in sessions {
        if let Some(ref s) = session.analysis.sustained {
            let quality = s
                .reliability
                .as_ref()
                .map(|r| r.analysis_quality.as_str())
                .unwrap_or_else(|| s.detection_quality.as_deref().unwrap_or("pitch"));
            let cpps_str = s.cpps_db.map(|c| format!("{c:.1}")).unwrap_or_else(|| "—".into());
            let period_str = s.periodicity_mean.map(|p| format!("{p:.2}")).unwrap_or_else(|| "—".into());
            md.push_str(&format!(
                "| {} | {:.1} | {:.1} | {:.2}{} | {:.2}{} | {:.1}{} | {} | {} | {} |\n",
                session.date,
                s.mpt_seconds,
                s.mean_f0_hz,
                s.jitter_local_percent,
                flag_high(s.jitter_local_percent, thresholds.jitter_pathological),
                s.shimmer_local_percent,
                flag_high(s.shimmer_local_percent, thresholds.shimmer_pathological),
                s.hnr_db,
                flag_low(s.hnr_db, thresholds.hnr_low),
                cpps_str,
                period_str,
                quality,
            ));
        }
    }
    md.push('\n');

    // Scale metrics table
    md.push_str("## Pitch Range (Scale)\n\n");
    md.push_str("| Date | Floor (Hz) | Ceiling (Hz) | Range (Hz) | Semitones |\n");
    md.push_str("|------|-----------|-------------|-----------|----------|\n");

    for session in sessions {
        if let Some(ref s) = session.analysis.scale {
            md.push_str(&format!(
                "| {} | {:.1} | {:.1} | {:.1} | {:.1} |\n",
                session.date,
                s.pitch_floor_hz,
                s.pitch_ceiling_hz,
                s.range_hz,
                s.range_semitones,
            ));
        }
    }
    md.push('\n');

    // Reading metrics table
    md.push_str("## Reading Passage Metrics\n\n");
    md.push_str("| Date | Mean F0 (Hz) | F0 Std (Hz) | Breaks | Voiced (%) | CPPS (dB) | Quality |\n");
    md.push_str("|------|-------------|-----------|--------|----------|----------|----------|\n");

    for session in sessions {
        if let Some(ref r) = session.analysis.reading {
            let quality = r
                .reliability
                .as_ref()
                .map(|r| r.analysis_quality.as_str())
                .unwrap_or_else(|| r.detection_quality.as_deref().unwrap_or("pitch"));
            let cpps_str = r.cpps_db.map(|c| format!("{c:.1}")).unwrap_or_else(|| "—".into());
            md.push_str(&format!(
                "| {} | {:.1} | {:.1} | {} | {:.0} | {} | {} |\n",
                session.date,
                r.mean_f0_hz,
                r.f0_std_hz,
                r.voice_breaks,
                r.voiced_fraction * 100.0,
                cpps_str,
                quality,
            ));
        }
    }
    md.push('\n');

    // S/Z ratio table
    let has_sz = sessions.iter().any(|s| s.analysis.sz.is_some());
    if has_sz {
        md.push_str("## S/Z Ratio\n\n");
        md.push_str("| Date | Mean /s/ (s) | Mean /z/ (s) | S/Z Ratio |\n");
        md.push_str("|------|-------------|-------------|----------|\n");

        for session in sessions {
            if let Some(ref sz) = session.analysis.sz {
                md.push_str(&format!(
                    "| {} | {:.1} | {:.1} | {:.2}{} |\n",
                    session.date,
                    sz.mean_s,
                    sz.mean_z,
                    sz.sz_ratio,
                    if sz.sz_ratio > 1.4 { " \u{26a0}" } else { "" },
                ));
            }
        }
        md.push('\n');
    }

    // Fatigue slope table
    let has_fatigue = sessions.iter().any(|s| s.analysis.fatigue.is_some());
    if has_fatigue {
        md.push_str("## Vocal Fatigue\n\n");
        md.push_str("| Date | Trials | MPT Slope (s/trial) | CPPS Slope (dB/trial) |\n");
        md.push_str("|------|--------|--------------------|-----------------------|\n");

        for session in sessions {
            if let Some(ref f) = session.analysis.fatigue {
                md.push_str(&format!(
                    "| {} | {} | {:+.2} | {:+.2} |\n",
                    session.date,
                    f.mpt_per_trial.len(),
                    f.mpt_slope,
                    f.cpps_slope,
                ));
            }
        }
        md.push('\n');
    }

    // Trend interpretation
    if sessions.len() >= 2 {
        md.push_str("## Trends\n\n");

        let first = &sessions[0];
        let last = &sessions[sessions.len() - 1];

        if let (Some(ref f_s), Some(ref l_s)) =
            (&first.analysis.sustained, &last.analysis.sustained)
        {
            let hnr_delta = l_s.hnr_db - f_s.hnr_db;
            let direction = if hnr_delta > 0.0 {
                "improved"
            } else {
                "decreased"
            };
            md.push_str(&format!(
                "- **HNR** {} from {:.1} to {:.1} dB ({:+.1} dB) — breathiness is {}.\n",
                direction,
                f_s.hnr_db,
                l_s.hnr_db,
                hnr_delta,
                if hnr_delta > 0.0 {
                    "decreasing"
                } else {
                    "increasing"
                },
            ));

            let mpt_delta = l_s.mpt_seconds - f_s.mpt_seconds;
            md.push_str(&format!(
                "- **MPT** changed from {:.1}s to {:.1}s ({:+.1}s).\n",
                f_s.mpt_seconds, l_s.mpt_seconds, mpt_delta,
            ));

            let jitter_delta = l_s.jitter_local_percent - f_s.jitter_local_percent;
            md.push_str(&format!(
                "- **Jitter** went from {:.2}% to {:.2}% ({:+.2}%).\n",
                f_s.jitter_local_percent, l_s.jitter_local_percent, jitter_delta,
            ));

            if let (Some(f_cpps), Some(l_cpps)) = (f_s.cpps_db, l_s.cpps_db) {
                let cpps_delta = l_cpps - f_cpps;
                md.push_str(&format!(
                    "- **CPPS** went from {:.1} to {:.1} dB ({:+.1} dB).\n",
                    f_cpps, l_cpps, cpps_delta,
                ));
            }
        }

        if let (Some(ref f_sc), Some(ref l_sc)) = (&first.analysis.scale, &last.analysis.scale) {
            let range_delta = l_sc.range_semitones - f_sc.range_semitones;
            md.push_str(&format!(
                "- **Pitch range** went from {:.1} to {:.1} semitones ({:+.1}).\n",
                f_sc.range_semitones, l_sc.range_semitones, range_delta,
            ));
        }

        md.push('\n');
    }

    Ok(md)
}

/// Append a flag marker if value exceeds threshold (higher is worse).
fn flag_high(value: f32, threshold: f32) -> &'static str {
    if value > threshold {
        " \u{26a0}" // ⚠
    } else {
        ""
    }
}

/// Append a flag marker if value is below threshold (lower is worse).
fn flag_low(value: f32, threshold: f32) -> &'static str {
    if value < threshold {
        " \u{26a0}" // ⚠
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::session_data::*;

    fn sample_session(date: &str, hnr: f32, mpt: f32) -> SessionData {
        SessionData {
            date: date.into(),
            recordings: SessionRecordings {
                sustained: Some(format!("data/recordings/{date}/sustained.wav")),
                scale: None,
                reading: None,
            },
            analysis: SessionAnalysis {
                sustained: Some(SustainedAnalysis {
                    mpt_seconds: mpt,
                    mean_f0_hz: 100.0,
                    f0_std_hz: 3.0,
                    jitter_local_percent: 1.5,
                    shimmer_local_percent: 4.0,
                    hnr_db: hnr,
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
        }
    }

    #[test]
    fn generates_markdown_with_tables() {
        let sessions = vec![
            sample_session("2026-02-01", 8.0, 5.0),
            sample_session("2026-02-08", 12.0, 7.0),
        ];

        let config = AppConfig::default();
        let md = generate_report(&sessions, &config).unwrap();

        assert!(md.contains("Voice Recovery"));
        assert!(md.contains("2026-02-01"));
        assert!(md.contains("2026-02-08"));
        assert!(md.contains("Sustained Vowel"));
    }

    #[test]
    fn trends_show_improvement() {
        let sessions = vec![
            sample_session("2026-02-01", 8.0, 5.0),
            sample_session("2026-02-08", 12.0, 7.0),
        ];

        let config = AppConfig::default();
        let md = generate_report(&sessions, &config).unwrap();

        assert!(md.contains("improved"));
        assert!(md.contains("breathiness is decreasing"));
    }

    #[test]
    fn empty_sessions() {
        let config = AppConfig::default();
        let md = generate_report(&[], &config).unwrap();
        assert!(md.contains("No sessions"));
    }
}
