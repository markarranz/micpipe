/// Streaming linear resampler. Operatres on whole frames (channels samples each).
pub struct Resampler {
    ratio: f32,
    position: f32,
    channels: usize,
    prev: Vec<f32>,
    next: Vec<f32>,
    initialized: bool,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32, channels: usize) -> Self {
        Resampler {
            ratio: in_rate as f32 / out_rate as f32,
            position: 0.0,
            channels,
            prev: vec![0.0; channels],
            next: vec![0.0; channels],
            initialized: false,
        }
    }

    pub fn process(&mut self, input_frame: &[f32], out: &mut Vec<f32>) {
        if !self.initialized {
            self.prev.copy_from_slice(input_frame);
            self.initialized = true;
            return;
        }

        self.next.copy_from_slice(input_frame);

        while self.position < 1.0 {
            let frac = self.position;
            for c in 0..self.channels {
                let sample = self.prev[c] * (1.0 - frac) + self.next[c] * frac;
                out.push(sample);
            }
            self.position += self.ratio;
        }
        self.position -= 1.0;
        self.prev.copy_from_slice(&self.next);
    }
}
