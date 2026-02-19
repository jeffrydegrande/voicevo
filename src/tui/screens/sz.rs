use std::time::Instant;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

use crate::audio::capture::{AudioState, SILENCE_POLL_COUNT};
use crate::tui::event::{AppEvent, EventHandler};
use crate::tui::widgets::timer::TimerWidget;
use crate::tui::widgets::volume_meter::VolumeMeterWidget;
use crate::tui::widgets::waveform::WaveformWidget;
use crate::tui::Tui;

use ratatui::crossterm::event::{KeyCode, KeyEventKind};

/// Minimum sound duration before auto-stop can trigger.
const MIN_DURATION_SECS: f32 = 1.0;

/// Number of trials per sound.
const TRIALS_PER_SOUND: usize = 2;

/// State machine for the S/Z exercise.
enum SzState {
    /// Waiting for user to press Enter to start recording.
    WaitingForStart { sound: Sound, trial: usize },
    /// Currently recording a sound.
    Recording { sound: Sound, trial: usize, start: Instant, silent_polls: usize },
    /// Showing result before moving to next trial.
    ShowResult { sound: Sound, trial: usize, duration: f32 },
}

#[derive(Clone, Copy)]
enum Sound {
    S,
    Z,
}

impl Sound {
    fn label(self) -> &'static str {
        match self {
            Sound::S => "/s/",
            Sound::Z => "/z/",
        }
    }

    fn instruction(self) -> &'static str {
        match self {
            Sound::S => "Hold a steady \"SSSSS\" sound as long as you can.",
            Sound::Z => "Hold a steady \"ZZZZZ\" sound as long as you can.",
        }
    }
}

/// Outcome of the S/Z exercise TUI.
pub struct SzOutcome {
    pub s_durations: Vec<f32>,
    pub z_durations: Vec<f32>,
}

/// Run the S/Z ratio exercise in the TUI.
pub fn run(
    terminal: &mut Tui,
    audio: &AudioState,
) -> anyhow::Result<SzOutcome> {
    let events = EventHandler::new(std::time::Duration::from_millis(33));

    let mut s_durations: Vec<f32> = Vec::new();
    let mut z_durations: Vec<f32> = Vec::new();
    let mut state = SzState::WaitingForStart { sound: Sound::S, trial: 1 };

    loop {
        let rms_db = audio.rms_db();
        let waveform = audio.waveform_snapshot();

        terminal.draw(|frame| {
            let area = frame.area();
            render_sz(frame, area, &state, rms_db, &waveform, &s_durations, &z_durations);
        })?;

        match events.next()? {
            AppEvent::Key(key) if key.kind == KeyEventKind::Press => {
                match &mut state {
                    SzState::WaitingForStart { sound, trial } => {
                        if key.code == KeyCode::Enter {
                            state = SzState::Recording {
                                sound: *sound,
                                trial: *trial,
                                start: Instant::now(),
                                silent_polls: 0,
                            };
                        } else if key.code == KeyCode::Esc || key.code == KeyCode::Char('q') {
                            return Ok(SzOutcome { s_durations, z_durations });
                        }
                    }
                    SzState::Recording { sound, trial, start, .. } => {
                        if key.code == KeyCode::Enter {
                            let duration = start.elapsed().as_secs_f32();
                            state = SzState::ShowResult { sound: *sound, trial: *trial, duration };
                        }
                    }
                    SzState::ShowResult { sound, trial, duration } => {
                        if key.code == KeyCode::Enter || key.code == KeyCode::Char(' ') {
                            // Save the duration
                            match sound {
                                Sound::S => s_durations.push(*duration),
                                Sound::Z => z_durations.push(*duration),
                            }

                            // Advance to next trial
                            let next = next_sz_state(*sound, *trial);
                            match next {
                                Some(s) => state = s,
                                None => return Ok(SzOutcome { s_durations, z_durations }),
                            }
                        }
                    }
                }
            }
            AppEvent::Tick | AppEvent::Resize(_, _) => {
                // Check auto-stop for recording state
                if let SzState::Recording { sound, trial, start, silent_polls } = &mut state {
                    let elapsed = start.elapsed().as_secs_f32();
                    if elapsed > MIN_DURATION_SECS && audio.is_silent() {
                        *silent_polls += 1;
                        if *silent_polls >= SILENCE_POLL_COUNT {
                            let trailing = *silent_polls as f32 * 0.033;
                            let duration = (elapsed - trailing).max(0.0);
                            state = SzState::ShowResult {
                                sound: *sound,
                                trial: *trial,
                                duration,
                            };
                        }
                    } else {
                        *silent_polls = 0;
                    }
                }
            }
            _ => {}
        }
    }
}

