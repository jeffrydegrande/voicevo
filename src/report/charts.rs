use std::path::Path;

use anyhow::{Context, Result};
use plotters::prelude::*;

use crate::storage::session_data::SessionData;

/// Chart dimensions
const WIDTH: u32 = 1200;
const PANEL_HEIGHT: u32 = 250;
const PANELS: u32 = 7;
const TOTAL_HEIGHT: u32 = PANEL_HEIGHT * PANELS + 80; // extra for title

/// Colors for chart lines/points
const COLOR_PRIMARY: RGBColor = RGBColor(41, 128, 185); // blue
const COLOR_SECONDARY: RGBColor = RGBColor(231, 76, 60); // red
const COLOR_TERTIARY: RGBColor = RGBColor(46, 204, 113); // green
const COLOR_THRESHOLD: RGBColor = RGBColor(200, 200, 200); // light gray

/// Generate a multi-panel trend report PNG from a list of sessions.
///
/// Each panel shows one metric over time, with dates on the x-axis.
/// Threshold lines are drawn where clinically relevant.
pub fn generate_trend_chart(sessions: &[SessionData], output_path: &Path) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let root = BitMapBackend::new(output_path, (WIDTH, TOTAL_HEIGHT)).into_drawing_area();
    root.fill(&WHITE).context("Failed to fill background")?;

    // Title
    root.draw(&Text::new(
        "Voice Recovery — Trend Report",
        (WIDTH as i32 / 2 - 180, 15),
        ("sans-serif", 28).into_font().color(&BLACK),
    ))
    .context("Failed to draw title")?;

    // Split into panels
    let panels_area = root.margin(60, 10, 10, 10);
    let panels = panels_area.split_evenly((PANELS as usize, 1));

    // Extract date labels for x-axis
    let dates: Vec<&str> = sessions.iter().map(|s| s.date.as_str()).collect();
    let x_range = 0..dates.len();

    // Panel 1: Voice Quality Index (composite)
    draw_voice_quality(&panels[0], sessions, &dates, x_range.clone())?;

    // Panel 2: Pitch Range (floor + ceiling)
    draw_pitch_range(&panels[1], sessions, &dates, x_range.clone())?;

    // Panel 3: HNR
    draw_hnr(&panels[2], sessions, &dates, x_range.clone())?;

    // Panel 4: Jitter + Shimmer
    draw_jitter_shimmer(&panels[3], sessions, &dates, x_range.clone())?;

    // Panel 5: MPT
    draw_mpt(&panels[4], sessions, &dates, x_range.clone())?;

    // Panel 6: Voice Breaks
    draw_voice_breaks(&panels[5], sessions, &dates, x_range.clone())?;

    // Panel 7: Mean Speaking F0
    draw_mean_f0(&panels[6], sessions, &dates, x_range)?;

    root.present().context("Failed to write chart PNG")?;

    Ok(())
}

/// Helper: get x-axis labels, showing every Nth date to avoid crowding.
fn date_labels(dates: &[&str]) -> Vec<(usize, String)> {
    let step = (dates.len() / 8).max(1);
    dates
        .iter()
        .enumerate()
        .filter(|(i, _)| i % step == 0 || *i == dates.len() - 1)
        .map(|(i, d)| {
            // Shorten date: "2026-02-08" → "02-08"
            let short = if d.len() >= 10 { &d[5..] } else { d };
            (i, short.to_string())
        })
        .collect()
}

fn draw_pitch_range(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    sessions: &[SessionData],
    dates: &[&str],
    x_range: std::ops::Range<usize>,
) -> Result<()> {
    let floors: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.scale.as_ref().map(|a| a.pitch_floor_hz))
        .collect();
    let ceilings: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.scale.as_ref().map(|a| a.pitch_ceiling_hz))
        .collect();

    let all_vals: Vec<f32> = floors
        .iter()
        .chain(ceilings.iter())
        .filter_map(|v| *v)
        .collect();
    let (y_min, y_max) = min_max_with_margin(&all_vals, 20.0, 500.0);

    let mut chart = ChartBuilder::on(area)
        .caption("Pitch Range (Hz)", ("sans-serif", 18))
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(50)
        .build_cartesian_2d(x_range, y_min..y_max)?;

    chart
        .configure_mesh()
        .x_labels(8)
        .x_label_formatter(&|x| {
            date_labels(dates)
                .iter()
                .find(|(i, _)| i == x)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        })
        .draw()?;

    // Floor line
    let floor_points: Vec<(usize, f32)> = floors
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart.draw_series(LineSeries::new(floor_points.iter().copied(), &COLOR_PRIMARY))?
        .label("Floor")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &COLOR_PRIMARY));

    // Ceiling line
    let ceiling_points: Vec<(usize, f32)> = ceilings
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart.draw_series(LineSeries::new(ceiling_points.iter().copied(), &COLOR_SECONDARY))?
        .label("Ceiling")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &COLOR_SECONDARY));

    chart.configure_series_labels().draw()?;

    Ok(())
}

