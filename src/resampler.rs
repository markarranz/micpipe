/// Streaming linear resampler. Operates on whole frames (channels samples each).
#[derive(Debug)]
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
        Self {
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
        debug_assert_eq!(input_frame.len(), self.channels);

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

#[cfg(test)]
mod tests {
    use super::Resampler;

    #[test]
    fn first_frame_primes_without_output() {
        let mut resampler = Resampler::new(48_000, 48_000, 1);
        let mut output = Vec::new();

        resampler.process(&[0.25], &mut output);

        assert!(output.is_empty());
    }

    #[test]
    fn matching_rates_emit_next_frame() {
        let mut resampler = Resampler::new(48_000, 48_000, 1);
        let mut output = Vec::new();

        resampler.process(&[0.0], &mut output);
        resampler.process(&[1.0], &mut output);

        assert_eq!(output, vec![0.0]);
    }

    #[test]
    fn upsampling_emits_interpolated_frames() {
        let mut resampler = Resampler::new(24_000, 48_000, 1);
        let mut output = Vec::new();

        resampler.process(&[0.0], &mut output);
        resampler.process(&[1.0], &mut output);

        assert_eq!(output, vec![0.0, 0.5]);
    }
}
