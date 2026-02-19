use std::time::Instant;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::audio::capture::{AudioState, SILENCE_POLL_COUNT, MIN_DURATION_SECS};
use crate::tui::event::{AppEvent, EventHandler};
use crate::tui::widgets::pitch_display::PitchDisplayWidget;
use crate::tui::widgets::timer::TimerWidget;
use crate::tui::widgets::volume_meter::VolumeMeterWidget;
use crate::tui::widgets::waveform::WaveformWidget;
use crate::tui::Tui;

use ratatui::crossterm::event::{KeyCode, KeyEventKind};

/// Outcome of the scale recording screen.
pub struct ScaleOutcome {
    pub phonation_secs: f32,
    pub silent_polls: usize,
}

/// Run the chromatic scale recording screen with live pitch feedback.
pub fn run(
    terminal: &mut Tui,
    audio: &AudioState,
) -> anyhow::Result<ScaleOutcome> {
    let events = EventHandler::new(std::time::Duration::from_millis(33));
    let start = Instant::now();
    let mut silent_polls: usize = 0;

    loop {
        let elapsed = start.elapsed().as_secs_f32();
        let rms_db = audio.rms_db();
        let waveform = audio.waveform_snapshot();
        let pitch_hz = audio.pitch_hz();

        terminal.draw(|frame| {
            let area = frame.area();
            render_scale(frame, area, elapsed, rms_db, &waveform, pitch_hz);
        })?;

        match events.next()? {
            AppEvent::Key(key) => {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Enter | KeyCode::Char('q') | KeyCode::Esc => break,
                        _ => {}
                    }
                }
            }
            AppEvent::Tick | AppEvent::Resize(_, _) => {}
        }

        if elapsed > MIN_DURATION_SECS && audio.is_silent() {
            silent_polls += 1;
            if silent_polls >= SILENCE_POLL_COUNT {
                break;
            }
        } else {
            silent_polls = 0;
        }
    }

    let total_elapsed = start.elapsed().as_secs_f32();
    let trailing_silence = silent_polls as f32 * 0.033;
    let phonation_secs = (total_elapsed - trailing_silence).max(0.0);

    Ok(ScaleOutcome {
        phonation_secs,
        silent_polls,
    })
}

fn render_scale(
    frame: &mut ratatui::Frame,
    area: Rect,
    elapsed: f32,
    rms_db: f32,
    waveform: &[f32],
    pitch_hz: Option<f32>,
) {
    let outer = Block::default()
        .title(" Chromatic Scale ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let rows = Layout::vertical([
        Constraint::Length(2), // instructions
        Constraint::Length(5), // pitch display + timer
        Constraint::Length(3), // volume
        Constraint::Min(4),   // waveform
        Constraint::Length(1), // key hint
    ])
    .split(inner);

    // Instructions
    let inst = Paragraph::new(Line::from(Span::styled(
        "  Sing lowest to highest, then back down.",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(inst, rows[0]);

    // Pitch + Timer side by side
    let cols = Layout::horizontal([
        Constraint::Percentage(60),
        Constraint::Percentage(40),
    ])
    .split(rows[1]);

    frame.render_widget(PitchDisplayWidget::new(pitch_hz), cols[0]);
    frame.render_widget(TimerWidget::new(elapsed), cols[1]);

    // Volume
    frame.render_widget(VolumeMeterWidget::new(rms_db), rows[2]);

    // Waveform
    frame.render_widget(WaveformWidget::new(waveform), rows[3]);

    // Key hint
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("  [Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" stop"),
    ]));
    frame.render_widget(hint, rows[4]);
}
