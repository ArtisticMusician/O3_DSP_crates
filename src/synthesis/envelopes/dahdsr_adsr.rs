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
        self.release_coef = if release_sec > 0.0 {
            1.0 - (-1.0 / (release_sec * self.sample_rate)).exp()
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
                self.current_level += (-0.005 - self.current_level) * self.release_coef;
                self.stage_samples = self.stage_samples.saturating_sub(1);

                if self.stage_samples == 0 || self.current_level <= 0.0 {
                    self.current_level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
            }
        }
        self.current_level
    }
}
