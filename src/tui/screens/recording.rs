use std::time::Instant;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::audio::capture::{AudioState, SILENCE_POLL_COUNT, MIN_DURATION_SECS};
use crate::tui::event::{AppEvent, EventHandler};
use crate::tui::widgets::timer::TimerWidget;
use crate::tui::widgets::volume_meter::VolumeMeterWidget;
use crate::tui::widgets::waveform::WaveformWidget;
use crate::tui::Tui;

use ratatui::crossterm::event::{KeyCode, KeyEventKind};

/// Outcome of the recording screen.
pub struct RecordingOutcome {
    /// How many seconds the patient phonated (excluding trailing silence).
    pub phonation_secs: f32,
    /// Number of trailing silent polls (for trimming samples).
    pub silent_polls: usize,
}

/// Run the recording screen for sustained phonation or reading exercises.
///
/// Shows a timer, volume meter, and waveform. Auto-stops on sustained silence.
/// Returns the phonation duration when the user presses Enter or silence is detected.
pub fn run(
    terminal: &mut Tui,
    audio: &AudioState,
    reference_target: Option<f32>,
    title: &str,
    instruction: &str,
) -> anyhow::Result<RecordingOutcome> {
    let events = EventHandler::new(std::time::Duration::from_millis(33));
    let start = Instant::now();
    let mut silent_polls: usize = 0;

    loop {
        let elapsed = start.elapsed().as_secs_f32();
        let rms_db = audio.rms_db();
        let waveform = audio.waveform_snapshot();

        terminal.draw(|frame| {
            let area = frame.area();
            render_recording_layout(
                frame,
                area,
                title,
                instruction,
                elapsed,
                reference_target,
                rms_db,
                &waveform,
            );
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

        // Auto-stop on sustained silence after minimum duration
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
    let trailing_silence = silent_polls as f32 * 0.033; // ~33ms per poll at 30fps
    let phonation_secs = (total_elapsed - trailing_silence).max(0.0);

    Ok(RecordingOutcome {
        phonation_secs,
        silent_polls,
    })
}

fn render_recording_layout(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    instruction: &str,
    elapsed: f32,
    target: Option<f32>,
    rms_db: f32,
    waveform: &[f32],
) {
    let outer = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::White));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let rows = Layout::vertical([
        Constraint::Length(2), // instructions
        Constraint::Length(3), // timer + volume
        Constraint::Min(4),   // waveform
        Constraint::Length(1), // key hint
    ])
    .split(inner);

    // Instructions
    let inst = Paragraph::new(Line::from(Span::styled(
        format!("  {}", instruction),
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(inst, rows[0]);

    // Timer + Volume side by side
    let cols = Layout::horizontal([
        Constraint::Length(22),
        Constraint::Min(20),
    ])
    .split(rows[1]);

    let timer = if let Some(t) = target {
        TimerWidget::new(elapsed).with_target(t)
    } else {
        TimerWidget::new(elapsed)
    };
    frame.render_widget(timer, cols[0]);
    frame.render_widget(VolumeMeterWidget::new(rms_db), cols[1]);

    // Waveform
    frame.render_widget(WaveformWidget::new(waveform), rows[2]);

    // Key hint
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("  [Enter]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" stop"),
    ]));
    frame.render_widget(hint, rows[3]);
}
