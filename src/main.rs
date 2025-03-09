use cast_sender;

use macro_rules_attribute::apply;
use smol_macros::main;
use reqwest;
use local_ip_address::local_ip;

use cast_sender::namespace::media::{
    MediaInformationBuilder, MusicTrackMediaMetadataBuilder, StreamType,
};
use cast_sender::{AppId, Error, ImageBuilder, MediaController, Receiver};

use anyhow;
use cpal;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::net::TcpListener;
use std::io::Write;

const CHROMECAST_IP_A: &str = "192.168.86.247"; // works
const CHROMECAST_IP_B: &str = "192.168.86.29"; // works
const CHROMECAST_IP_C: &str = "192.168.86.238"; // works

const CHROMECAST_IP: &str = CHROMECAST_IP_C;

const STREAM_IP: &str = "127.0.0.1";
const STREAM_PORT: &str = "8080";
// #[apply(main!)]
// async fn main() -> Result<(), Error> {
//     enumerate()
// }

async fn cast() -> Result<(), Error> {
    let receiver = Receiver::new();
    receiver.connect(CHROMECAST_IP).await?;

    let app = receiver.launch_app(AppId::DefaultMediaReceiver).await?;
    let media_controller = MediaController::new(app.clone(), receiver.clone())?;

    let metadata = MusicTrackMediaMetadataBuilder::default()
        .title("BBC World Service")
        .images(vec![ImageBuilder::default()
            .url("https://sounds.files.bbci.co.uk/3.7.0/networks/bbc_world_service/colour_default.svg")
            .build()
            .unwrap()])
        .build()
        .unwrap();

    let media_info = MediaInformationBuilder::default()
        .content_id("http://stream.live.vc.bbcmedia.co.uk/bbc_world_service")
        .stream_type(StreamType::Live)
        .content_type("audio/*")
        .metadata(metadata)
        .build()
        .unwrap();

    let media_info_local_file = MediaInformationBuilder::default()
        .content_id("file:///Users/e/Downloads/test.mp3")
        .stream_type(StreamType::Live)
        .content_type("audio/*")
        .build()
        .unwrap();

    media_controller.load(media_info_local_file).await?;
    // media_controller.start().await?;

    // media_controller.stop().await?;
    Ok(())
}

async fn get_external_ip() -> Result<String, reqwest::Error> {
    let response = reqwest::get("https://api.ipify.org").await?;
    response.text().await
}

fn main() -> Result<(), anyhow::Error> {
    // Get local IP first
    let ip = local_ip().unwrap();
    let stream_address = format!("{}:{}", STREAM_IP, STREAM_PORT);
    let listener = TcpListener::bind(&stream_address)?;
    println!("Server listening on {}:{}", STREAM_IP, STREAM_PORT);

    let available_hosts = cpal::available_hosts();
    println!("Available hosts:\n  {:?}", available_hosts);

    for host_id in available_hosts {
        println!("{}", host_id.name());
        let host = cpal::host_from_id(host_id)?;

        let default_in = host.default_input_device().map(|e| e.name().unwrap());
        let default_out = host.default_output_device().map(|e| e.name().unwrap());
        println!("  Default Input Device:\n    {:?}", default_in);
        println!("  Default Output Device:\n    {:?}", default_out);

        let devices = host.devices()?;
        println!("  Devices: ");
        for (device_index, device) in devices.enumerate() {
            println!("  {}. \"{}\"", device_index + 1, device.name()?);

            let mut supported_configs_range = device.supported_output_configs()
                .expect("error while querying configs");

            // This should be this one on my machine:
            // SupportedStreamConfigRange { channels: 20, min_sample_rate: SampleRate(44100), max_sample_rate: SampleRate(44100), buffer_size: Range { min: 14, max: 4096 }, sample_format: F32 }
            let supported_config = supported_configs_range.next()
                .expect("no supported config?!")
                .with_max_sample_rate();
            println!("  Stream config: {:?}. ", supported_config);

            let stream = device.build_output_stream(
                &supported_config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // react to stream events and read or write stream data here.
                },
                move |err| {
                    // react to errors here.
                },
                None // None=blocking, Some(Duration)=timeout
            )?;
            // println!("  Stream: {:?}. ", stream.);

            stream.play()?;

            for stream in listener.incoming() {
                println!("Incoming connection");
                
                let mut stream = stream?;
                let response = "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\n\r\n";
                stream.write_all(response.as_bytes())?;

                // Send audio data to the client
                // This is a simplified example and doesn't include proper WAV headers
                loop {
                    let buffer = vec![0.0f32; 1024];
                    // Fill buffer with audio data from the input stream
                    stream.write_all(bytemuck::cast_slice(&buffer))?;
                }
            }

            // // Input configs
            // if let Ok(conf) = device.default_input_config() {
            //     println!("    Default input stream config:\n      {:?}", conf);
            // }
            // let input_configs = match device.supported_input_configs() {
            //     Ok(f) => f.collect(),
            //     Err(e) => {
            //         println!("    Error getting supported input configs: {:?}", e);
            //         Vec::new()
            //     }
            // };
            // if !input_configs.is_empty() {
            //     println!("    All supported input stream configs:");
            //     for (config_index, config) in input_configs.into_iter().enumerate() {
            //         println!(
            //             "      {}.{}. {:?}",
            //             device_index + 1,
            //             config_index + 1,
            //             config
            //         );

            //         if device_index == 0 && config_index == 0 {
                
            //             // stream.play()?;
            //         }
            //     }
            // }

            // // Output configs
            // if let Ok(conf) = device.default_output_config() {
            //     println!("    Default output stream config:\n      {:?}", conf);
            // }
            // let output_configs = match device.supported_output_configs() {
            //     Ok(f) => f.collect(),
            //     Err(e) => {
            //         println!("    Error getting supported output configs: {:?}", e);
            //         Vec::new()
            //     }
            // };
            // if !output_configs.is_empty() {
            //     println!("    All supported output stream configs:");
            //     for (config_index, config) in output_configs.into_iter().enumerate() {
            //         println!(
            //             "      {}.{}. {:?}",
            //             device_index + 1,
            //             config_index + 1,
            //             config
            //         );
            //     }
            // }
        }
    }

    Ok(())
}