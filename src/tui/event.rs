use std::sync::mpsc;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent};

/// Events consumed by the TUI main loop.
pub enum AppEvent {
    /// A keyboard event.
    Key(KeyEvent),
    /// Terminal was resized.
    #[allow(dead_code)]
    Resize(u16, u16),
    /// Periodic tick for driving render updates.
    Tick,
}

/// Polls crossterm events and sends them to the main render loop.
///
/// Runs in a background thread. Sends Key and Resize events as they arrive,
/// plus a Tick at ~30 fps when no other events occur.
pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    _handle: std::thread::JoinHandle<()>,
}

impl EventHandler {
    /// Start the event polling thread.
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();

        let handle = std::thread::spawn(move || loop {
            if event::poll(tick_rate).unwrap_or(false) {
                match event::read() {
                    Ok(Event::Key(key)) => {
                        if tx.send(AppEvent::Key(key)).is_err() {
                            return;
                        }
                    }
                    Ok(Event::Resize(w, h)) => {
                        if tx.send(AppEvent::Resize(w, h)).is_err() {
                            return;
                        }
                    }
                    _ => {}
                }
            } else {
                // No event within tick_rate — send a tick to drive re-render
                if tx.send(AppEvent::Tick).is_err() {
                    return;
                }
            }
        });

        Self {
            rx,
            _handle: handle,
        }
    }

    /// Receive the next event, blocking until one is available.
    pub fn next(&self) -> Result<AppEvent, mpsc::RecvError> {
        self.rx.recv()
    }
}
