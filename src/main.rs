use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cast_sender::namespace::media::{
    MediaInformationBuilder, MusicTrackMediaMetadataBuilder, StreamType,
};
use cast_sender::{AppId, MediaController, Receiver};
use std::error::Error;
use tokio::sync::mpsc;
use warp::Filter;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::env;
use async_stream;
use bytes::Bytes;
use std::convert::Infallible;
use auxcast::{create_wav_header, write_wav_file};
use auxcast::discover_devices;
use dialoguer::{theme::ColorfulTheme, Select};
use local_ip_address::local_ip;
use anyhow::Result;

const CHROMECAST_IP: &str = "192.168.86.29";
const HTTP_PORT: u16 = 8080;
const OUTPUT_FILE: &str = "output.wav";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Hit Ctrl-C to stop casting");

    let args: Vec<String> = env::args().collect();
    let debug_mode = args.iter().any(|arg| arg == "--debug" || arg == "-d");

    // List all available audio input devices
    let host = cpal::default_host();
    let devices: Vec<_> = host.input_devices()?.collect();
    
    // Collect device names into a vector
    let device_names: Vec<String> = devices
        .iter()
        .map(|device| device.name().unwrap_or_else(|_| "Unknown Device".to_string()))
        .collect();

    if device_names.is_empty() {
        println!("No audio input devices found!");
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

    println!("Selected audio input device: {}", selected_device.name()?);

    // Get a supported configuration for the audio input
    let config = selected_device.default_input_config()?;
    println!("Default input config: {:?}", config);
    let num_channels = config.channels() as usize;

    if debug_mode {
        // Debug mode: Write to WAV file
        let audio_data = Arc::new(Mutex::new(Vec::new()));
        let audio_data_clone = audio_data.clone();
        let sample_rate = config.sample_rate().0;
        
        // Build the input stream
        let input_stream = selected_device.build_input_stream(
            &config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Convert audio data to bytes and store in buffer
                let mut bytes = Vec::with_capacity(data.len() * 2);
                
                // Process each sample pair (left and right) directly
                for samples in data.chunks(num_channels) {
                    if samples.len() >= 2 {
                        let left_sample = (samples[0] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        let right_sample = (samples[1] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        
                        bytes.extend_from_slice(&left_sample.to_le_bytes());
                        bytes.extend_from_slice(&right_sample.to_le_bytes());
                    }
                }
                
                let mut audio_data = audio_data_clone.lock().unwrap();
                audio_data.extend_from_slice(&bytes);
            },
            |err| eprintln!("Error in input stream: {}", err),
            None,
        )?;
        
        // Start the audio input stream
        input_stream.play()?;
        println!("Started audio input stream");
        println!("Recording audio to {}. Press Ctrl+C to stop.", OUTPUT_FILE);
        
        // Wait for Ctrl+C
        ctrlc::set_handler(move || {
            println!("Stopping recording...");
            let audio_data = audio_data.lock().unwrap();
            if let Err(e) = write_wav_file(&audio_data, sample_rate, OUTPUT_FILE) {
                eprintln!("Error writing WAV file: {}", e);
            }
            std::process::exit(0);
        })?;
        
        // Keep the program running
        std::thread::park();
    } else {
        // Chromecast mode: Stream to Chromecast
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(100);
        
        // Build the input stream
        let input_stream = selected_device.build_input_stream(
            &config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Convert audio data to bytes and send through channel
                let mut bytes = Vec::with_capacity(data.len() * 2);
                
                // Process each sample pair (left and right) directly
                for samples in data.chunks(num_channels) {
                    if samples.len() >= 2 {
                        let left_sample = (samples[0] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        let right_sample = (samples[1] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        
                        bytes.extend_from_slice(&left_sample.to_le_bytes());
                        bytes.extend_from_slice(&right_sample.to_le_bytes());
                    }
                }
                
                if let Err(e) = tx.blocking_send(bytes) {
                    eprintln!("Error sending audio data: {}", e);
                }
            },
            |err| eprintln!("Error in input stream: {}", err),
            None,
        )?;
        
        // Start the HTTP server
        let local_ip = local_ip()?;
        let server_addr = SocketAddr::new(local_ip, HTTP_PORT);
        let audio_buffer = Arc::new(Mutex::new(Vec::new()));
        let audio_buffer_http = audio_buffer.clone();
        let sample_rate = config.sample_rate().0;
        
        let audio_route = warp::path("audio")
            .map(move || {
                let audio_buffer = audio_buffer_http.clone();
                let stream = async_stream::stream! {
                    // Create WAV header
                    let header = create_wav_header(
                        sample_rate,
                        2, // stereo
                        16, // 16-bit
                        0 // We'll update this later
                    );
                    
                    // Send header first
                    yield Ok::<Bytes, Infallible>(header.into());
                    
                    loop {
                        let data = {
                            let mut buffer = audio_buffer.lock().unwrap();
                            if buffer.len() > 0 {
                                let data = buffer.clone();
                                buffer.clear();
                                data
                            } else {
                                vec![]
                            }
                        };
                        if !data.is_empty() {
                            // println!("Serving audio data: {} bytes", data.len());
                            yield Ok::<Bytes, Infallible>(data.into());
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    }
                };
                
                let resp = warp::http::Response::builder()
                    .header("Content-Type", "audio/wav")
                    .header("Transfer-Encoding", "chunked")
                    .header("Connection", "keep-alive")
                    .header("Cache-Control", "no-cache")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(warp::hyper::Body::wrap_stream(stream))
                    .unwrap();
                
                warp::reply::Response::new(resp.into_body())
            });
        
        tokio::spawn(async move {
            warp::serve(audio_route)
                .run(server_addr)
                .await;
        });
        
        println!("HTTP server running at http://{}:{}", local_ip, HTTP_PORT);

        let cast_devices = discover_devices().await?;

         // Print results
        if cast_devices.is_empty() {
            println!("No Chromecast devices found");
        } else {
            println!("\nFound {} Chromecast device(s):", cast_devices.len());
            for device in cast_devices {
                println!("\n{}: {} ({})", 
                    if device.is_group { "Speaker Group" } else { "Device" },
                    device.name, device.ip);
                if debug_mode {
                    println!("Records:");
                    for record in device.records {
                        println!("  {}", record);
                    }
                }
            }
        }
        
        // Connect to the Chromecast device
        println!("Connecting to Chromecast at {}", CHROMECAST_IP);
        let receiver = Receiver::new();
        receiver.connect(CHROMECAST_IP).await?;
        println!("Connected to Chromecast");
        
        let app = receiver.launch_app(AppId::DefaultMediaReceiver).await?;
        println!("Launched Default Media Receiver app");
        
        let media_controller = MediaController::new(app.clone(), receiver.clone())?;
        println!("Created media controller");
        
        // Start the audio input stream
        input_stream.play()?;
        println!("Started audio input stream");
        
        println!("Streaming audio to Chromecast. Press Ctrl+C to stop.");
        
        // Start a task to update the audio data
        let audio_task = tokio::spawn(async move {
            while let Some(data_to_send) = rx.recv().await {
                // println!("Received audio data: {} bytes", data_to_send.len());
                let mut buffer = audio_buffer.lock().unwrap();
                buffer.extend_from_slice(&data_to_send);
            }
        });
        
        // Create media data packet
        let media_info = MediaInformationBuilder::default()
            .content_id(&format!("http://{}:{}/audio", local_ip, HTTP_PORT))
            .stream_type(StreamType::Buffered)
            .content_type("audio/wav")
            .metadata(
                MusicTrackMediaMetadataBuilder::default()
                    .title("Live Audio Stream")
                    .build()
                    .unwrap()
            )
            .build()
            .unwrap();
        
        // Send to Chromecast
        println!("Sending media info to Chromecast...");
        println!("Stream URL: http://{}:{}/audio", local_ip, HTTP_PORT);
        match media_controller.load(media_info).await {
            Ok(_) => println!("Media info sent successfully"),
            Err(e) => eprintln!("Error sending media data: {}", e),
        }
        
        // Wait for Ctrl+C
        tokio::signal::ctrl_c().await?;
        
        // Clean up
        audio_task.abort();
        input_stream.pause()?;
        
        println!("Shutting down...");
    }
    
    Ok(())
}