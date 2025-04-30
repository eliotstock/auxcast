use cast_sender;
use cast_sender::namespace::media::{
    MediaInformationBuilder, MusicTrackMediaMetadataBuilder, StreamType,
};
use cast_sender::{AppId, Error, MediaController, Receiver};
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::io::{self, Read};
use anyhow;
use cpal;

// Get these from the Home app on your phone
// TODO: How do we send to a speaker group?
const CHROMECAST_IP_A: &str = "192.168.86.20"; // works
const CHROMECAST_IP_B: &str = "192.168.86.29"; // works
const CHROMECAST_IP_C: &str = "192.168.86.238"; // works

const CHROMECAST_IP: &str = CHROMECAST_IP_C;

// To find this on a Mac, assuming we're using WiFi:
// ipconfig getifaddr en0
const STREAM_IP: &str = "192.168.86.247";
const STREAM_PORT: u16 = 8080;

async fn cast(stream_port: u16) -> Result<(), Error> {
    let receiver = Receiver::new();
    receiver.connect(CHROMECAST_IP).await?;

    let app = receiver.launch_app(AppId::DefaultMediaReceiver).await?;
    let media_controller = MediaController::new(app.clone(), receiver.clone())?;

    let metadata = MusicTrackMediaMetadataBuilder::default()
        .title("Live Audio Stream")
        .build()
        .unwrap();

    // Create a streaming URL that points to our local server
    let stream_url = format!("http://{}:{}", STREAM_IP, stream_port);
    
    let media_info = MediaInformationBuilder::default()
        .content_id(&stream_url)
        .stream_type(StreamType::Live)
        .content_type("audio/wav")
        .metadata(metadata)
        .build()
        .unwrap();

    media_controller.load(media_info).await?;
    Ok(())
}

fn stream_from_input() -> Result<u16> {
    println!("Starting audio streaming server...");
    
    // Set up UDP socket for streaming
    let socket_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        0 // Let the OS assign an available port
    );
    let socket = Arc::new(UdpSocket::bind(socket_addr)?);
    let bound_addr = socket.local_addr()?;
    println!("UDP socket bound to {}", bound_addr);

    // Find the default audio host
    let host = cpal::default_host();

    // List all input devices
    println!("Available input devices:");
    let devices: Vec<_> = host.devices()?.collect();
    for (device_index, device) in devices.iter().enumerate() {
        if let Ok(name) = device.name() {
            println!("  {}. {}", device_index + 1, name);
        }
    }

    // Wait for user input
    println!("\nSelect a device (1-9): ");
    let mut input = [0u8; 1];
    io::stdin().read_exact(&mut input)?;
    let selection = input[0] as char;
    
    if !selection.is_digit(10) || selection == '0' {
        return Err(anyhow::anyhow!("Invalid selection. Please choose a number between 1-9"));
    }
    
    let device_index = selection.to_digit(10).unwrap() as usize - 1;
    if device_index >= devices.len() {
        return Err(anyhow::anyhow!("Invalid device number"));
    }

    // Get the selected device
    let input_device = &devices[device_index];
    println!("Selected device: {}", input_device.name()?);
    
    // Get the default input config
    let config = input_device
        .default_input_config()
        .context("Failed to get default input config")?;
    println!("Default config: {:?}", config);
    
    // Create a buffer to share between the audio thread and network thread
    let buffer = Arc::new(Mutex::new(Vec::new()));
    
    // Set up the audio stream
    let buffer_clone = Arc::clone(&buffer);
    let stream = match config.sample_format() {
        SampleFormat::F32 => stream_setup::<f32>(&input_device, &config.into(), buffer_clone)?,
        SampleFormat::I16 => stream_setup::<i16>(&input_device, &config.into(), buffer_clone)?,
        SampleFormat::U16 => stream_setup::<u16>(&input_device, &config.into(), buffer_clone)?,
        _ => return Err(anyhow::anyhow!("Unsupported sample format")),
    };
    
    // Start the stream
    stream.play()?;
    println!("Audio stream started");
    
    // Define the target - this is where we'll send the audio data
    let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), STREAM_PORT);
    println!("Streaming audio to {}", target);
    
    // Set up a thread for network sending
    let socket_clone = Arc::clone(&socket);
    let buffer_clone = Arc::clone(&buffer);
    
    // Create a separate thread for sending data to avoid blocking the main thread
    let _sender_thread = thread::spawn(move || {
        const MAX_PACKET_SIZE: usize = 1400; // Conservative UDP packet size
        loop {
            let data = {
                match buffer_clone.lock() {
                    Ok(mut buffer) => {
                        if buffer.is_empty() {
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        
                        let data = buffer.clone();
                        buffer.clear();
                        data
                    },
                    Err(_) => {
                        eprintln!("Failed to acquire lock on buffer");
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                }
            };
            
            if !data.is_empty() {
                // Split the data into chunks of MAX_PACKET_SIZE
                for chunk in data.chunks(MAX_PACKET_SIZE) {
                    if let Err(e) = socket_clone.send_to(chunk, target) {
                        eprintln!("Failed to send data chunk: {}", e);
                    }
                    // Small delay between chunks to avoid overwhelming the network
                    thread::sleep(Duration::from_micros(100));
                }
            }
        }
    });
    
    Ok(bound_addr.port())
}

fn stream_setup<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buffer: Arc<Mutex<Vec<u8>>>,
) -> Result<Stream>
where
    T: cpal::Sample + cpal::SizedSample + Send + 'static,
{
    let err_fn = |err| eprintln!("an error occurred on the audio stream: {}", err);
    
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            // This is called in a real-time thread, so we should be careful
            // Convert the audio data to bytes
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    data.as_ptr() as *const u8,
                    data.len() * std::mem::size_of::<T>(),
                )
            };
            
            // Update the buffer directly, but be quick about it
            if let Ok(mut buffer_lock) = buffer.lock() {
                buffer_lock.extend_from_slice(bytes);
                // If buffer gets too large, truncate it to avoid memory issues
                if buffer_lock.len() > 1024 * 1024 {  // 1MB limit
                    let new_start = buffer_lock.len() - (512 * 1024);  // Keep last 512KB
                    buffer_lock.drain(0..new_start);
                }
            }
        },
        err_fn,
        None,
    )?;
    
    Ok(stream)
}

#[tokio::main]
async fn main() -> Result<()> {
    let stream_port = stream_from_input()?;
    cast(stream_port).await?;
    
    // Keep the main thread alive
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
