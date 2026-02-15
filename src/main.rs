mod audio;
mod cli;
mod util;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, RecordCommand};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Devices => audio::devices::list_devices(),

        Command::Record { exercise } => match exercise {
            RecordCommand::MicCheck => audio::mic_check::run(),

            RecordCommand::Sustained { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("sustained", &date)
            }

            RecordCommand::Scale { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("scale", &date)
            }

            RecordCommand::Reading { date } => {
                let date = util::resolve_date(date.as_deref())?;
                audio::recorder::record_exercise("reading", &date)
            }

            RecordCommand::Session { date: _ } => {
                anyhow::bail!("Guided session recording is a Phase 3 feature")
            }
        },

        Command::Play { target, exercise } => {
            audio::playback::play(&target, exercise.as_deref())
        }

        Command::Analyze { .. } => {
            anyhow::bail!("Analysis is a Phase 2 feature")
        }

        Command::Report { .. } => {
            anyhow::bail!("Reports are a Phase 4 feature")
        }

        Command::Compare { .. } => {
            anyhow::bail!("Session comparison is a Phase 4 feature")
        }

        Command::Sessions => {
            anyhow::bail!("Session listing is a Phase 3 feature")
        }
    }
}