fn next_sz_state(sound: Sound, trial: usize) -> Option<SzState> {
    if trial < TRIALS_PER_SOUND {
        Some(SzState::WaitingForStart {
            sound,
            trial: trial + 1,
        })
    } else {
        match sound {
            Sound::S => Some(SzState::WaitingForStart {
                sound: Sound::Z,
                trial: 1,
            }),
            Sound::Z => None, // All done
        }
    }
}

fn render_sz(
    frame: &mut ratatui::Frame,
    area: Rect,
    state: &SzState,
    rms_db: f32,
    waveform: &[f32],
    s_durations: &[f32],
    z_durations: &[f32],
) {
    let outer = Block::default()
        .title(" S/Z Ratio Test ")
        .borders(Borders::ALL);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let rows = Layout::vertical([
        Constraint::Length(3), // instructions / status
        Constraint::Length(3), // timer + volume
        Constraint::Min(4),   // waveform
        Constraint::Length(2 + s_durations.len().max(z_durations.len()) as u16 + 1), // results table
        Constraint::Length(1), // key hint
    ])
    .split(inner);

    // Instructions / status
    let (status_text, status_color) = match state {
        SzState::WaitingForStart { sound, trial } => (
            format!("{} trial {}/{}: {}\n  Press [Enter] to start.",
                sound.label(), trial, TRIALS_PER_SOUND, sound.instruction()),
            Color::White,
        ),
        SzState::Recording { sound, trial, .. } => (
            format!("{} trial {}/{}: Recording...",
                sound.label(), trial, TRIALS_PER_SOUND),
            Color::Green,
        ),
        SzState::ShowResult { sound, trial, duration } => (
            format!("{} trial {}/{}: {:.1}s\n  Press [Enter] to continue.",
                sound.label(), trial, TRIALS_PER_SOUND, duration),
            Color::Cyan,
        ),
    };
    let status = Paragraph::new(format!("  {}", status_text))
        .style(Style::default().fg(status_color));
    frame.render_widget(status, rows[0]);

    // Timer + Volume
    let cols = Layout::horizontal([
        Constraint::Length(22),
        Constraint::Min(20),
    ])
    .split(rows[1]);

    if let SzState::Recording { start, .. } = state {
        let elapsed = start.elapsed().as_secs_f32();
        frame.render_widget(TimerWidget::new(elapsed), cols[0]);
    } else {
        frame.render_widget(TimerWidget::new(0.0).with_label("--".into()), cols[0]);
    }
    frame.render_widget(VolumeMeterWidget::new(rms_db), cols[1]);

    // Waveform
    frame.render_widget(WaveformWidget::new(waveform), rows[2]);

    // Results table
    if !s_durations.is_empty() || !z_durations.is_empty() {
        let max_rows = s_durations.len().max(z_durations.len());
        let table_rows: Vec<Row> = (0..max_rows)
            .map(|i| {
                let s = s_durations.get(i).map(|d| format!("{:.1}s", d)).unwrap_or_default();
                let z = z_durations.get(i).map(|d| format!("{:.1}s", d)).unwrap_or_default();
                Row::new(vec![format!("  {}", i + 1), s, z])
            })
            .collect();

        let table = Table::new(
            table_rows,
            [Constraint::Length(6), Constraint::Length(10), Constraint::Length(10)],
        )
        .header(Row::new(vec!["  #", "/s/", "/z/"]).style(Style::default().add_modifier(Modifier::BOLD)))
        .block(Block::default().title(" Results ").borders(Borders::ALL));

        frame.render_widget(table, rows[3]);
    }

    // Key hint
    let hint = match state {
        SzState::Recording { .. } => vec![
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
