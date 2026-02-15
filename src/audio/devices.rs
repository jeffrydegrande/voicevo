use anyhow::Result;
use console::style;
use cpal::traits::{DeviceTrait, HostTrait};

/// List all available audio input devices with their supported configurations.
pub fn list_devices() -> Result<()> {
    let host = cpal::default_host();

    let default_device = host.default_input_device();
    let default_name = default_device
        .as_ref()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let devices: Vec<_> = host.input_devices()?.collect();

    if devices.is_empty() {
        eprintln!("No audio input devices found.");
        return Ok(());
    }

    println!("{}", style("Audio Input Devices").bold());
    println!();

    for device in &devices {
        let name = device.name().unwrap_or_else(|_| "<unknown>".into());
        let is_default = name == default_name;

        if is_default {
            print!("  {} ", style("*").green().bold());
            print!("{}", style(&name).green().bold());
        } else {
            print!("    {}", style(&name).bold());
        }
        println!();

        match device.supported_input_configs() {
            Ok(configs) => {
                for cfg in configs {
                    let channels = cfg.channels();
                    let min_rate = cfg.min_sample_rate().0;
                    let max_rate = cfg.max_sample_rate().0;
                    let format = cfg.sample_format();

                    if min_rate == max_rate {
                        println!(
                            "      {channels}ch  {min_rate} Hz  {format:?}"
                        );
                    } else {
                        println!(
                            "      {channels}ch  {min_rate}-{max_rate} Hz  {format:?}"
                        );
                    }
                }
            }
            Err(e) => {
                println!("      Could not query configs: {e}");
            }
        }
        println!();
    }

    if !default_name.is_empty() {
        println!(
            "  {} = default device",
            style("*").green().bold()
        );
    }

    Ok(())
}
