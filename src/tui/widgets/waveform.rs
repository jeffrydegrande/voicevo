use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Sparkline, Widget};

/// Rolling waveform display using a sparkline of recent RMS values.
pub struct WaveformWidget<'a> {
    /// Recent RMS values (linear) from the audio ring buffer.
    rms_values: &'a [f32],
}

impl<'a> WaveformWidget<'a> {
    pub fn new(rms_values: &'a [f32]) -> Self {
        Self { rms_values }
    }
}

impl Widget for WaveformWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Map linear RMS to 0-100 range via dB scaling.
        // Range: -60 dB (0) to 0 dB (100).
        let data: Vec<u64> = self
            .rms_values
            .iter()
            .map(|&rms| {
                let db = if rms > 0.0 {
                    20.0 * rms.log10()
                } else {
                    -60.0
                };
                let normalized = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                (normalized * 100.0) as u64
            })
            .collect();

        Sparkline::default()
            .block(
                Block::default()
                    .title(" Waveform ")
                    .borders(Borders::ALL),
            )
            .data(&data)
            .max(100)
            .style(Style::default().fg(Color::Cyan))
            .render(area, buf);
    }
}
