use hound::{SampleFormat, WavSpec, WavWriter};

fn write_tone(path: &str, freq_left: f32, freq_right: f32) {
    let spec = WavSpec {
        channels: 2,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let sample_rate = 48000.0_f32;
    let total_frames = (sample_rate * 3.0) as usize;

    for i in 0..total_frames {
        let t = i as f32 / sample_rate;
        let left = (t * freq_left * 2.0 * std::f32::consts::PI).sin() * 0.3;
        let right = (t * freq_right * 2.0 * std::f32::consts::PI).sin() * 0.3;
        writer.write_sample(left).unwrap();
        writer.write_sample(right).unwrap();
    }
    writer.finalize().unwrap();
    println!("Wrote {}", path);
}

fn write_tone_mono(path: &str, freq: f32, sample_rate: u32) {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec).unwrap();
    let sr = sample_rate as f32;
    let total_frames = (sr * 3.0) as usize;

    for i in 0..total_frames {
        let t = i as f32 / sr;
        let s = (t * freq * 2.0 * std::f32::consts::PI).sin() * 0.3;
        writer.write_sample(s).unwrap();
    }
    writer.finalize().unwrap();
    println!("Wrote {} at {} Hz", path, sample_rate);
}

fn main() {
    // A4, both channels
    write_tone("a.wav", 440.0, 440.0);
    // C#4, single channel, 44.1k
    write_tone_mono("b.mono.441k.wav", 277.18, 44_100);
}
