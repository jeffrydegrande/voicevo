use chrono::{Local, Timelike};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Table};

use crate::storage::session_data::RecordingConditions;
use crate::tui::event::{AppEvent, EventHandler};
use crate::tui::Tui;

use ratatui::crossterm::event::{KeyCode, KeyEventKind};

/// Steps in the conditions questionnaire.
enum Step {
    TimeOfDay,
    Fatigue,
    ThroatCleared,
    MucusLevel,
    Hydration,
    Notes,
    Done,
}

/// Accumulated answers as we progress through the steps.
struct Answers {
    time_of_day: String,
    fatigue_level: u8,
    throat_cleared: bool,
    mucus_level: String,
    hydration: String,
    notes_buf: String,
}

impl Answers {
    fn new() -> Self {
        Self {
            time_of_day: detect_time_of_day(),
            fatigue_level: 0,
            throat_cleared: false,
            mucus_level: String::new(),
            hydration: String::new(),
            notes_buf: String::new(),
        }
    }
}

/// Auto-detect time of day from system clock.
fn detect_time_of_day() -> String {
    let hour = Local::now().hour();
    if hour < 12 {
        "morning".into()
    } else if hour < 17 {
        "afternoon".into()
    } else {
        "evening".into()
    }
}

/// Run the conditions questionnaire in the TUI.
pub fn run(terminal: &mut Tui) -> anyhow::Result<RecordingConditions> {
    let events = EventHandler::new(std::time::Duration::from_millis(33));
    let mut step = Step::TimeOfDay;
    let mut answers = Answers::new();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            render(frame, area, &step, &answers);
        })?;

        match events.next()? {
            AppEvent::Key(key) if key.kind == KeyEventKind::Press => {
                match &mut step {
                    Step::TimeOfDay => match key.code {
                        KeyCode::Enter => step = Step::Fatigue,
                        KeyCode::Char('1') => {
                            answers.time_of_day = "morning".into();
                            step = Step::Fatigue;
                        }
                        KeyCode::Char('2') => {
                            answers.time_of_day = "afternoon".into();
                            step = Step::Fatigue;
                        }
                        KeyCode::Char('3') => {
                            answers.time_of_day = "evening".into();
                            step = Step::Fatigue;
                        }
                        _ => {}
                    },
                    Step::Fatigue => {
                        if let KeyCode::Char(c) = key.code {
                            if let Some(d) = c.to_digit(10) {
                                answers.fatigue_level = d as u8;
                                step = Step::ThroatCleared;
                            }
                        }
                    }
                    Step::ThroatCleared => match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            answers.throat_cleared = true;
                            step = Step::MucusLevel;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            answers.throat_cleared = false;
                            step = Step::MucusLevel;
                        }
                        _ => {}
                    },
                    Step::MucusLevel => match key.code {
                        KeyCode::Char('1') => {
                            answers.mucus_level = "low".into();
                            step = Step::Hydration;
                        }
                        KeyCode::Char('2') => {
                            answers.mucus_level = "moderate".into();
                            step = Step::Hydration;
                        }
                        KeyCode::Char('3') => {
                            answers.mucus_level = "high".into();
                            step = Step::Hydration;
                        }
                        _ => {}
                    },
                    Step::Hydration => match key.code {
                        KeyCode::Char('1') => {
                            answers.hydration = "low".into();
                            step = Step::Notes;
                        }
                        KeyCode::Char('2') => {
                            answers.hydration = "normal".into();
                            step = Step::Notes;
                        }
                        KeyCode::Char('3') => {
                            answers.hydration = "high".into();
                            step = Step::Notes;
                        }
                        _ => {}
                    },
                    Step::Notes => match key.code {
                        KeyCode::Enter => step = Step::Done,
                        KeyCode::Backspace => {
                            answers.notes_buf.pop();
                        }
                        KeyCode::Char(c) => {
                            answers.notes_buf.push(c);
                        }
                        _ => {}
                    },
                    Step::Done => unreachable!(),
                }

                if matches!(step, Step::Done) {
                    break;
                }
            }
            _ => {}
        }
    }

    let notes = if answers.notes_buf.trim().is_empty() {
        None
    } else {
        Some(answers.notes_buf.trim().to_string())
    };

    Ok(RecordingConditions {
        time_of_day: answers.time_of_day,
        fatigue_level: answers.fatigue_level,
        throat_cleared: answers.throat_cleared,
        mucus_level: answers.mucus_level,
        hydration: answers.hydration,
        notes,
    })
}