fn draw_hnr(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    sessions: &[SessionData],
    dates: &[&str],
    x_range: std::ops::Range<usize>,
) -> Result<()> {
    let values: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.sustained.as_ref().map(|a| a.hnr_db))
        .collect();
    let all_vals: Vec<f32> = values.iter().filter_map(|v| *v).collect();
    let (y_min, y_max) = min_max_with_margin(&all_vals, 0.0, 30.0);

    let mut chart = ChartBuilder::on(area)
        .caption("Harmonic-to-Noise Ratio (dB)", ("sans-serif", 18))
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(50)
        .build_cartesian_2d(x_range, y_min..y_max)?;

    chart
        .configure_mesh()
        .x_labels(8)
        .x_label_formatter(&|x| {
            date_labels(dates)
                .iter()
                .find(|(i, _)| i == x)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        })
        .draw()?;

    // Threshold lines
    draw_horizontal_line(&mut chart, 7.0, y_min, y_max, "<7 dB = severely breathy")?;
    draw_horizontal_line(&mut chart, 20.0, y_min, y_max, ">20 dB = healthy voice")?;

    let points: Vec<(usize, f32)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart.draw_series(LineSeries::new(points.iter().copied(), &COLOR_PRIMARY))?;
    chart.draw_series(points.iter().map(|&(x, y)| Circle::new((x, y), 4, COLOR_PRIMARY.filled())))?;

    Ok(())
}

fn draw_jitter_shimmer(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    sessions: &[SessionData],
    dates: &[&str],
    x_range: std::ops::Range<usize>,
) -> Result<()> {
    let jitter: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.sustained.as_ref().map(|a| a.jitter_local_percent))
        .collect();
    let shimmer: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.sustained.as_ref().map(|a| a.shimmer_local_percent))
        .collect();

    let all_vals: Vec<f32> = jitter
        .iter()
        .chain(shimmer.iter())
        .filter_map(|v| *v)
        .collect();
    let (y_min, y_max) = min_max_with_margin(&all_vals, 0.0, 10.0);

    let mut chart = ChartBuilder::on(area)
        .caption("Jitter & Shimmer (%)", ("sans-serif", 18))
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(50)
        .build_cartesian_2d(x_range, y_min..y_max)?;

    chart
        .configure_mesh()
        .x_labels(8)
        .x_label_formatter(&|x| {
            date_labels(dates)
                .iter()
                .find(|(i, _)| i == x)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        })
        .draw()?;

    // Threshold lines: values below these are normal (Praat norms)
    draw_horizontal_line(&mut chart, 1.04, y_min, y_max, "Jitter normal limit (1.04%)")?;
    draw_horizontal_line(&mut chart, 3.81, y_min, y_max, "Shimmer normal limit (3.81%)")?;

    let j_points: Vec<(usize, f32)> = jitter
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart
        .draw_series(LineSeries::new(j_points.iter().copied(), &COLOR_PRIMARY))?
        .label("Jitter")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &COLOR_PRIMARY));

    let s_points: Vec<(usize, f32)> = shimmer
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart
        .draw_series(LineSeries::new(s_points.iter().copied(), &COLOR_SECONDARY))?
        .label("Shimmer")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &COLOR_SECONDARY));

    chart.configure_series_labels().draw()?;

    Ok(())
}

