use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::error::Error;
use tokio::sync::mpsc;
use std::sync::Arc;
use std::sync::Mutex;
use std::env;
use auxcast::discover_devices;
use dialoguer::{theme::ColorfulTheme, Select};
use local_ip_address::local_ip;
use anyhow::Result;

const HTTP_PORT: u16 = 8080;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let debug_mode = args.iter().any(|arg| arg == "--debug" || arg == "-d");
    let verbose_mode = args.iter().any(|arg| arg == "--verbose" || arg == "-v");

    // Get local IP address
    let local_ip = local_ip()?;

    // List all available audio input devices
    let host = cpal::default_host();
    let devices: Vec<_> = host.input_devices()?.collect();
    
    // Collect device names into a vector
    let device_names: Vec<String> = devices
        .iter()
        .map(|device| device.name().unwrap_or_else(|_| "Unknown Device".to_string()))
        .collect();

    if device_names.is_empty() {
        println!("No audio input devices found.");
        return Ok(());
    }

    // Create an interactive selection prompt
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select an audio input device")
        .items(&device_names)
        .default(0)
        .interact()?;

    // Get the selected device
    let selected_device = devices
        .get(selection)
        .ok_or_else(|| anyhow::anyhow!("Failed to get selected device"))?;

    // Get a supported configuration for the audio input
    let config = selected_device.default_input_config()?;
    let num_channels = config.channels() as usize;

    if verbose_mode {
        println!("Using audio input device: {}, with {} channel(s)", selected_device.name()?, num_channels);
    }

    println!("Discovering Chromecast devices...");
    let cast_devices = discover_devices().await?;

    // Print results
    if cast_devices.is_empty() {
        println!("No Chromecast devices found.");
        return Ok(());
    }

    // Create device options for the dropdown
    let device_options: Vec<String> = cast_devices
        .iter()
        .map(|device| {
            format!("{}: {} {}", 
                if device.is_group { "Speaker Group" } else { "Device" },
                device.name,
                if verbose_mode { format!("({})", device.ip) } else { "".to_string() })
        })
        .collect();

    // Create an interactive selection prompt
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select a Chromecast device")
        .items(&device_options)
        .default(0)
        .interact()?;

    // Get the selected device
    let selected_cast_device = cast_devices
        .get(selection)
        .ok_or_else(|| anyhow::anyhow!("Failed to get selected device"))?;

    if debug_mode {
        println!("Selected device details:");
        println!("Records:");
        for record in &selected_cast_device.records {
            println!("  {}", record);
        }
    }

    // Chromecast mode: Stream to Chromecast
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(100);
    
    // Build the input stream
    let input_stream = auxcast::build_input_stream_and_sender(
        selected_device,
        &config,
        num_channels,
        tx.clone(),
    )?;
    
    // Prepare audio buffer and sample rate for HTTP server
    let audio_buffer = Arc::new(Mutex::new(Vec::new()));
    let sample_rate = config.sample_rate().0;

    // Start the HTTP server
    auxcast::spawn_audio_http_server(audio_buffer.clone(), sample_rate, HTTP_PORT);
    
    if verbose_mode {
        println!("HTTP server running at http://{}:{}", local_ip, HTTP_PORT);
        println!("Connecting to Chromecast at {}", selected_cast_device.ip);
    }
    
    // Start the audio input stream
    input_stream.play()?;

    if verbose_mode {
        println!("Started audio input stream");
    }
    
    // Start a task to update the audio data
    let audio_task = tokio::spawn(async move {
        while let Some(data_to_send) = rx.recv().await {
            let mut buffer = audio_buffer.lock().unwrap();
            buffer.extend_from_slice(&data_to_send);
        }
    });

    // Connect and stream to Chromecast
    auxcast::connect_and_stream_to_chromecast(selected_cast_device, local_ip, HTTP_PORT, verbose_mode).await?;
    
    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await?;
    
    // Clean up
    audio_task.abort();
    input_stream.pause()?;
    
    println!("Shutting down...");
    
    Ok(())
}