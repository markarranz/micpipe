use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no default output device");

    println!("Default output device: {}", device.description().unwrap());

    println!("\nAll output devices:");
    for device in host.output_devices().unwrap() {
        let desc = device.description().unwrap();
        println!(" {}", desc);
        match device.supported_output_configs() {
            Ok(configs) => {
                for config in configs {
                    println!(
                        "   channels: {}, sample rate: {:?}-{:?}, format: {:?}, buffer: {:?}",
                        config.channels(),
                        config.min_sample_rate(),
                        config.max_sample_rate(),
                        config.sample_format(),
                        config.buffer_size(),
                    );
                }
            }
            Err(e) => println!("   Error getting configs: {}", e),
        }
    }
}
