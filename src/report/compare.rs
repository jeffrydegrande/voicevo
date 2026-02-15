use anyhow::Result;
use console::style;

use crate::storage::session_data::SessionData;
use crate::storage::store;

/// Compare two sessions side by side and print the results.
pub fn compare_sessions(baseline_date: &str, current_date: &str) -> Result<()> {
    let baseline = store::load_session(baseline_date)?;
    let current = store::load_session(current_date)?;

    println!(
        "{}",
        style("=== Session Comparison ===").bold()
    );
    println!();
    println!(
        "  Baseline: {}    Current: {}",
        style(baseline_date).cyan(),
        style(current_date).cyan()
    );
    println!();

    // Sustained vowel comparison
    if let (Some(ref b), Some(ref c)) =
        (&baseline.analysis.sustained, &current.analysis.sustained)
    {
        println!("{}", style("  Sustained Vowel").bold());
        print_comparison("    MPT", b.mpt_seconds, c.mpt_seconds, "s", true);
        print_comparison("    Mean F0", b.mean_f0_hz, c.mean_f0_hz, "Hz", true);
        print_comparison("    F0 std", b.f0_std_hz, c.f0_std_hz, "Hz", false);
        print_comparison("    Jitter", b.jitter_local_percent, c.jitter_local_percent, "%", false);
        print_comparison("    Shimmer", b.shimmer_local_percent, c.shimmer_local_percent, "%", false);
        print_comparison("    HNR", b.hnr_db, c.hnr_db, "dB", true);
        println!();
    } else {
        print_missing("Sustained", &baseline, &current);
    }

    // Scale comparison
    if let (Some(ref b), Some(ref c)) = (&baseline.analysis.scale, &current.analysis.scale) {
        println!("{}", style("  Pitch Range (Scale)").bold());
        print_comparison("    Floor", b.pitch_floor_hz, c.pitch_floor_hz, "Hz", false);
        print_comparison("    Ceiling", b.pitch_ceiling_hz, c.pitch_ceiling_hz, "Hz", true);
        print_comparison("    Range", b.range_semitones, c.range_semitones, "st", true);
        println!();
    } else {
        print_missing("Scale", &baseline, &current);
    }

    // Reading comparison
    if let (Some(ref b), Some(ref c)) = (&baseline.analysis.reading, &current.analysis.reading) {
        println!("{}", style("  Reading Passage").bold());
        print_comparison("    Mean F0", b.mean_f0_hz, c.mean_f0_hz, "Hz", true);
        print_comparison("    F0 std", b.f0_std_hz, c.f0_std_hz, "Hz", true);
        print_comparison_int("    Breaks", b.voice_breaks, c.voice_breaks, false);
        print_comparison(
            "    Voiced",
            b.voiced_fraction * 100.0,
            c.voiced_fraction * 100.0,
            "%",
            true,
        );
        println!();
    } else {
        print_missing("Reading", &baseline, &current);
    }

    Ok(())
}

/// Print a comparison line for f32 values.
/// `higher_is_better` controls the arrow color: green for improvement, red for regression.
fn print_comparison(label: &str, baseline: f32, current: f32, unit: &str, higher_is_better: bool) {
    let delta = current - baseline;
    let arrow = if delta.abs() < 0.01 {
        style("=").dim().to_string()
    } else {
        let improving = if higher_is_better {
            delta > 0.0
        } else {
            delta < 0.0
        };
        if improving {
            style(format!("{:+.1}", delta)).green().to_string()
        } else {
            style(format!("{:+.1}", delta)).red().to_string()
        }
    };

    println!(
        "{:16} {:>8.1} {} → {:>8.1} {}  {}",
        label, baseline, unit, current, unit, arrow
    );
}

/// Print a comparison line for integer values.
fn print_comparison_int(label: &str, baseline: usize, current: usize, higher_is_better: bool) {
    let delta = current as i64 - baseline as i64;
    let arrow = if delta == 0 {
        style("=").dim().to_string()
    } else {
        let improving = if higher_is_better {
            delta > 0
        } else {
            delta < 0
        };
        if improving {
            style(format!("{:+}", delta)).green().to_string()
        } else {
            style(format!("{:+}", delta)).red().to_string()
        }
    };

    println!(
        "{:16} {:>8} → {:>8}    {}",
        label, baseline, current, arrow
    );
}

fn print_missing(exercise: &str, baseline: &SessionData, current: &SessionData) {
    let b_has = match exercise {
        "Sustained" => baseline.analysis.sustained.is_some(),
        "Scale" => baseline.analysis.scale.is_some(),
        "Reading" => baseline.analysis.reading.is_some(),
        _ => false,
    };
    let c_has = match exercise {
        "Sustained" => current.analysis.sustained.is_some(),
        "Scale" => current.analysis.scale.is_some(),
        "Reading" => current.analysis.reading.is_some(),
        _ => false,
    };

    if !b_has && !c_has {
        println!(
            "  {} — not recorded in either session",
            style(exercise).dim()
        );
    } else if !b_has {
        println!(
            "  {} — not recorded in baseline",
            style(exercise).dim()
        );
    } else {
        println!(
            "  {} — not recorded in current session",
            style(exercise).dim()
        );
    }
}
