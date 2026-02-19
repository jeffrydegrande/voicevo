use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "voicevo")]
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

        /// Store results at a specific analysis version (default: current)
        #[arg(long)]
        version: Option<u32>,
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

    /// Get an LLM interpretation of a session's analysis
    Explain {
        /// Session date (defaults to today)
        #[arg(long)]
        date: Option<String>,

        /// LLM provider: "claude" or "gpt"
        #[arg(long, default_value = "claude")]
        provider: String,

        /// Model override (ignores --fast/--think)
        #[arg(long)]
        model: Option<String>,

        /// Use fastest/cheapest model (Haiku / GPT-5.2)
        #[arg(long, conflicts_with = "think")]
        fast: bool,

        /// Use most capable model (Opus / GPT-5.2-pro)
        #[arg(long, conflicts_with = "fast")]
        think: bool,

        /// Deep report: get interpretations from both Claude and GPT,
        /// then synthesize and fact-check the findings
        #[arg(long)]
        deep: bool,
    },

    /// Discard the latest recording attempt for an exercise
    Discard {
        /// Exercise name (sustained, scale, reading). If omitted, discards the most recently modified recording.
        exercise: Option<String>,

        /// Date of the recording (defaults to today)
        #[arg(long)]
        date: Option<String>,
    },

    /// Open the latest report in your default viewer
    Browse,

    /// Interactive voice exercises with real-time feedback
    Exercise {
        #[command(subcommand)]
        exercise: ExerciseCommand,
    },

    /// Export all data as LLM-friendly markdown and copy to clipboard
    Dump,

    /// Migrate JSON session files to SQLite database
    Migrate,

    /// Show where data and config files are stored
    Paths,
}

#[derive(Subcommand)]
pub enum ExerciseCommand {
    /// Sustained phonation: hold "AAAH" with live timer and volume meter
    Sustain,

    /// S/Z ratio test: sustain /s/ then /z/ to measure glottal efficiency
    Sz,

    /// Fatigue slope: 5 sustained trials to measure vocal endurance
    Fatigue,

    /// Chromatic scale with live pitch feedback
    Scale,
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
