use cast_sender;
use cast_sender::namespace::media::{
    MediaInformationBuilder, MusicTrackMediaMetadataBuilder, StreamType,
};
use cast_sender::{AppId, Error, ImageBuilder, MediaController, Receiver};
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

const CHROMECAST_IP_A: &str = "192.168.86.247"; // works
const CHROMECAST_IP_B: &str = "192.168.86.29"; // works
const CHROMECAST_IP_C: &str = "192.168.86.238"; // works

const CHROMECAST_IP: &str = CHROMECAST_IP_C;

const STREAM_IP: &str = "127.0.0.1";
const STREAM_PORT: &str = "8080";

async fn cast() -> Result<(), Error> {
    let receiver = Receiver::new();
    receiver.connect(CHROMECAST_IP).await?;

    let app = receiver.launch_app(AppId::DefaultMediaReceiver).await?;
    let media_controller = MediaController::new(app.clone(), receiver.clone())?;

    let metadata = MusicTrackMediaMetadataBuilder::default()
        .title("Local stream")
        .build()
        .unwrap();

    // Was: http://stream.live.vc.bbcmedia.co.uk/bbc_world_service
    let media_info = MediaInformationBuilder::default()
        .content_id("http://127.0.0.1:8080")
        .stream_type(StreamType::Live)
        .content_type("audio/*")
        .metadata(metadata)
        .build()
        .unwrap();

    // let media_info_local_file = MediaInformationBuilder::default()
    //     .content_id("file:///Users/e/Downloads/test.mp3")
    //     .stream_type(StreamType::Live)
    //     .content_type("audio/*")
    //     .build()
    //     .unwrap();

    media_controller.load(media_info).await?;
    media_controller.start().await?;

    // media_controller.stop().await?;
    Ok(())
}

// async fn get_external_ip() -> Result<String, reqwest::Error> {
//     let response = reqwest::get("https://api.ipify.org").await?;
//     response.text().await
// }

// fn main() -> Result<(), anyhow::Error> {
//     // Get local IP first
//     let ip = local_ip().unwrap();
//     let stream_address = format!("{}:{}", STREAM_IP, STREAM_PORT);
//     let listener = TcpListener::bind(&stream_address)?;
//     println!("Server listening on {}:{}", STREAM_IP, STREAM_PORT);

//     let available_hosts = cpal::available_hosts();
//     println!("Available hosts:\n  {:?}", available_hosts);

//     for host_id in available_hosts {
//         println!("{}", host_id.name());
//         let host = cpal::host_from_id(host_id)?;

//         let default_in = host.default_input_device().map(|e| e.name().unwrap());
//         let default_out = host.default_output_device().map(|e| e.name().unwrap());
//         println!("  Default Input Device:\n    {:?}", default_in);
//         println!("  Default Output Device:\n    {:?}", default_out);

//         let devices = host.devices()?;
//         println!("  Devices: ");
//         for (device_index, device) in devices.enumerate() {
//             println!("  {}. \"{}\"", device_index + 1, device.name()?);

//             let mut supported_configs_range = device.supported_output_configs()
//                 .expect("error while querying configs");

//             // This should be this one on my machine:
//             // SupportedStreamConfigRange { channels: 20, min_sample_rate: SampleRate(44100), max_sample_rate: SampleRate(44100), buffer_size: Range { min: 14, max: 4096 }, sample_format: F32 }
//             let supported_config = supported_configs_range.next()
//                 .expect("no supported config?!")
//                 .with_max_sample_rate();
//             println!("  Stream config: {:?}. ", supported_config);

//             let stream = device.build_output_stream(
//                 &supported_config.into(),
//                 move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
//                     // react to stream events and read or write stream data here.
//                 },
//                 move |err| {
//                     // react to errors here.
//                 },
//                 None // None=blocking, Some(Duration)=timeout
//             )?;
//             // println!("  Stream: {:?}. ", stream.);

//             stream.play()?;

//             for stream in listener.incoming() {
//                 println!("Incoming connection");
                
//                 let mut stream = stream?;
//                 let response = "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\n\r\n";
//                 stream.write_all(response.as_bytes())?;

//                 // Send audio data to the client
//                 // This is a simplified example and doesn't include proper WAV headers
//                 loop {
//                     let buffer = vec![0.0f32; 1024];
//                     // Fill buffer with audio data from the input stream
//                     stream.write_all(bytemuck::cast_slice(&buffer))?;
//                 }
//             }

//             // // Input configs
//             // if let Ok(conf) = device.default_input_config() {
//             //     println!("    Default input stream config:\n      {:?}", conf);
//             // }
//             // let input_configs = match device.supported_input_configs() {
//             //     Ok(f) => f.collect(),
//             //     Err(e) => {
//             //         println!("    Error getting supported input configs: {:?}", e);
//             //         Vec::new()
//             //     }
//             // };
//             // if !input_configs.is_empty() {
//             //     println!("    All supported input stream configs:");
//             //     for (config_index, config) in input_configs.into_iter().enumerate() {
//             //         println!(
//             //             "      {}.{}. {:?}",
//             //             device_index + 1,
//             //             config_index + 1,
//             //             config
//             //         );

//             //         if device_index == 0 && config_index == 0 {
                
//             //             // stream.play()?;
//             //         }
//             //     }
//             // }

//             // // Output configs
//             // if let Ok(conf) = device.default_output_config() {
//             //     println!("    Default output stream config:\n      {:?}", conf);
//             // }
//             // let output_configs = match device.supported_output_configs() {
//             //     Ok(f) => f.collect(),
//             //     Err(e) => {
//             //         println!("    Error getting supported output configs: {:?}", e);
//             //         Vec::new()
//             //     }
//             // };
//             // if !output_configs.is_empty() {
//             //     println!("    All supported output stream configs:");
//             //     for (config_index, config) in output_configs.into_iter().enumerate() {
//             //         println!(
//             //             "      {}.{}. {:?}",
//             //             device_index + 1,
//             //             config_index + 1,
//             //             config
//             //         );
//             //     }
//             // }
//         }
//     }

//     Ok(())
// }

fn stream_from_input() -> Result<()> {
    println!("Starting audio streaming server...");
    
    // Set up UDP socket for streaming
    // We'll bind to any available port, not 8080
    let socket_addr = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        0 // Let the OS assign an available port
    );
    let socket = Arc::new(UdpSocket::bind(socket_addr)?);
    println!("UDP socket bound to {}", socket.local_addr()?);

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
    println!("\nSelect a device: ");
    let mut input = [0u8; 1];
    io::stdin().read_exact(&mut input)?;
    let selection = input[0] as char;
    
    if !selection.is_digit(10) || selection == '0' {
        return Err(anyhow::anyhow!("Invalid selection. Please choose a device ID above."));
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
    // Now we're sending to port 8080, not binding to it
    let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
    println!("Streaming audio to {}", target);
    println!("Start the receiver with: nc -ul 127.0.0.1 8080 > audio_data.raw");
    
    // Set up a thread for network sending
    let socket_clone = Arc::clone(&socket);
    let buffer_clone = Arc::clone(&buffer);
    
    // Create a separate thread for sending data to avoid blocking the main thread
    let _sender_thread = thread::spawn(move || {
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
                if let Err(e) = socket_clone.send_to(&data, target) {
                    eprintln!("Failed to send data: {}", e);
                }
            }
        }
    });

    Ok(())
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
    stream_from_input()?;
    cast().await?;
    
    // Keep the main thread alive
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
