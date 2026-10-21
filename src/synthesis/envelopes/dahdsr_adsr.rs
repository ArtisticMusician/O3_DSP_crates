/// Below this level the envelope is treated as silent: -80 dBFS, inaudible against any programme.
const RELEASE_FLOOR: f32 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// An audio-thread-safe ADSR envelope utilizing natural exponential curves
/// and dynamic, automation-resilient sample countdown tracking.
#[derive(Debug, Clone, Copy)]
pub struct AdsrEnvelope {
    stage: AdsrStage,
    current_level: f32,

    // Cached curve multipliers
    attack_coef: f32,
    decay_coef: f32,
    release_coef: f32,
    sustain_level: f32,

    // Cached target phase durations in total samples
    attack_samples: u32,
    decay_samples: u32,
    release_samples: u32,

    // Active step counter
    stage_samples: u32,
    sample_rate: f32,
}

impl AdsrEnvelope {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            stage: AdsrStage::Idle,
            current_level: 0.0,
            attack_coef: 0.0,
            decay_coef: 0.0,
            release_coef: 0.0,
            sustain_level: 0.7,
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            stage_samples: 0,
            sample_rate,
        }
    }

    /// Updates all internal tracking structures simultaneously.
    /// 100% real-time safe against dynamic parameter automation shifts mid-phase.
    pub fn set_parameters(
        &mut self,
        attack_sec: f32,
        decay_sec: f32,
        sustain_lvl: f32,
        release_sec: f32,
    ) {
        self.sustain_level = sustain_lvl.clamp(0.0, 1.0);

        // Pre-calculate natural capacitor exponential mapping coefficients
        self.attack_coef = if attack_sec > 0.0 {
            1.0 - (-1.0 / (attack_sec * self.sample_rate)).exp()
        } else {
            1.0
        };
        self.decay_coef = if decay_sec > 0.0 {
            1.0 - (-1.0 / (decay_sec * self.sample_rate)).exp()
        } else {
            1.0
        };
        // The release has to reach silence *within* its own duration, not one time constant into
        // it: stopping at 1 tau leaves the level around 26% of where it started, and dropping from
        // there to zero is an audible click. Pick the coefficient that lands on RELEASE_FLOOR after
        // exactly `release_sec` worth of samples.
        self.release_coef = if release_sec > 0.0 {
            let samples = (release_sec * self.sample_rate).max(1.0);
            1.0 - (RELEASE_FLOOR as f64).powf(1.0 / samples as f64) as f32
        } else {
            1.0
        };

        // Calculate new total target sample durations
        let new_attack_samples = (attack_sec * self.sample_rate).max(1.0) as u32;
        let new_decay_samples = (decay_sec * self.sample_rate).max(1.0) as u32;
        let new_release_samples = (release_sec * self.sample_rate).max(1.0) as u32;

        // Dynamic Active Phase Correction: If parameters change while a phase is running,
        // adjust the active sample countdown relative to the new duration limit.
        match self.stage {
            AdsrStage::Attack => {
                if new_attack_samples > self.attack_samples {
                    let delta = new_attack_samples - self.attack_samples;
                    self.stage_samples += delta;
                } else {
                    let delta = self.attack_samples - new_attack_samples;
                    self.stage_samples = self.stage_samples.saturating_sub(delta).max(1);
                }
            }
            AdsrStage::Decay => {
                if new_decay_samples > self.decay_samples {
                    let delta = new_decay_samples - self.decay_samples;
                    self.stage_samples += delta;
                } else {
                    let delta = self.decay_samples - new_decay_samples;
                    self.stage_samples = self.stage_samples.saturating_sub(delta).max(1);
                }
            }
            AdsrStage::Release => {
                if new_release_samples > self.release_samples {
                    let delta = new_release_samples - self.release_samples;
                    self.stage_samples += delta;
                } else {
                    let delta = self.release_samples - new_release_samples;
                    self.stage_samples = self.stage_samples.saturating_sub(delta).max(1);
                }
            }
            _ => {}
        }

        // Lock down the updated configuration state
        self.attack_samples = new_attack_samples;
        self.decay_samples = new_decay_samples;
        self.release_samples = new_release_samples;
    }

    /// Triggers gate note on. Parameter-free and real-time safe.
    pub fn gate_on(&mut self) {
        self.stage = AdsrStage::Attack;
        self.stage_samples = self.attack_samples;
    }

    /// Triggers gate note off. Parameter-free and real-time safe.
    pub fn gate_off(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.stage = AdsrStage::Release;
            self.stage_samples = self.release_samples;
        }
    }

    /// Retrieve the scaling coefficient multiplier. Real-time safe.
    #[inline(always)]
    pub fn next_coefficient(&mut self) -> f32 {
        match self.stage {
            AdsrStage::Idle => {
                self.current_level = 0.0;
            }
            AdsrStage::Attack => {
                self.current_level += (1.005 - self.current_level) * self.attack_coef;
                self.stage_samples = self.stage_samples.saturating_sub(1);

                if self.stage_samples == 0 || self.current_level >= 1.0 {
                    self.current_level = 1.0;
                    self.stage = AdsrStage::Decay;
                    self.stage_samples = self.decay_samples;
                }
            }
            AdsrStage::Decay => {
                self.current_level += (self.sustain_level - self.current_level) * self.decay_coef;
                self.stage_samples = self.stage_samples.saturating_sub(1);

                if self.stage_samples == 0 {
                    self.current_level = self.sustain_level;
                    self.stage = AdsrStage::Sustain;
                }
            }
            AdsrStage::Sustain => {
                // Smoothly tracks changes to sustain level parameter during sustain state
                self.current_level = self.sustain_level;
            }
            AdsrStage::Release => {
                self.current_level += (0.0 - self.current_level) * self.release_coef;
                self.stage_samples = self.stage_samples.saturating_sub(1);

                // The counter is a bound, not the ending: only go idle once the level is actually
                // inaudible, otherwise the last sample is a step from a loud level to zero.
                if self.current_level <= RELEASE_FLOOR || self.stage_samples == 0 {
                    self.current_level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
            }
        }
        self.current_level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    /// Run a note into sustain, release it, and report the level of the last sample that was not
    /// yet silent, plus how long the release took in seconds.
    fn release_tail(release_sec: f32) -> (f32, f32) {
        let mut envelope = AdsrEnvelope::new(SAMPLE_RATE);
        envelope.set_parameters(0.001, 0.05, 1.0, release_sec);

        envelope.gate_on();
        for _ in 0..4_800 {
            envelope.next_coefficient();
        }

        envelope.gate_off();
        let mut last_audible = 1.0;
        let mut samples = 0;
        loop {
            let level = envelope.next_coefficient();
            samples += 1;
            if level <= 0.0 {
                break;
            }
            last_audible = level;
            assert!(samples < (SAMPLE_RATE * 30.0) as usize, "the release never ended");
        }

        (last_audible, samples as f32 / SAMPLE_RATE)
    }

    #[test]
    fn the_release_lands_on_silence_instead_of_cutting() {
        // Stopping one time constant into the release leaves roughly a quarter of the level in
        // place, and stepping from there to zero clicks. The last audible sample must be quiet.
        let (last_audible, _) = release_tail(0.4);
        let decibels = 20.0 * last_audible.log10();

        assert!(
            last_audible < 1e-3,
            "release cut from {last_audible} ({decibels:.1} dBFS)"
        );
    }

    #[test]
    fn a_longer_release_takes_longer() {
        let (_, short) = release_tail(0.1);
        let (_, long) = release_tail(1.0);

        assert!(
            long > short * 3.0,
            "0.1 s took {short} s, 1.0 s took {long} s"
        );
    }

    #[test]
    fn a_release_reaches_silence_within_its_own_duration() {
        // The parameter promises a length; the fade must not drag on for several seconds.
        let (_, seconds) = release_tail(0.4);
        assert!(
            seconds > 0.1 && seconds < 0.8,
            "a 0.4 s release took {seconds} s"
        );
    }

    #[test]
    fn the_sustain_level_is_held_indefinitely() {
        let mut envelope = AdsrEnvelope::new(SAMPLE_RATE);
        envelope.set_parameters(0.001, 0.05, 0.5, 0.4);
        envelope.gate_on();

        let mut level = 0.0;
        for _ in 0..(SAMPLE_RATE as usize * 2) {
            level = envelope.next_coefficient();
        }

        assert!((level - 0.5).abs() < 0.01, "sustain settled at {level}");
    }
}
