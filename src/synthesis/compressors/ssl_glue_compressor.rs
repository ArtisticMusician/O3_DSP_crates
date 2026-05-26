#[derive(Clone, Debug)]
pub struct SslGlueCompressor {
    sample_rate: f32,
    
    threshold_db: f32,
    ratio_slope: f32,
    makeup_gain_db: f32,
    knee_half_db: f32,
    knee_inv: f32,
    auto_release: bool,

    attack_coef: f32,
    release_coef: f32,
    envelope_db: f32,
    
    auto_fast_attack_coef: f32,
    auto_fast_release_coef: f32,
    auto_slow_attack_coef: f32,
    auto_slow_release_coef: f32,
    
    auto_fast_env_db: f32,
    auto_slow_env_db: f32,
}

impl SslGlueCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut comp = Self {
            sample_rate,
            threshold_db: 0.0,
            ratio_slope: 0.75,
            makeup_gain_db: 0.0,
            knee_half_db: 2.0,
            knee_inv: 0.125,
            auto_release: false,
            
            attack_coef: 0.0,
            release_coef: 0.0,
            envelope_db: 0.0,
            
            auto_fast_attack_coef: 0.0,
            auto_fast_release_coef: 0.0,
            auto_slow_attack_coef: 0.0,
            auto_slow_release_coef: 0.0,
            
            auto_fast_env_db: 0.0,
            auto_slow_env_db: 0.0,
        };
        
        comp.set_attack_ms(1.0);
        comp.set_release_ms(None); 
        comp
    }

    pub fn set_threshold(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db;
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        let ratio = ratio.max(1.0);
        self.ratio_slope = 1.0 - (1.0 / ratio);
    }

    pub fn set_makeup_gain(&mut self, gain_db: f32) {
        self.makeup_gain_db = gain_db;
    }

    pub fn set_knee_width(&mut self, knee_db: f32) {
        let knee_width_db = knee_db.max(0.0);
        if knee_width_db < 1e-5 {
            self.knee_half_db = 0.0;
            self.knee_inv = 0.0;
        } else {
            self.knee_half_db = knee_width_db * 0.5;
            self.knee_inv = 1.0 / (2.0 * knee_width_db);
        }
    }

    pub fn set_attack_ms(&mut self, ms: f32) {
        self.attack_coef = self.calculate_coef(ms);
        self.auto_fast_attack_coef = self.attack_coef;
    }

    pub fn set_release_ms(&mut self, ms: Option<f32>) {
        if let Some(time) = ms {
            self.auto_release = false;
            self.release_coef = self.calculate_coef(time);
        } else {
            self.auto_release = true;
            self.auto_fast_release_coef = self.calculate_coef(200.0);
            self.auto_slow_release_coef = self.calculate_coef(2500.0);
            self.auto_slow_attack_coef = self.calculate_coef(100.0); 
        }
    }

    fn calculate_coef(&self, time_ms: f32) -> f32 {
        let time_sec = (time_ms * 0.001).max(0.0001);
        (-1.0 / (time_sec * self.sample_rate)).exp()
    }

    #[inline(always)]
    fn lin2db(lin: f32) -> f32 {
        lin.max(1e-12).ln() * 8.6858896
    }

    #[inline(always)]
    fn db2lin(db: f32) -> f32 {
        (db * 0.11512925).exp()
    }

    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let sidechain_lin = left.abs().max(right.abs());
        let sidechain_db = Self::lin2db(sidechain_lin);

        let over_threshold = sidechain_db - self.threshold_db;
        
        let target_gr_db = if self.knee_half_db > 0.0 {
            if over_threshold > self.knee_half_db {
                over_threshold * self.ratio_slope
            } else if over_threshold > -self.knee_half_db {
                let x = over_threshold + self.knee_half_db;
                self.ratio_slope * (x * x) * self.knee_inv
            } else {
                0.0
            }
        } else {
            if over_threshold > 0.0 {
                over_threshold * self.ratio_slope
            } else {
                0.0
            }
        };

        if self.auto_release {
            if target_gr_db > self.auto_fast_env_db {
                self.auto_fast_env_db = self.auto_fast_attack_coef * self.auto_fast_env_db + (1.0 - self.auto_fast_attack_coef) * target_gr_db;
            } else {
                self.auto_fast_env_db = self.auto_fast_release_coef * self.auto_fast_env_db + (1.0 - self.auto_fast_release_coef) * target_gr_db;
            }

            if target_gr_db > self.auto_slow_env_db {
                self.auto_slow_env_db = self.auto_slow_attack_coef * self.auto_slow_env_db + (1.0 - self.auto_slow_attack_coef) * target_gr_db;
            } else {
                self.auto_slow_env_db = self.auto_slow_release_coef * self.auto_slow_env_db + (1.0 - self.auto_slow_release_coef) * target_gr_db;
            }

            self.envelope_db = self.auto_fast_env_db.max(self.auto_slow_env_db);
        } else {
            if target_gr_db > self.envelope_db {
                self.envelope_db = self.attack_coef * self.envelope_db + (1.0 - self.attack_coef) * target_gr_db;
            } else {
                self.envelope_db = self.release_coef * self.envelope_db + (1.0 - self.release_coef) * target_gr_db;
            }
        }

        let final_gain_db = -self.envelope_db + self.makeup_gain_db;
        let final_gain_lin = Self::db2lin(final_gain_db);

        (left * final_gain_lin, right * final_gain_lin)
    }
}