fn render(
    frame: &mut ratatui::Frame,
    area: Rect,
    step: &Step,
    answers: &Answers,
) {
    let outer = Block::default()
        .title(" Recording Conditions ")
        .borders(Borders::ALL);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let rows = Layout::vertical([
        Constraint::Length(2), // subtitle
        Constraint::Length(summary_height(step)),
        Constraint::Length(4), // current question
        Constraint::Min(0),   // spacer
        Constraint::Length(1), // key hint
    ])
    .split(inner);

    // Subtitle
    let subtitle = Paragraph::new("  Quick questions to help interpret your results.")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(subtitle, rows[0]);

    // Summary of already-answered fields
    render_summary(frame, rows[1], step, answers);

    // Current question
    render_question(frame, rows[2], step, answers);

    // Key hint
    let hint = current_hint(step);
    frame.render_widget(Paragraph::new(Line::from(hint)), rows[4]);
}

fn summary_height(step: &Step) -> u16 {
    let answered = match step {
        Step::TimeOfDay => 0,
        Step::Fatigue => 1,
        Step::ThroatCleared => 2,
        Step::MucusLevel => 3,
        Step::Hydration => 4,
        Step::Notes => 5,
        Step::Done => 6,
    };
    if answered == 0 {
        0
    } else {
        answered + 3 // header + border
    }
}

fn render_summary(
    frame: &mut ratatui::Frame,
    area: Rect,
    step: &Step,
    answers: &Answers,
) {
    let answered: usize = match step {
        Step::TimeOfDay => 0,
        Step::Fatigue => 1,
        Step::ThroatCleared => 2,
        Step::MucusLevel => 3,
        Step::Hydration => 4,
        Step::Notes => 5,
        Step::Done => 6,
    };

    if answered == 0 {
        return;
    }

    let mut table_rows = Vec::new();

    let fields: Vec<(&str, String)> = vec![
        ("Time of day", answers.time_of_day.clone()),
        ("Fatigue", format!("{}/9", answers.fatigue_level)),
        (
            "Throat cleared",
            if answers.throat_cleared { "yes" } else { "no" }.into(),
        ),
        ("Mucus level", answers.mucus_level.clone()),
        ("Hydration", answers.hydration.clone()),
        (
            "Notes",
            if answers.notes_buf.is_empty() {
                "-".into()
            } else {
                answers.notes_buf.clone()
            },
        ),
    ];

    for (label, value) in fields.into_iter().take(answered) {
        table_rows.push(Row::new(vec![format!("  {}", label), value]));
    }

    let table = Table::new(
        table_rows,
        [Constraint::Length(18), Constraint::Min(20)],
    )
    .header(
        Row::new(vec!["  Field", "Value"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::default().title(" Answers ").borders(Borders::ALL));

    frame.render_widget(table, area);
}

fn render_question(
    frame: &mut ratatui::Frame,
    area: Rect,
    step: &Step,
    answers: &Answers,
) {
    let (question, options) = match step {
        Step::TimeOfDay => (
            "Time of day?",
            format!(
                "[Enter] {} (auto)   [1] morning   [2] afternoon   [3] evening",
                answers.time_of_day
            ),
        ),
        Step::Fatigue => (
            "Fatigue level?",
            "[0] none ... [9] extreme".into(),
        ),
        Step::ThroatCleared => (
            "Cleared throat before recording?",
            "[y] yes   [n] no".into(),
        ),
        Step::MucusLevel => (
            "Mucus level?",
            "[1] low   [2] moderate   [3] high".into(),
        ),
        Step::Hydration => (
            "Hydration?",
            "[1] low   [2] normal   [3] high".into(),
        ),
        Step::Notes => (
            "Notes (optional):",
            format!("{}|", answers.notes_buf),
        ),
        Step::Done => ("", String::new()),
    };

    let text = vec![
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                question,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(options, Style::default().fg(Color::Cyan)),
        ]),
    ];

    frame.render_widget(Paragraph::new(text), area);
}

fn current_hint(step: &Step) -> Vec<Span<'static>> {
    match step {
        Step::TimeOfDay => vec![
            Span::styled(
                "  [1-3]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" select  "),
            Span::styled(
                "[Enter]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" confirm auto"),
        ],
        Step::Fatigue => vec![
            Span::styled(
                "  [0-9]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" select level"),
        ],
        Step::ThroatCleared => vec![
            Span::styled(
                "  [y/n]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" answer"),
        ],
        Step::MucusLevel | Step::Hydration => vec![
            Span::styled(
                "  [1-3]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" select"),
        ],
        Step::Notes => vec![
            Span::raw("  Type notes, "),
            Span::styled(
                "[Enter]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to submit/skip"),
        ],
        Step::Done => vec![],
    }
}
