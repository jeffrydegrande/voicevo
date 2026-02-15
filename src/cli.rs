use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "voice-tracker")]
#[command(about = "Track vocal cord recovery with objective measurements")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// List available audio input devices
    Devices,

    /// Record a voice exercise
    Record {
        #[command(subcommand)]
        exercise: RecordCommand,
    },

    /// Play back a recording
    Play {
        /// Date (YYYY-MM-DD) or path to a WAV file
        target: String,

        /// Exercise name (required when target is a date)
        exercise: Option<String>,
    },

    /// Analyze recorded sessions
    Analyze {
        /// Date of the session to analyze
        #[arg(long)]
        date: Option<String>,

        /// Re-analyze all sessions
        #[arg(long)]
        all: bool,
    },

    /// Generate trend reports
    Report {
        /// Number of recent sessions to include
        #[arg(long)]
        last: Option<usize>,

        /// Include all sessions
        #[arg(long)]
        all: bool,
    },

    /// Compare two sessions side by side
    Compare {
        /// Baseline session date
        #[arg(long)]
        baseline: String,

        /// Current session date
        #[arg(long)]
        current: String,
    },

    /// List all recorded sessions
    Sessions,
}

#[derive(Subcommand)]
pub enum RecordCommand {
    /// Quick 2-second mic level check
    MicCheck,

    /// Record a sustained vowel ("AAAH")
    Sustained {
        /// Recording date (defaults to today)
        #[arg(long)]
        date: Option<String>,
    },

    /// Record a chromatic scale (low to high and back)
    Scale {
        /// Recording date (defaults to today)
        #[arg(long)]
        date: Option<String>,
    },

    /// Record a reading passage
    Reading {
        /// Recording date (defaults to today)
        #[arg(long)]
        date: Option<String>,
    },

    /// Run a full guided session (all exercises)
    Session {
        /// Recording date (defaults to today)
        #[arg(long)]
        date: Option<String>,
    },
}
