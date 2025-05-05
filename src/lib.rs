use std::error::Error;
use std::fs::File;
use std::io::{BufWriter, Write};

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