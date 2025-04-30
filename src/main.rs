use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cast_sender::namespace::media::{
    MediaInformationBuilder, MusicTrackMediaMetadataBuilder, StreamType,
};
use cast_sender::{AppId, MediaController, Receiver};
use std::error::Error;
use tokio::sync::mpsc;
use warp::Filter;
use std::net::SocketAddr;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Arc;
use std::sync::Mutex;
use std::env;
use async_stream;
use bytes::Bytes;
use std::convert::Infallible;
use tokio_stream;
use tokio::sync::broadcast;

const CHROMECAST_IP: &str = "192.168.86.238";
const AUDIO_DEVICE_ID: usize = 0;
const HTTP_PORT: u16 = 8080;
const OUTPUT_FILE: &str = "output.wav";

fn create_wav_header(sample_rate: u32, channels: u16, bits_per_sample: u16, data_size: u32) -> Vec<u8> {
    let mut header = Vec::new();
    // RIFF header
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_size).to_le_bytes());
    header.extend_from_slice(b"WAVE");
    
    // fmt chunk
    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    header.extend_from_slice(&1u16.to_le_bytes()); // PCM format (1 = LPCM)
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    header.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * bits_per_sample / 8;
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits_per_sample.to_le_bytes());
    
    // data chunk
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_size.to_le_bytes());
    
    header
}

// Add a function to write the WAV file
fn write_wav_file(audio_data: &[u8], sample_rate: u32) -> Result<(), Box<dyn Error>> {
    let file = File::create(OUTPUT_FILE)?;
    let mut writer = BufWriter::new(file);
    
    let header = create_wav_header(
        sample_rate,
        2, // stereo
        16, // 16-bit
        audio_data.len() as u32
    );
    
    writer.write_all(&header)?;
    writer.write_all(audio_data)?;
    
    println!("Wrote {} bytes to {}", audio_data.len() + header.len(), OUTPUT_FILE);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let debug_mode = args.iter().any(|arg| arg == "--debug" || arg == "--d");

    // Set up audio capture from specified device
    let host = cpal::default_host();
    let devices = host.input_devices()?;
    
    // Try to get the audio device with above ID
    let device = devices
        .enumerate()
        .filter(|(id, _)| *id == AUDIO_DEVICE_ID)
        .map(|(_, device)| device)
        .next()
        .ok_or("Could not find audio device with given ID")?;
    
    println!("Using input device: {}", device.name()?);
    
    // Get a supported configuration for the audio input
    let config = device.default_input_config()?;
    println!("Default input config: {:?}", config);

    if debug_mode {
        // Debug mode: Write to WAV file
        let audio_data = Arc::new(Mutex::new(Vec::new()));
        let audio_data_clone = audio_data.clone();
        let sample_rate = config.sample_rate().0;
        
        // Build the input stream
        let input_stream = device.build_input_stream(
            &config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Convert audio data to bytes and store in buffer
                let mut bytes = Vec::with_capacity(data.len() * 2);
                
                for chunk in data.chunks(10) { // Process all 10 channels at once
                    if chunk.len() >= 10 {
                        // Use channels 1 and 2 (indices 0 and 1) for stereo output
                        let left_sample = (chunk[0] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        let right_sample = (chunk[1] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        
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
            if let Err(e) = write_wav_file(&audio_data, sample_rate) {
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
        let input_stream = device.build_input_stream(
            &config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Convert audio data to bytes and send through channel
                let mut bytes = Vec::with_capacity(data.len() * 2);
                
                for chunk in data.chunks(10) { // Process all 10 channels at once
                    if chunk.len() >= 10 {
                        // Use channels 1 and 2 (indices 0 and 1) for stereo output
                        let left_sample = (chunk[0] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        let right_sample = (chunk[1] * i16::MAX as f32).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                        
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
        let local_ip = local_ip_address::local_ip()?;
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
                            println!("Serving audio data: {} bytes", data.len());
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
                println!("Received audio data: {} bytes", data_to_send.len());
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