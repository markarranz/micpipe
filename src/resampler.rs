/// Streaming linear resampler. Operatres on whole frames (channels samples each).
pub struct Resampler {
    ratio: f32,        // in_rate / out_rate; how far position advances per output frame
    position: f32,     // fractional position in [0, 1) between `prev` and `next`
    channels: usize,   // number of audio channels
    prev: Vec<f32>,    // the "left" frame of the current interval
    next: Vec<f32>,    // the "right" frame
    initialized: bool, // have we loaded the first prev frame yet?
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

    /// Feed one input frame; appends zero or more output frames to `out`.
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
