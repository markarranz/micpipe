use hound::{WavSpec, WavWriter, SampleFormat};

fn main() {
    let spec = WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };

    let mut writer = WavWriter::create("test.wav", spec).unwrap();

    let sample_rate = 48000.0_f32;
    let duration_secs = 3.0;
    let total_frames = (sample_rate * duration_secs) as usize;

    // Left channel: 440 Hz. Right channel: 660 Hz. So you can hear stereo separation.
    let freq_left = 440.0_f32;
    let freq_right = 660.0_f32;

    for i in 0..total_frames {
        let t = i as f32 / sample_rate;
        let left = (t * freq_left * 2.0 * std::f32::consts::PI).sin() * 0.3;
        let right = (t * freq_right * 2.0 * std::f32::consts::PI).sin() * 0.3;

        // Interleaved: write left then right
        writer.write_sample(left).unwrap();
        writer.write_sample(right).unwrap();
    }

    writer.finalize().unwrap();
    println!("Wrote test.wav: {} frames, {} seconds", total_frames, duration_secs);
}
