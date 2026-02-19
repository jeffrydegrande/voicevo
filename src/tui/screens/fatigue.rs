use std::time::Instant;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Row, Table};

use crate::audio::capture::{AudioState, SILENCE_POLL_COUNT, MIN_DURATION_SECS};
use crate::tui::event::{AppEvent, EventHandler};
use crate::tui::widgets::timer::TimerWidget;
use crate::tui::widgets::volume_meter::VolumeMeterWidget;
use crate::tui::widgets::waveform::WaveformWidget;
use crate::tui::Tui;

use ratatui::crossterm::event::{KeyCode, KeyEventKind};

/// Number of sustained vowel trials.
const NUM_TRIALS: usize = 5;

/// Rest period between trials in seconds.
const REST_SECS: u64 = 45;

/// State machine for the fatigue exercise.
enum FatigueState {
    WaitingForStart { trial: usize },
    Recording { trial: usize, start: Instant, silent_polls: usize },
    EffortRating { trial: usize, duration: f32 },
    Resting { trial: usize, rest_start: Instant },
}

/// Outcome of a single fatigue trial.
struct TrialResult {
    duration: f32,
    effort: u8,
}

/// Outcome of the full fatigue exercise.
pub struct FatigueOutcome {
    pub mpt_per_trial: Vec<f32>,
    pub effort_per_trial: Vec<u8>,
}