fn draw_mpt(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    sessions: &[SessionData],
    dates: &[&str],
    x_range: std::ops::Range<usize>,
) -> Result<()> {
    let values: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.sustained.as_ref().map(|a| a.mpt_seconds))
        .collect();
    let all_vals: Vec<f32> = values.iter().filter_map(|v| *v).collect();
    let (y_min, y_max) = min_max_with_margin(&all_vals, 0.0, 25.0);

    let mut chart = ChartBuilder::on(area)
        .caption("Max Phonation Time (s)", ("sans-serif", 18))
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(50)
        .build_cartesian_2d(x_range, y_min..y_max)?;

    chart
        .configure_mesh()
        .x_labels(8)
        .x_label_formatter(&|x| {
            date_labels(dates)
                .iter()
                .find(|(i, _)| i == x)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        })
        .draw()?;

    // Threshold lines
    draw_horizontal_line(&mut chart, 10.0, y_min, y_max, "<10s = significant dysfunction")?;
    draw_horizontal_line(&mut chart, 15.0, y_min, y_max, ">15s = healthy range")?;

    let points: Vec<(usize, f32)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart.draw_series(LineSeries::new(points.iter().copied(), &COLOR_TERTIARY))?;
    chart.draw_series(points.iter().map(|&(x, y)| Circle::new((x, y), 4, COLOR_TERTIARY.filled())))?;

    Ok(())
}

fn draw_voice_breaks(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    sessions: &[SessionData],
    dates: &[&str],
    x_range: std::ops::Range<usize>,
) -> Result<()> {
    let values: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.reading.as_ref().map(|a| a.voice_breaks as f32))
        .collect();
    let all_vals: Vec<f32> = values.iter().filter_map(|v| *v).collect();
    let (y_min, y_max) = min_max_with_margin(&all_vals, 0.0, 10.0);

    let mut chart = ChartBuilder::on(area)
        .caption("Voice Breaks (Reading)", ("sans-serif", 18))
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(50)
        .build_cartesian_2d(x_range, y_min..y_max)?;

    chart
        .configure_mesh()
        .x_labels(8)
        .x_label_formatter(&|x| {
            date_labels(dates)
                .iter()
                .find(|(i, _)| i == x)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        })
        .draw()?;

    // Threshold lines: fewer is better, 0 is ideal
    draw_horizontal_line(&mut chart, 0.0, y_min, y_max, "0 = ideal (no breaks)")?;
    draw_horizontal_line(&mut chart, 5.0, y_min, y_max, ">5 = concerning")?;

    let points: Vec<(usize, f32)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart.draw_series(LineSeries::new(points.iter().copied(), &COLOR_SECONDARY))?;
    chart.draw_series(points.iter().map(|&(x, y)| Circle::new((x, y), 4, COLOR_SECONDARY.filled())))?;

    Ok(())
}

fn draw_mean_f0(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    sessions: &[SessionData],
    dates: &[&str],
    x_range: std::ops::Range<usize>,
) -> Result<()> {
    let values: Vec<Option<f32>> = sessions
        .iter()
        .map(|s| s.analysis.reading.as_ref().map(|a| a.mean_f0_hz))
        .collect();
    let all_vals: Vec<f32> = values.iter().filter_map(|v| *v).collect();
    let (y_min, y_max) = min_max_with_margin(&all_vals, 50.0, 200.0);

    let mut chart = ChartBuilder::on(area)
        .caption("Mean Speaking F0 (Hz)", ("sans-serif", 18))
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(50)
        .build_cartesian_2d(x_range, y_min..y_max)?;

    chart
        .configure_mesh()
        .x_labels(8)
        .x_label_formatter(&|x| {
            date_labels(dates)
                .iter()
                .find(|(i, _)| i == x)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        })
        .draw()?;

    // Male normal range: 85–180 Hz
    draw_horizontal_line(&mut chart, 85.0, y_min, y_max, "Male low (85 Hz)")?;
    draw_horizontal_line(&mut chart, 180.0, y_min, y_max, "Male high (180 Hz)")?;

    let points: Vec<(usize, f32)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();
    chart.draw_series(LineSeries::new(points.iter().copied(), &COLOR_PRIMARY))?;
    chart.draw_series(points.iter().map(|&(x, y)| Circle::new((x, y), 4, COLOR_PRIMARY.filled())))?;

    Ok(())
}

/// Draw a dashed horizontal threshold line with a right-aligned label.
fn draw_horizontal_line(
    chart: &mut ChartContext<BitMapBackend, Cartesian2d<plotters::coord::types::RangedCoordusize, plotters::coord::types::RangedCoordf32>>,
    y_val: f32,
    y_min: f32,
    y_max: f32,
    label: &str,
) -> Result<()> {
    if y_val >= y_min && y_val <= y_max {
        chart.draw_series(DashedLineSeries::new(
            vec![(0usize, y_val), (1000usize, y_val)],
            5,
            3,
            COLOR_THRESHOLD.into(),
        ))?;
        // Label at the left edge, slightly above the line
        let nudge = (y_max - y_min) * 0.03;
        chart.draw_series(std::iter::once(Text::new(
            label.to_string(),
            (1usize, y_val + nudge),
            ("sans-serif", 13).into_font().color(&RGBColor(140, 140, 140)),
        )))?;
    }
    Ok(())
}

