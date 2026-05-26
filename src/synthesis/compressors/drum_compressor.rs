#[derive(Clone, Debug)]
pub struct DrumCompressor {
    sample_rate: f32,
    threshold_db: f32,
    ratio_slope: f32,
    attack_coef: f32,
    release_coef: f32,
    envelope: f32,
}

impl DrumCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut comp = Self {
            sample_rate,
            threshold_db: -12.0,
            ratio_slope: 0.75, 
            attack_coef: 0.0,
            release_coef: 0.0,
            envelope: 0.0,
        };
        comp.set_attack_ms(5.0);   
        comp.set_release_ms(50.0); 
        comp
    }

    pub fn set_threshold(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db;
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        let ratio = ratio.max(1.0);
        self.ratio_slope = 1.0 - (1.0 / ratio);
    }

    pub fn set_attack_ms(&mut self, ms: f32) {
        self.attack_coef = (-1.0 / ((ms * 0.001).max(1e-5) * self.sample_rate)).exp();
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        self.release_coef = (-1.0 / ((ms * 0.001).max(1e-5) * self.sample_rate)).exp();
    }

    #[inline(always)]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let sidechain_lin = left.abs().max(right.abs()).max(1e-12);
        let sidechain_db = sidechain_lin.ln() * 8.6858896;

        let over_threshold = (sidechain_db - self.threshold_db).max(0.0);
        let target_gr_db = over_threshold * self.ratio_slope;

        if target_gr_db > self.envelope {
            self.envelope = self.attack_coef * self.envelope + (1.0 - self.attack_coef) * target_gr_db;
        } else {
            self.envelope = self.release_coef * self.envelope + (1.0 - self.release_coef) * target_gr_db;
        }

        let final_gain_lin = (-self.envelope * 0.11512925).exp();

        (left * final_gain_lin, right * final_gain_lin)
    }
}