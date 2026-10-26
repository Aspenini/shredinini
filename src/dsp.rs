use crate::model::ToneParameters;
use biquad::{Biquad, Coefficients, DirectForm2Transposed, Q_BUTTERWORTH_F32, ToHertz, Type};

pub fn db_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

// Discard inaudible feedback to avoid subnormal arithmetic after long decays.
fn flush_tiny(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

struct DelayLine {
    samples: Vec<f32>,
    position: usize,
    low: f32,
}
impl DelayLine {
    fn new(length: usize) -> Self {
        Self {
            samples: vec![0.0; length.max(1)],
            position: 0,
            low: 0.0,
        }
    }
    fn comb(&mut self, input: f32, feedback: f32, damping: f32) -> f32 {
        let out = self.samples[self.position];
        self.low = flush_tiny(self.low + damping * (out - self.low));
        self.samples[self.position] = flush_tiny(input + self.low * feedback);
        self.position = (self.position + 1) % self.samples.len();
        out
    }
    fn allpass(&mut self, input: f32) -> f32 {
        let stored = self.samples[self.position];
        let out = stored - input;
        self.samples[self.position] = flush_tiny(input + stored * 0.5);
        self.position = (self.position + 1) % self.samples.len();
        out
    }
}

struct Reverb {
    combs: [DelayLine; 4],
    diffusion: [DelayLine; 2],
}
impl Reverb {
    fn new(rate: f32, spread: f32) -> Self {
        Self {
            combs: [0.0297, 0.0371, 0.0411, 0.0437]
                .map(|s| DelayLine::new(((s + spread) * rate) as usize)),
            diffusion: [0.005, 0.0017]
                .map(|s| DelayLine::new(((s + spread * 0.3) * rate) as usize)),
        }
    }
    fn tick(&mut self, input: f32) -> f32 {
        let mut x = 0.0;
        for line in &mut self.combs {
            x += line.comb(input * 0.18, 0.77, 0.3);
        }
        for line in &mut self.diffusion {
            x = line.allpass(x);
        }
        x * 0.5
    }
}

pub struct Processor {
    high_pass: DirectForm2Transposed<f32>,
    crossover: f32,
    cross_alpha: f32,
    envelope: f32,
    attack: f32,
    release: f32,
    smooth: f32,
    current: [f32; 8],
    target: [f32; 8],
    delays: [DelayLine; 2],
    reverbs: [Reverb; 2],
}
impl Processor {
    pub fn new(rate: u32, p: ToneParameters) -> Self {
        let rate = rate as f32;
        let coefficients = Coefficients::<f32>::from_params(
            Type::HighPass,
            rate.hz(),
            65.0.hz(),
            Q_BUTTERWORTH_F32,
        )
        .expect("valid audio sample rate");
        let mut result = Self {
            high_pass: DirectForm2Transposed::new(coefficients),
            crossover: 0.0,
            cross_alpha: 1.0 - (-std::f32::consts::TAU * 1100.0 / rate).exp(),
            envelope: 0.0,
            attack: (-1.0 / (0.005 * rate)).exp(),
            release: (-1.0 / (0.120 * rate)).exp(),
            smooth: 1.0 - (-1.0 / (0.020 * rate)).exp(),
            current: [0.0; 8],
            target: [0.0; 8],
            delays: [
                DelayLine::new((rate * 0.290) as usize),
                DelayLine::new((rate * 0.435) as usize),
            ],
            reverbs: [Reverb::new(rate, 0.0), Reverb::new(rate, 0.0013)],
        };
        result.set_parameters(p);
        result.current = result.target;
        result
    }
    pub fn use_full_instrument_range(&mut self, rate: u32) {
        self.high_pass = DirectForm2Transposed::new(
            Coefficients::<f32>::from_params(
                Type::HighPass,
                (rate as f32).hz(),
                20.0.hz(),
                Q_BUTTERWORTH_F32,
            )
            .expect("valid instrument sample rate"),
        );
    }
    pub fn set_parameters(&mut self, mut p: ToneParameters) {
        p.sanitize();
        let tilt = (p.tone - 0.5) * 12.0;
        self.target = [
            db_gain(tilt),
            db_gain(-tilt),
            p.warmth,
            p.delay,
            p.reverb,
            if p.compression { 1.0 } else { 0.0 },
            db_gain(p.input_trim_db),
            db_gain(p.output_db),
        ];
    }
    pub fn tick(&mut self, input: f32, bypass: f32) -> [f32; 2] {
        for i in 0..8 {
            self.current[i] += self.smooth * (self.target[i] - self.current[i]);
        }
        let c = self.current;
        let dry = if input.is_finite() {
            input.clamp(-8.0, 8.0) * c[6]
        } else {
            0.0
        };
        let hp = self.high_pass.run(dry);
        if dry.abs() < 1e-20 && hp.abs() < 1e-20 {
            self.high_pass.reset_state();
        }
        self.crossover = flush_tiny(self.crossover + self.cross_alpha * (hp - self.crossover));
        let tone = self.crossover * c[1] + (hp - self.crossover) * c[0];
        let abs = tone.abs();
        let envelope_coeff = if abs > self.envelope {
            self.attack
        } else {
            self.release
        };
        self.envelope = flush_tiny(envelope_coeff * self.envelope + (1.0 - envelope_coeff) * abs);
        let gain = if self.envelope > 0.18 {
            (0.18 / self.envelope).powf(2.0 / 3.0)
        } else {
            1.0
        };
        let compressed = tone * (1.0 + c[5] * (gain * 1.15 - 1.0));
        let drive = 1.0 + c[2] * 5.0;
        let saturated = (compressed * drive).tanh() / drive;
        let x = compressed + (saturated - compressed) * c[2];
        let mut result = [0.0; 2];
        for (i, out) in result.iter_mut().enumerate() {
            let echo = self.delays[i].comb(x, 0.30, 0.38);
            let space = self.reverbs[i].tick(x + echo * c[3] * 0.4);
            let wet = x + echo * c[3] * 0.65 + space * c[4] * 1.4;
            let mixed = (wet + (dry - wet) * bypass) * c[7];
            *out = peak_protect(mixed);
        }
        result
    }
}

pub fn peak_protect(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let a = x.abs();
    if a <= 0.90 {
        x
    } else {
        x.signum() * (0.90 + 0.099 * ((a - 0.90) / 0.099).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_and_extremes_stay_finite_and_bounded() {
        for rate in [44100, 48000, 96000] {
            let mut p = Processor::new(rate, ToneParameters::default());
            for _ in 0..1000 {
                assert_eq!(p.tick(0.0, 0.0), [0.0, 0.0]);
            }
            p.set_parameters(ToneParameters {
                tone: 1.0,
                warmth: 1.0,
                reverb: 1.0,
                delay: 1.0,
                input_trim_db: 24.0,
                output_db: 0.0,
                ..Default::default()
            });
            for i in 0..rate * 2 {
                let input = if i % 997 == 0 {
                    f32::NAN
                } else {
                    ((i as f32) * 0.13).sin() * 4.0
                };
                for out in p.tick(input, 0.0) {
                    assert!(out.is_finite() && out.abs() < 1.0);
                }
            }
        }
    }
    #[test]
    fn impulse_has_stereo_tails_and_decays() {
        let mut p = Processor::new(
            44100,
            ToneParameters {
                delay: 1.0,
                reverb: 1.0,
                ..Default::default()
            },
        );
        p.tick(0.5, 0.0);
        let mut tail = 0.0;
        let mut stereo = 0.0;
        let mut end = 0.0;
        for i in 0..44100 * 8 {
            let [l, r] = p.tick(0.0, 0.0);
            if i > 4410 {
                tail += l.abs() + r.abs();
                stereo += (l - r).abs();
            }
            if i > 44100 * 7 {
                end += l.abs() + r.abs();
            }
        }
        assert!(tail > 0.01 && stereo > 0.01);
        assert!(end < tail * 0.01);
    }
    #[test]
    fn bypass_is_dry_with_master_and_parameter_steps_are_smoothed() {
        let mut p = Processor::new(44100, ToneParameters::default());
        let [l, r] = p.tick(0.2, 1.0);
        assert!((l - 0.2 * db_gain(-12.0)).abs() < 1e-6);
        assert_eq!(l, r);
        p.set_parameters(ToneParameters {
            output_db: 0.0,
            ..Default::default()
        });
        let next = p.tick(0.2, 1.0)[0];
        assert!((next - l).abs() < 0.001);
    }
}