/// Run the fatigue exercise in the TUI.
///
/// The caller is responsible for computing CPPS from the collected samples
/// after the TUI session ends.
pub fn run(
    terminal: &mut Tui,
    audio: &AudioState,
) -> anyhow::Result<FatigueOutcome> {
    let events = EventHandler::new(std::time::Duration::from_millis(33));

    let mut results: Vec<TrialResult> = Vec::new();
    let mut state = FatigueState::WaitingForStart { trial: 1 };

    loop {
        let rms_db = audio.rms_db();
        let waveform = audio.waveform_snapshot();

        terminal.draw(|frame| {
            let area = frame.area();
            render_fatigue(frame, area, &state, rms_db, &waveform, &results);
        })?;

        match events.next()? {
            AppEvent::Key(key) if key.kind == KeyEventKind::Press => {
                match &mut state {
                    FatigueState::WaitingForStart { trial } => {
                        if key.code == KeyCode::Enter {
                            state = FatigueState::Recording {
                                trial: *trial,
                                start: Instant::now(),
                                silent_polls: 0,
                            };
                        } else if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                            break;
                        }
                    }
                    FatigueState::Recording { trial, start, .. } => {
                        if key.code == KeyCode::Enter {
                            let duration = start.elapsed().as_secs_f32();
                            state = FatigueState::EffortRating { trial: *trial, duration };
                        }
                    }
                    FatigueState::EffortRating { trial, duration } => {
                        // Accept digit keys 1-9 and 0 (=10) for effort rating
                        let effort = match key.code {
                            KeyCode::Char('1') => Some(1),
                            KeyCode::Char('2') => Some(2),
                            KeyCode::Char('3') => Some(3),
                            KeyCode::Char('4') => Some(4),
                            KeyCode::Char('5') => Some(5),
                            KeyCode::Char('6') => Some(6),
                            KeyCode::Char('7') => Some(7),
                            KeyCode::Char('8') => Some(8),
                            KeyCode::Char('9') => Some(9),
                            KeyCode::Char('0') => Some(10),
                            _ => None,
                        };

                        if let Some(e) = effort {
                            results.push(TrialResult { duration: *duration, effort: e });

                            if *trial < NUM_TRIALS {
                                state = FatigueState::Resting {
                                    trial: *trial,
                                    rest_start: Instant::now(),
                                };
                            } else {
                                break; // All trials complete
                            }
                        }
                    }
                    FatigueState::Resting { trial, .. } => {
                        // Allow skipping rest with Enter
                        if key.code == KeyCode::Enter {
                            state = FatigueState::WaitingForStart { trial: *trial + 1 };
                        }
                    }
                }
            }
            AppEvent::Tick | AppEvent::Resize(_, _) => {
                match &mut state {
                    FatigueState::Recording { trial, start, silent_polls } => {
                        let elapsed = start.elapsed().as_secs_f32();
                        if elapsed > MIN_DURATION_SECS && audio.is_silent() {
                            *silent_polls += 1;
                            if *silent_polls >= SILENCE_POLL_COUNT {
                                let trailing = *silent_polls as f32 * 0.033;
                                let duration = (elapsed - trailing).max(0.0);
                                state = FatigueState::EffortRating { trial: *trial, duration };
                            }
                        } else {
                            *silent_polls = 0;
                        }
                    }
                    FatigueState::Resting { trial, rest_start } => {
                        let elapsed = rest_start.elapsed().as_secs();
                        if elapsed >= REST_SECS {
                            state = FatigueState::WaitingForStart { trial: *trial + 1 };
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    Ok(FatigueOutcome {
        mpt_per_trial: results.iter().map(|r| r.duration).collect(),
        effort_per_trial: results.iter().map(|r| r.effort).collect(),
    })
}

fn render_fatigue(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &FatigueState,
    rms_db: f32,
    waveform: &[f32],
    results: &[TrialResult],
) {
    let outer = Block::default()
        .title(" Vocal Fatigue Test ")
        .borders(Borders::ALL);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let results_height = if results.is_empty() { 0 } else { results.len() as u16 + 3 };

    let rows = Layout::vertical([
        Constraint::Length(3), // status / instructions
        Constraint::Length(3), // timer + volume (or rest gauge)
        Constraint::Min(4),   // waveform
        Constraint::Length(results_height), // results table
        Constraint::Length(1), // key hint
    ])
    .split(inner);

    // Status / instructions
    let (status_text, status_color) = match state {
        FatigueState::WaitingForStart { trial } => (
            format!("Trial {}/{}: Take a deep breath, then hold \"AAAH\".\n  Press [Enter] when ready.",
                trial, NUM_TRIALS),
            Color::White,
        ),
        FatigueState::Recording { trial, .. } => (
            format!("Trial {}/{}: Recording... hold your note!", trial, NUM_TRIALS),
            Color::Green,
        ),
        FatigueState::EffortRating { trial, duration } => (
            format!("Trial {}/{}: {:.1}s\n  Rate effort (1=easy, 9=hard, 0=max strain):",
                trial, NUM_TRIALS, duration),
            Color::Cyan,
        ),
        FatigueState::Resting { trial, rest_start } => {
            let remaining = REST_SECS.saturating_sub(rest_start.elapsed().as_secs());
            (
                format!("Rest before trial {}/{}... {}s remaining\n  Press [Enter] to skip.",
                    trial + 1, NUM_TRIALS, remaining),
                Color::Yellow,
            )
        }
    };
    let status = Paragraph::new(format!("  {}", status_text))
        .style(Style::default().fg(status_color));
    frame.render_widget(status, rows[0]);

    // Timer / Volume / Rest gauge
    let cols = Layout::horizontal([
        Constraint::Length(22),
        Constraint::Min(20),
    ])
    .split(rows[1]);

    match state {
        FatigueState::Recording { start, .. } => {
            let elapsed = start.elapsed().as_secs_f32();
            frame.render_widget(TimerWidget::new(elapsed), cols[0]);
            frame.render_widget(VolumeMeterWidget::new(rms_db), cols[1]);
        }
        FatigueState::Resting { rest_start, .. } => {
            let elapsed = rest_start.elapsed().as_secs();
            let remaining = REST_SECS.saturating_sub(elapsed);
            let ratio = elapsed as f64 / REST_SECS as f64;

            let gauge = Gauge::default()
                .block(Block::default().title(" Rest ").borders(Borders::ALL))
                .gauge_style(Style::default().fg(Color::Blue))
                .ratio(ratio.min(1.0))
                .label(format!("{}s remaining", remaining));
            frame.render_widget(gauge, rows[1]);
        }
        _ => {
            frame.render_widget(TimerWidget::new(0.0).with_label("--".into()), cols[0]);
            frame.render_widget(VolumeMeterWidget::new(rms_db), cols[1]);
        }
    }

    // Waveform
    frame.render_widget(WaveformWidget::new(waveform), rows[2]);

    // Results table
    if !results.is_empty() {
        let table_rows: Vec<Row> = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                Row::new(vec![
                    format!("  {}", i + 1),
                    format!("{:.1}s", r.duration),
                    format!("{}", r.effort),
                ])
            })
            .collect();

        let table = Table::new(
            table_rows,
            [Constraint::Length(6), Constraint::Length(10), Constraint::Length(10)],
        )
        .header(Row::new(vec!["  #", "MPT", "Effort"])
            .style(Style::default().add_modifier(Modifier::BOLD)))
        .block(Block::default().title(" Trials ").borders(Borders::ALL));

        frame.render_widget(table, rows[3]);
    }

    // Key hint
    let hint = match state {
        FatigueState::EffortRating { .. } => vec![
            Span::styled("  [1-9, 0=10]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" rate effort"),
        ],
        FatigueState::Recording { .. } => vec![
            Span::styled("  [Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" stop"),
        ],
        _ => vec![
            Span::styled("  [Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" continue  "),
            Span::styled("[Esc]", Style::default().fg(Color::Red)),
            Span::raw(" quit"),
        ],
    };
    frame.render_widget(Paragraph::new(Line::from(hint)), rows[4]);
}
