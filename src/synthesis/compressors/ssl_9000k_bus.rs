use crate::synthesis::math_utils::{db_to_linear_approx, linear_to_db_approx, TWO_PI};

/// Stepped attack times from the SSL G / 9000 center-section bus compressor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslBusAttack {
    Ms0_1,
    Ms0_3,
    Ms1,
    Ms3,
    Ms10,
    Ms30,
}

impl SslBusAttack {
    #[inline(always)]
    pub fn milliseconds(self) -> f32 {
        match self {
            Self::Ms0_1 => 0.1,
            Self::Ms0_3 => 0.3,
            Self::Ms1 => 1.0,
            Self::Ms3 => 3.0,
            Self::Ms10 => 10.0,
            Self::Ms30 => 30.0,
        }
    }
}

/// Stepped release + the dual-rate Auto program circuit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslBusRelease {
    Ms100,
    Ms300,
    Ms600,
    Ms1200,
    Auto,
}

impl SslBusRelease {
    #[inline(always)]
    pub fn milliseconds(self) -> Option<f32> {
        match self {
            Self::Ms100 => Some(100.0),
            Self::Ms300 => Some(300.0),
            Self::Ms600 => Some(600.0),
            Self::Ms1200 => Some(1200.0),
            Self::Auto => None,
        }
    }
}

/// Hardware ratios. 2:1 is the mix-bus glue setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslBusRatio {
    R2,
    R4,
    R10,
}

impl SslBusRatio {
    #[inline(always)]
    pub fn value(self) -> f32 {
        match self {
            Self::R2 => 2.0,
            Self::R4 => 4.0,
            Self::R10 => 10.0,
        }
    }
}

/// One-pole highpass used only on the sidechain (kick-filter).
#[derive(Debug, Clone, Copy)]
struct SidechainHpf {
    enabled: bool,
    coeff: f32,
    x1: f32,
    y1: f32,
}

impl SidechainHpf {
    fn new() -> Self {
        Self {
            enabled: false,
            coeff: 0.0,
            x1: 0.0,
            y1: 0.0,
        }
    }

