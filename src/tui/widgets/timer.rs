use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

/// Timer display showing elapsed time and optional target.
pub struct TimerWidget {
    elapsed_secs: f32,
    target_secs: Option<f32>,
    /// Optional label override (e.g., "Trial 3/5" or "Rest: 30s").
    label: Option<String>,
}

impl TimerWidget {
    pub fn new(elapsed_secs: f32) -> Self {
        Self {
            elapsed_secs,
            target_secs: None,
            label: None,
        }
    }

    pub fn with_target(mut self, target: f32) -> Self {
        self.target_secs = Some(target);
        self
    }

    pub fn with_label(mut self, label: String) -> Self {
        self.label = Some(label);
        self
    }
}

impl Widget for TimerWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = if let Some(label) = &self.label {
            label.clone()
        } else if let Some(target) = self.target_secs {
            format!("{:.1}s / {:.1}s", self.elapsed_secs, target)
        } else {
            format!("{:.1}s", self.elapsed_secs)
        };

        let color = if let Some(target) = self.target_secs {
            if self.elapsed_secs >= target {
                Color::Green
            } else {
                Color::White
            }
        } else {
            Color::White
        };

        let line = Line::from(vec![Span::styled(
            text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]);

        Paragraph::new(line)
            .block(
                Block::default()
                    .title(" Timer ")
                    .borders(Borders::ALL),
            )
            .render(area, buf);
    }
}