/// Compute y-axis range with margin, falling back to defaults if no data.
fn min_max_with_margin(values: &[f32], default_min: f32, default_max: f32) -> (f32, f32) {
    if values.is_empty() {
        return (default_min, default_max);
    }
    let min = values.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let margin = (max - min).max(1.0) * 0.1;
    (min - margin, max + margin)
}

/// Compute a composite Voice Quality Index (0–100, higher = healthier) from
/// whichever metrics are available in a session.
///
/// Each metric is linearly mapped to 0–100:
///   HNR:            0 dB → 0,   20 dB → 100
///   Jitter:         5%  → 0,    0%    → 100
///   Shimmer:       50%  → 0,    0%    → 100
///   MPT:            0 s → 0,   20 s   → 100
///   Voice breaks:  40   → 0,    0     → 100
///   Voiced frac:   0.2  → 0,    0.8   → 100
///
/// All sub-scores are clamped to [0, 100] then averaged.
fn compute_voice_quality_index(session: &SessionData) -> Option<f32> {
    let mut scores = Vec::new();

    if let Some(ref s) = session.analysis.sustained {
        // HNR: 0 dB = 0, 20 dB = 100
        scores.push(((s.hnr_db / 20.0) * 100.0).clamp(0.0, 100.0));
        // Jitter: 5% = 0, 0% = 100
        scores.push(((1.0 - s.jitter_local_percent / 5.0) * 100.0).clamp(0.0, 100.0));
        // Shimmer: 50% = 0, 0% = 100
        scores.push(((1.0 - s.shimmer_local_percent / 50.0) * 100.0).clamp(0.0, 100.0));
        // MPT: 0s = 0, 20s = 100
        scores.push(((s.mpt_seconds / 20.0) * 100.0).clamp(0.0, 100.0));
    }

    if let Some(ref r) = session.analysis.reading {
        // Voice breaks: 40 = 0, 0 = 100
        scores.push(((1.0 - r.voice_breaks as f32 / 40.0) * 100.0).clamp(0.0, 100.0));
        // Voiced fraction: 0.2 = 0, 0.8 = 100
        scores.push((((r.voiced_fraction - 0.2) / 0.6) * 100.0).clamp(0.0, 100.0));
    }

    if scores.is_empty() {
        None
    } else {
        Some(scores.iter().sum::<f32>() / scores.len() as f32)
    }
}

fn draw_voice_quality(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    sessions: &[SessionData],
    dates: &[&str],
    x_range: std::ops::Range<usize>,
) -> Result<()> {
    let values: Vec<Option<f32>> = sessions
        .iter()
        .map(compute_voice_quality_index)
        .collect();

    let mut chart = ChartBuilder::on(area)
        .caption("Voice Quality Index (0–100)", ("sans-serif", 18))
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(50)
        .build_cartesian_2d(x_range, 0.0_f32..105.0_f32)?;

    chart
        .configure_mesh()
        .x_labels(8)
        .x_label_formatter(&|x| {
            date_labels(dates)
                .iter()
                .find(|(i, _)| i == x)
                .map(|(_, l)| l.clone())
                .unwrap_or_default()
        })
        .draw()?;

    // Shade zones: red (0-33), yellow (33-66), green (66-100)
    let n = dates.len().max(1);
    chart.draw_series(std::iter::once(Rectangle::new(
        [(0, 0.0_f32), (n, 33.0_f32)],
        RGBColor(255, 200, 200).filled(),
    )))?;
    chart.draw_series(std::iter::once(Rectangle::new(
        [(0, 33.0_f32), (n, 66.0_f32)],
        RGBColor(255, 240, 200).filled(),
    )))?;
    chart.draw_series(std::iter::once(Rectangle::new(
        [(0, 66.0_f32), (n, 100.0_f32)],
        RGBColor(200, 255, 200).filled(),
    )))?;

    let points: Vec<(usize, f32)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|f| (i, f)))
        .collect();

    chart.draw_series(LineSeries::new(
        points.iter().copied(),
        COLOR_PRIMARY.stroke_width(2),
    ))?;
    chart.draw_series(
        points
            .iter()
            .map(|&(x, y)| Circle::new((x, y), 5, COLOR_PRIMARY.filled())),
    )?;

    Ok(())
}
