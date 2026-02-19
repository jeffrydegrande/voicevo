use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, LineGauge, Widget};

/// Volume meter with dynamic color based on level.
pub struct VolumeMeterWidget {
    /// Current RMS in dB.
    rms_db: f32,
}

impl VolumeMeterWidget {
    pub fn new(rms_db: f32) -> Self {
        Self { rms_db }
    }
}

impl Widget for VolumeMeterWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Map -60..0 dB to 0.0..1.0
        let ratio = ((self.rms_db + 60.0) / 60.0).clamp(0.0, 1.0) as f64;

        let color = if ratio < 0.6 {
            Color::Green
        } else if ratio < 0.85 {
            Color::Yellow
        } else {
            Color::Red
        };

        let label = if self.rms_db.is_finite() {
            format!("{:.1} dB", self.rms_db)
        } else {
            "-- dB".to_string()
        };

        LineGauge::default()
            .block(
                Block::default()
                    .title(" Volume ")
                    .borders(Borders::ALL),
            )
            .filled_style(Style::default().fg(color))
            .ratio(ratio)
            .label(label)
            .render(area, buf);
    }
}
