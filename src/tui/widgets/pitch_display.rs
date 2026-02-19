use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::dsp::pitch;

/// Live pitch display showing detected note, deviation bar, and cents offset.
pub struct PitchDisplayWidget {
    pitch_hz: Option<f32>,
}

impl PitchDisplayWidget {
    pub fn new(pitch_hz: Option<f32>) -> Self {
        Self { pitch_hz }
    }
}

impl Widget for PitchDisplayWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .title(" Pitch ")
            .borders(Borders::ALL);
        let inner = block.inner(area);
        block.render(area, buf);

        let Some(hz) = self.pitch_hz else {
            let line = Line::from(Span::styled(
                "---",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ));
            Paragraph::new(line).render(inner, buf);
            return;
        };

        let (note, octave, cents) = pitch::freq_to_note(hz);

        // Color based on tuning accuracy
        let color = if cents.abs() <= 20.0 {
            Color::Green
        } else if cents.abs() <= 50.0 {
            Color::Yellow
        } else {
            Color::Red
        };

        let rows = Layout::vertical([
            Constraint::Length(1), // note name
            Constraint::Length(1), // deviation bar
            Constraint::Length(1), // cents + Hz
        ])
        .split(inner);

        // Note name (e.g., "G#4")
        let note_text = format!("{}{}", note, octave);
        let note_line = Line::from(Span::styled(
            format!("{:^width$}", note_text, width = inner.width as usize),
            Style::default()
                .fg(color)
                .add_modifier(Modifier::BOLD),
        ));
        Paragraph::new(note_line).render(rows[0], buf);

        // Deviation bar: center = in tune, left = flat, right = sharp
        if rows.len() > 1 {
            let bar_width = (inner.width as usize).saturating_sub(2);
            let center = bar_width / 2;
            // Map cents (-50..+50) to position
            let offset = ((cents / 50.0).clamp(-1.0, 1.0) * center as f32) as i32;
            let pos = (center as i32 + offset).clamp(0, bar_width as i32 - 1) as usize;

            let mut bar = vec!['━'; bar_width];
            if center < bar_width {
                bar[center] = '┃';
            }
            if pos < bar_width {
                bar[pos] = '●';
            }

            let bar_str: String = bar.into_iter().collect();
            let bar_line = Line::from(Span::styled(
                format!("◄{}►", bar_str),
                Style::default().fg(color),
            ));
            Paragraph::new(bar_line).render(rows[1], buf);
        }

        // Cents offset + Hz
        if rows.len() > 2 {
            let sign = if cents >= 0.0 { "+" } else { "" };
            let info = format!("{}{:.0} cents  ({:.1} Hz)", sign, cents, hz);
            let info_line = Line::from(Span::styled(
                format!("{:^width$}", info, width = inner.width as usize),
                Style::default().fg(Color::DarkGray),
            ));
            Paragraph::new(info_line).render(rows[2], buf);
        }
    }
}
