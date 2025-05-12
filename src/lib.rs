use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use futures_util::{pin_mut, stream::StreamExt};
use mdns::{Record, RecordKind};
use std::{net::IpAddr, time::Duration, collections::HashMap};
use std::sync::{Arc, Mutex};
use cpal::{self, traits::{DeviceTrait, StreamTrait}};
use std::i16;
use ctrlc;
use warp;
use async_stream;
use bytes;
use local_ip_address;
use cast_sender::{Receiver, AppId, MediaController};
use cast_sender::namespace::media::{MediaInformationBuilder, MusicTrackMediaMetadataBuilder, StreamType};

pub fn create_wav_header(sample_rate: u32, channels: u16, bits_per_sample: u16, data_size: u32) -> Vec<u8> {
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

pub fn write_wav_file(audio_data: &[u8], sample_rate: u32, output_file: &str) -> Result<(), Box<dyn Error>> {
    let file = File::create(output_file)?;
    let mut writer = BufWriter::new(file);
    
    let header = create_wav_header(
        sample_rate,
        2, // stereo
        16, // 16-bit
        audio_data.len() as u32
    );
    
    writer.write_all(&header)?;
    writer.write_all(audio_data)?;
    
    println!("Wrote {} bytes to {}", audio_data.len() + header.len(), output_file);
    Ok(())
}

// The service of the devices we are searching for.
// Every Chromecast will respond to the service name in this example.
const SERVICE_NAME: &'static str = "_googlecast._tcp.local";

#[derive(Debug, Clone)]
pub struct ChromecastDevice {
    pub name: String,
    pub ip: IpAddr,
    pub is_group: bool,
    pub records: Vec<String>,
}

fn to_ip_addr(record: &Record) -> Option<IpAddr> {
    match record.kind {
        RecordKind::A(addr) => Some(addr.into()),
        RecordKind::AAAA(addr) => Some(addr.into()),
        _ => None,
    }
}

pub fn record_audio_to_file(
    device: &cpal::Device,
    config: cpal::SupportedStreamConfig,
    num_channels: usize,
    output_file: &str,
) -> Result<(), Box<dyn Error>> {
    let audio_data = Arc::new(Mutex::new(Vec::new()));
    let audio_data_clone = audio_data.clone();
    let sample_rate = config.sample_rate().0;
    let output_file = output_file.to_string();
    
    let input_stream = device.build_input_stream(
        &config.config(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut bytes = Vec::with_capacity(data.len() * 2);
            
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
    
    input_stream.play()?;
    println!("Started audio input stream");
    println!("Recording audio to {}. Press Ctrl+C to stop.", output_file);
    
    ctrlc::set_handler(move || {
        println!("Stopping recording...");
        let audio_data = audio_data.lock().unwrap();
        if let Err(e) = write_wav_file(&audio_data, sample_rate, &output_file) {
            eprintln!("Error writing WAV file: {}", e);
        }
        std::process::exit(0);
    })?;
    
    std::thread::park();
    Ok(())
}

/// Discovers Chromecast devices and speaker groups on the network.
/// Returns a list of unique devices and groups, with speaker groups first,
/// each sorted alphabetically by name.
pub async fn discover_devices() -> Result<Vec<ChromecastDevice>, Box<dyn Error>> {
    // Do a single discovery scan
    let stream = mdns::discover::all(SERVICE_NAME, Duration::from_secs(5))?.listen();
    pin_mut!(stream);

    // Collect all unique devices with their names and records
    let mut devices = HashMap::new();
    let mut seen_devices = HashMap::new();
    
    while let Some(response) = stream.next().await {
        match response {
            Ok(response) => {
                let mut name = "Unknown Device".to_string();
                let mut model = String::new();
                let mut records = Vec::new();
                
                // Get all records
                for record in response.records() {
                    match &record.kind {
                        RecordKind::A(addr) => records.push(format!("A: {}", addr)),
                        RecordKind::AAAA(addr) => records.push(format!("AAAA: {}", addr)),
                        RecordKind::TXT(txt) => {
                            for entry in txt {
                                if entry.starts_with("fn=") {
                                    name = entry[3..].to_string();
                                } else if entry.starts_with("md=") {
                                    model = entry[3..].to_string();
                                }
                                records.push(format!("TXT: {}", entry));
                            }
                        }
                        RecordKind::SRV { priority, weight, port, target } => {
                            records.push(format!("SRV: priority={}, weight={}, port={}, target={}", 
                                priority, weight, port, target));
                        }
                        RecordKind::PTR(ptr) => records.push(format!("PTR: {}", ptr)),
                        _ => records.push(format!("Other: {:?}", record.kind)),
                    }
                }
                
                if let Some(addr) = response.records()
                                          .filter_map(self::to_ip_addr)
                                          .next() {
                    let is_group = model.contains("Google Cast Group");
                    let key = (addr, name.clone());
                    
                    // Check if we've seen this device before
                    if seen_devices.contains_key(&key) {
                        // We've seen a repeat, exit the discovery
                        break;
                    }
                    
                    // Mark this device as seen
                    seen_devices.insert(key.clone(), true);
                    
                    // Add to our devices list
                    devices.insert(key, ChromecastDevice {
                        name,
                        ip: addr,
                        is_group,
                        records,
                    });
                }
            }
            Err(e) => println!("Error receiving response: {}", e),
        }
    }

    // Split into groups and devices, sort each, then recombine
    let all_devices: Vec<_> = devices.into_values().collect();
    let mut groups: Vec<_> = all_devices.iter()
        .filter(|d| d.is_group)
        .cloned()
        .collect();
    let mut devices: Vec<_> = all_devices.iter()
        .filter(|d| !d.is_group)
        .cloned()
        .collect();

    // Sort each list by name
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    devices.sort_by(|a, b| a.name.cmp(&b.name));

    // Combine with groups first
    let mut result = Vec::new();
    result.extend(groups);
    result.extend(devices);

    Ok(result)
}

pub fn build_input_stream_and_sender(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    num_channels: usize,
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let input_stream = device.build_input_stream(
        &config.config(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut bytes = Vec::with_capacity(data.len() * 2);
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
    Ok(input_stream)
}

pub fn spawn_audio_http_server(
    audio_buffer: Arc<Mutex<Vec<u8>>>,
    sample_rate: u32,
    port: u16,
) -> std::net::SocketAddr {
    use warp::Filter;
    use async_stream;
    use bytes::Bytes;
    use std::convert::Infallible;
    use local_ip_address::local_ip;
    use std::net::SocketAddr;

    let audio_buffer_http = audio_buffer.clone();
    let local_ip = local_ip().expect("Failed to get local IP address");
    let server_addr = SocketAddr::new(local_ip, port);

    let audio_route = warp::path("audio")
        .map(move || {
            let audio_buffer = audio_buffer_http.clone();
            let stream = async_stream::stream! {
                // Create WAV header
                let header = crate::create_wav_header(
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
    server_addr
}

pub async fn connect_and_stream_to_chromecast(
    selected_cast_device: &ChromecastDevice,
    local_ip: IpAddr,
    http_port: u16,
    verbose_mode: bool,
) -> Result<(), Box<dyn Error>> {
    // Connect to the Chromecast device
    let receiver = Receiver::new();
    receiver.connect(&selected_cast_device.ip.to_string()).await?;

    if verbose_mode {
        println!("Connected");
    }
    
    // Choose the correct app ID based on whether it's a group
    let app_id = if selected_cast_device.is_group {
        AppId::Custom("2872939A".to_string())
    } else {
        AppId::DefaultMediaReceiver
    };
    let app = receiver.launch_app(app_id).await?;

    if verbose_mode {
        println!("Launched {} app", if selected_cast_device.is_group { "Cast Audio Receiver" } else { "Default Media Receiver" });
    }
    
    let media_controller = MediaController::new(app.clone(), receiver.clone())?;

    if verbose_mode {
        println!("Created media controller");
    }
    
    // Create media data packet
    let media_info = MediaInformationBuilder::default()
        .content_id(&format!("http://{}:{}/audio", local_ip, http_port))
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
    if verbose_mode {
        println!("Sending media info to Chromecast...");
        println!("Stream URL: http://{}:{}/audio", local_ip, http_port);
    }
    
    match media_controller.load(media_info).await {
        Ok(_) => (),
        Err(e) => eprintln!("Error sending media data: {}", e),
    }

    println!("Use the Home app on your phone for volume control");
    println!("Press Ctrl-C to stop casting");
    
    Ok(())
}