    fn set_hz(&mut self, hz: f32, sample_rate: f32) {
        if hz <= 1.0 {
            self.enabled = false;
            self.x1 = 0.0;
            self.y1 = 0.0;
        } else {
            self.enabled = true;
            self.coeff = (-TWO_PI * hz.min(sample_rate * 0.45) / sample_rate).exp();
        }
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    #[inline(always)]
    fn process(&mut self, x: f32) -> f32 {
        if !self.enabled {
            return x;
        }
        let y = self.coeff * (self.y1 + x - self.x1);
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// Master-bus glue compressor modelled on the SSL 9000K centre-section bus VCA.
///
/// Topology notes (this is not the 9000 *channel* RMS compressor):
/// - The 9000K mix bus used the G-series bus-compressor circuit family
///   (THAT 2181 SuperAnalogue VCA, dominant L/R peak detector).
/// - Feedback sidechain: the detector sees the already-reduced signal, which is
///   what makes 1–4 dB of GR feel like glue instead of a clamp.
/// - Ratio shifts the effective threshold and knee the way the hardware network does.
/// - Auto release is the dual time-constant circuit (fast transient + slow programme).
///
/// Defaults: 2:1, 30 ms, Auto, 90 Hz sidechain HPF — the usual mix-bus starting point.
#[derive(Clone, Debug)]
pub struct Ssl9000kBusCompressor {
    sample_rate: f32,

    threshold_db: f32,
    ratio: SslBusRatio,
    ratio_slope: f32,
    knee_half_db: f32,
    knee_inv: f32,
    thresh_shift_db: f32,
    makeup_db: f32,
    mix: f32,

    attack_coef: f32,
    release_coef: f32,
    auto_release: bool,
    envelope_db: f32,

    auto_fast_attack_coef: f32,
    auto_fast_release_coef: f32,
    auto_slow_attack_coef: f32,
    auto_slow_release_coef: f32,
    auto_fast_env_db: f32,
    auto_slow_env_db: f32,

    hpf_l: SidechainHpf,
    hpf_r: SidechainHpf,

    /// Previous sample's gain-reduction linear gain (feedback detector state).
    last_gr_lin: f32,
}

impl Ssl9000kBusCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut comp = Self {
            sample_rate,
            threshold_db: 0.0,
            ratio: SslBusRatio::R2,
            ratio_slope: 0.5,
            knee_half_db: 3.0,
            knee_inv: 1.0 / 12.0,
            thresh_shift_db: -3.0,
            makeup_db: 0.0,
            mix: 1.0,
            attack_coef: 0.0,
            release_coef: 0.0,
            auto_release: true,
            envelope_db: 0.0,
            auto_fast_attack_coef: 0.0,
            auto_fast_release_coef: 0.0,
            auto_slow_attack_coef: 0.0,
            auto_slow_release_coef: 0.0,
            auto_fast_env_db: 0.0,
            auto_slow_env_db: 0.0,
            hpf_l: SidechainHpf::new(),
            hpf_r: SidechainHpf::new(),
            last_gr_lin: 1.0,
        };
        comp.set_ratio(SslBusRatio::R2);
        comp.set_attack(SslBusAttack::Ms30);
        comp.set_release(SslBusRelease::Auto);
        comp.set_sidechain_hpf_hz(90.0);
        comp
    }

    pub fn set_threshold_db(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db.clamp(-20.0, 10.0);
    }

    pub fn set_ratio(&mut self, ratio: SslBusRatio) {
        self.ratio = ratio;
        let r = ratio.value();
        self.ratio_slope = 1.0 - (1.0 / r);
        // Hardware ratio network also moves the knee and the effective threshold
        // so 2:1 still feels loud. Lower ratio → earlier, softer onset.
        match ratio {
            SslBusRatio::R2 => {
                self.knee_half_db = 3.0;
                self.thresh_shift_db = -3.0;
            }
            SslBusRatio::R4 => {
                self.knee_half_db = 2.0;
                self.thresh_shift_db = -1.5;
            }
            SslBusRatio::R10 => {
                self.knee_half_db = 1.0;
                self.thresh_shift_db = 0.0;
            }
        }
        self.knee_inv = 1.0 / (4.0 * self.knee_half_db);
    }

    pub fn set_attack(&mut self, attack: SslBusAttack) {
        self.attack_coef = self.coef(attack.milliseconds());
        self.auto_fast_attack_coef = self.attack_coef;
    }

    pub fn set_release(&mut self, release: SslBusRelease) {
        match release.milliseconds() {
            Some(ms) => {
                self.auto_release = false;
                self.release_coef = self.coef(ms);
            }
            None => {
                self.auto_release = true;
                // Dual-rate Auto: short recovery after hits, long after sustained GR.
                self.auto_fast_release_coef = self.coef(200.0);
                self.auto_slow_release_coef = self.coef(2500.0);
                self.auto_slow_attack_coef = self.coef(100.0);
            }
        }
    }

    pub fn set_makeup_db(&mut self, makeup_db: f32) {
        self.makeup_db = makeup_db.clamp(0.0, 15.0);
    }

    /// Dry/wet. Hardware is 100% wet; use 0.5 for parallel glue.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Sidechain highpass in Hz. `0` disables (kick will pump the mix).
    pub fn set_sidechain_hpf_hz(&mut self, hz: f32) {
        self.hpf_l.set_hz(hz, self.sample_rate);
        self.hpf_r.set_hz(hz, self.sample_rate);
    }

    pub fn reset(&mut self) {
        self.envelope_db = 0.0;
        self.auto_fast_env_db = 0.0;
        self.auto_slow_env_db = 0.0;
        self.last_gr_lin = 1.0;
        self.hpf_l.reset();
        self.hpf_r.reset();
    }

    /// Positive dB of gain reduction on the last sample (meter needle).
    #[inline(always)]
    pub fn gain_reduction_db(&self) -> f32 {
        self.envelope_db.max(0.0)
    }

    #[inline(always)]
    fn coef(&self, time_ms: f32) -> f32 {
        let time_sec = (time_ms * 0.001).max(1e-5);
        (-1.0 / (time_sec * self.sample_rate)).exp()
    }

    #[inline(always)]
    fn target_gr_db(&self, sidechain_db: f32) -> f32 {
        let over = sidechain_db - (self.threshold_db + self.thresh_shift_db);
        let half = self.knee_half_db;
        if over > half {
            over * self.ratio_slope
        } else if over > -half {
            let x = over + half;
            self.ratio_slope * x * x * self.knee_inv
        } else {
            0.0
        }
    }

    #[inline(always)]
    fn smooth_envelope(&mut self, target_gr_db: f32) {
        if self.auto_release {
            if target_gr_db > self.auto_fast_env_db {
                self.auto_fast_env_db = self.auto_fast_attack_coef * self.auto_fast_env_db
                    + (1.0 - self.auto_fast_attack_coef) * target_gr_db;
            } else {
                self.auto_fast_env_db = self.auto_fast_release_coef * self.auto_fast_env_db
                    + (1.0 - self.auto_fast_release_coef) * target_gr_db;
            }
            if target_gr_db > self.auto_slow_env_db {
                self.auto_slow_env_db = self.auto_slow_attack_coef * self.auto_slow_env_db
                    + (1.0 - self.auto_slow_attack_coef) * target_gr_db;
            } else {
                self.auto_slow_env_db = self.auto_slow_release_coef * self.auto_slow_env_db
                    + (1.0 - self.auto_slow_release_coef) * target_gr_db;
            }
            self.envelope_db = self.auto_fast_env_db.max(self.auto_slow_env_db);
        } else if target_gr_db > self.envelope_db {
            self.envelope_db =
                self.attack_coef * self.envelope_db + (1.0 - self.attack_coef) * target_gr_db;
        } else {
            self.envelope_db =
                self.release_coef * self.envelope_db + (1.0 - self.release_coef) * target_gr_db;
        }
    }

    #[inline(always)]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Feedback detector: sidechain sees the already-reduced bus.
        let det_l = self.hpf_l.process(left * self.last_gr_lin);
        let det_r = self.hpf_r.process(right * self.last_gr_lin);
        let peak = det_l.abs().max(det_r.abs()).max(1e-12);
        let target = self.target_gr_db(linear_to_db_approx(peak));
        self.smooth_envelope(target);

        let gr_lin = db_to_linear_approx(-self.envelope_db);
        self.last_gr_lin = gr_lin;

        let makeup = db_to_linear_approx(self.makeup_db);
        let wet_l = left * gr_lin * makeup;
        let wet_r = right * gr_lin * makeup;
        let mix = self.mix;
        (
            left * (1.0 - mix) + wet_l * mix,
            right * (1.0 - mix) + wet_r * mix,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Ssl9000kBusCompressor, SslBusAttack, SslBusRatio, SslBusRelease};

    #[test]
    fn silence_stays_silence() {
        let mut c = Ssl9000kBusCompressor::new(48_000.0);
        let (l, r) = c.process_sample(0.0, 0.0);
        assert_eq!((l, r), (0.0, 0.0));
    }

    #[test]
    fn hot_bus_reduces_gain() {
        let mut c = Ssl9000kBusCompressor::new(48_000.0);
        c.set_threshold_db(-10.0);
        c.set_ratio(SslBusRatio::R4);
        c.set_attack(SslBusAttack::Ms0_1);
        c.set_release(SslBusRelease::Ms100);
        c.set_makeup_db(0.0);
        c.set_mix(1.0);
        c.set_sidechain_hpf_hz(0.0);
        let mut out_peak = 0.0f32;
        for _ in 0..2048 {
            let (l, r) = c.process_sample(0.9, -0.9);
            out_peak = out_peak.max(l.abs()).max(r.abs());
        }
        assert!(out_peak < 0.9, "expected GR, peak {out_peak}");
        assert!(c.gain_reduction_db() > 0.5, "meter {0}", c.gain_reduction_db());
    }

    #[test]
    fn quiet_signal_is_near_unity() {
        let mut c = Ssl9000kBusCompressor::new(48_000.0);
        c.set_threshold_db(0.0);
        c.set_makeup_db(0.0);
        c.set_mix(1.0);
        let mut y = 0.0;
        for _ in 0..512 {
            y = c.process_sample(0.05, 0.05).0;
        }
        assert!((y - 0.05).abs() < 0.005, "quiet gain {y}");
    }
}
