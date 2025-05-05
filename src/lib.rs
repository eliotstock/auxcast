use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};
use futures_util::{pin_mut, stream::StreamExt};
use mdns::{Record, RecordKind};
use std::{net::IpAddr, time::Duration, collections::HashMap};

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

/// Discovers Chromecast devices and speaker groups on the network.
/// Returns a list of unique devices and groups.
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

    Ok(devices.into_values().collect())
}
