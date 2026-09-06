use crate::synthesis::compressors::ssl_9000k_bus::{SslBusAttack, SslBusRatio, SslBusRelease};
use crate::synthesis::math_utils::{db_to_linear_approx, linear_to_db_approx};

/// Reason mixer Master Compressor — the SSL-lookalike panel with the VU needle.
///
/// Control surface is the G-bus set. The ballistics are Reason's, not the 9000K
/// centre section:
/// - detector is stereo-linked `(L+R)/2` (ReasonTalk measurements)
/// - attack/release are constant-dB slews quoted as ms per 40 dB
/// - threshold −30..0 dB, makeup −5..+15 dB
/// - ON bypass and KEY external sidechain
///
/// Defaults match a typical Reason mix-bus start: 4:1, 3 ms, Auto, 0 dB makeup.
#[derive(Clone, Debug)]
pub struct MasterCompressor {
    sample_rate: f32,
    enabled: bool,
    key_enabled: bool,

    threshold_db: f32,
    ratio_slope: f32,
    knee_half_db: f32,
    knee_inv: f32,
    makeup_lin: f32,

    attack_db_per_sample: f32,
    release_db_per_sample: f32,
    auto_release: bool,
    envelope_db: f32,
    auto_fast_env_db: f32,
    auto_slow_env_db: f32,
    auto_fast_attack: f32,
    auto_fast_release: f32,
    auto_slow_attack: f32,
    auto_slow_release: f32,
}

impl MasterCompressor {
    pub fn new(sample_rate: f32) -> Self {
        let mut comp = Self {
            sample_rate,
            enabled: true,
            key_enabled: false,
            threshold_db: -12.0,
            ratio_slope: 0.75,
            knee_half_db: 2.0,
            knee_inv: 0.125,
            makeup_lin: 1.0,
            attack_db_per_sample: 0.0,
            release_db_per_sample: 0.0,
            auto_release: true,
            envelope_db: 0.0,
            auto_fast_env_db: 0.0,
            auto_slow_env_db: 0.0,
            auto_fast_attack: 0.0,
            auto_fast_release: 0.0,
            auto_slow_attack: 0.0,
            auto_slow_release: 0.0,
        };
        comp.set_ratio(SslBusRatio::R4);
        comp.set_attack(SslBusAttack::Ms3);
        comp.set_release(SslBusRelease::Auto);
        comp.set_makeup_db(0.0);
        comp
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.envelope_db = 0.0;
            self.auto_fast_env_db = 0.0;
            self.auto_slow_env_db = 0.0;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// KEY: detector follows the external sidechain instead of the mix bus.
    pub fn set_key(&mut self, on: bool) {
        self.key_enabled = on;
    }

    pub fn key_enabled(&self) -> bool {
        self.key_enabled
    }

    /// Reason range: −30 dB … 0 dB.
    pub fn set_threshold_db(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db.clamp(-30.0, 0.0);
    }

    pub fn set_ratio(&mut self, ratio: SslBusRatio) {
        let r = ratio.value();
        self.ratio_slope = 1.0 - (1.0 / r);
        self.knee_half_db = match ratio {
            SslBusRatio::R2 => 3.0,
            SslBusRatio::R4 => 2.0,
            SslBusRatio::R10 => 1.0,
        };
        self.knee_inv = 1.0 / (4.0 * self.knee_half_db);
    }

    pub fn set_attack(&mut self, attack: SslBusAttack) {
        // Reason mixer Master Section: ms per 40 dB (ReasonTalk measurements).
        let ms_per_40 = match attack {
            SslBusAttack::Ms0_1 => 0.40,
            SslBusAttack::Ms0_3 => 0.70,
            SslBusAttack::Ms1 => 1.35,
            SslBusAttack::Ms3 => 3.92,
            SslBusAttack::Ms10 => 11.6,
            SslBusAttack::Ms30 => 43.5,
        };
        self.attack_db_per_sample = 40.0 / (ms_per_40 * 0.001 * self.sample_rate).max(1e-6);
        self.auto_fast_attack = self.attack_db_per_sample;
    }

    pub fn set_release(&mut self, release: SslBusRelease) {
        match release {
            SslBusRelease::Auto => {
                self.auto_release = true;
                // Dual-rate Auto: short hits recover fast, sustained GR holds.
                self.auto_fast_release = 40.0 / (0.200 * self.sample_rate);
                self.auto_slow_release = 40.0 / (2.500 * self.sample_rate);
                self.auto_slow_attack = 40.0 / (0.100 * self.sample_rate);
            }
            other => {
                self.auto_release = false;
                let ms_per_40 = match other {
                    SslBusRelease::Ms100 => 648.0,
                    SslBusRelease::Ms300 => 870.0,
                    SslBusRelease::Ms600 => 1492.0,
                    SslBusRelease::Ms1200 => 2612.0,
                    SslBusRelease::Auto => unreachable!(),
                };
                self.release_db_per_sample =
                    40.0 / (ms_per_40 * 0.001 * self.sample_rate).max(1e-6);
            }
        }
    }

    /// Reason range: −5 dB … +15 dB.
    pub fn set_makeup_db(&mut self, makeup_db: f32) {
        self.makeup_lin = db_to_linear_approx(makeup_db.clamp(-5.0, 15.0));
    }

    /// Needle on the 0–20 dB COMPRESSION meter.
    #[inline(always)]
    pub fn gain_reduction_db(&self) -> f32 {
        self.envelope_db.clamp(0.0, 20.0)
    }

    pub fn reset(&mut self) {
        self.envelope_db = 0.0;
        self.auto_fast_env_db = 0.0;
        self.auto_slow_env_db = 0.0;
    }

    #[inline(always)]
    fn slew(current: f32, target: f32, attack: f32, release: f32) -> f32 {
        if target > current {
            (current + attack).min(target)
        } else {
            (current - release).max(target)
        }
    }

    #[inline(always)]
    fn target_gr_db(&self, sidechain_db: f32) -> f32 {
        let over = sidechain_db - self.threshold_db;
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
    fn update_envelope(&mut self, target: f32) {
        if self.auto_release {
            self.auto_fast_env_db = Self::slew(
                self.auto_fast_env_db,
                target,
                self.auto_fast_attack,
                self.auto_fast_release,
            );
            self.auto_slow_env_db = Self::slew(
                self.auto_slow_env_db,
                target,
                self.auto_slow_attack,
                self.auto_slow_release,
            );
            self.envelope_db = self.auto_fast_env_db.max(self.auto_slow_env_db);
        } else {
            self.envelope_db = Self::slew(
                self.envelope_db,
                target,
                self.attack_db_per_sample,
                self.release_db_per_sample,
            );
        }
    }

    #[inline(always)]
    fn detect(left: f32, right: f32) -> f32 {
        // Reason Master Section is stereo-linked on the mean, not max(L, R).
        (left.abs() + right.abs()) * 0.5
    }

    #[inline(always)]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.process_with_detector(left, right, left, right)
    }

    /// KEY path: `key_l` / `key_r` drive the detector when KEY is on.
    #[inline(always)]
    pub fn process_sample_keyed(
        &mut self,
        left: f32,
        right: f32,
        key_l: f32,
        key_r: f32,
    ) -> (f32, f32) {
        if self.key_enabled {
            self.process_with_detector(left, right, key_l, key_r)
        } else {
            self.process_with_detector(left, right, left, right)
        }
    }

    #[inline(always)]
    fn process_with_detector(
        &mut self,
        left: f32,
        right: f32,
        det_l: f32,
        det_r: f32,
    ) -> (f32, f32) {
        if !self.enabled {
            return (left, right);
        }

        let peak = Self::detect(det_l, det_r).max(1e-12);
        let target = self.target_gr_db(linear_to_db_approx(peak));
        self.update_envelope(target);

        let gain = db_to_linear_approx(-self.envelope_db) * self.makeup_lin;
        (left * gain, right * gain)
    }
}

#[cfg(test)]
mod tests {
    use super::MasterCompressor;
    use crate::synthesis::compressors::ssl_9000k_bus::{SslBusAttack, SslBusRatio, SslBusRelease};

    #[test]
    fn bypass_is_unity() {
        let mut c = MasterCompressor::new(48_000.0);
        c.set_enabled(false);
        c.set_threshold_db(-30.0);
        assert_eq!(c.process_sample(0.5, -0.25), (0.5, -0.25));
    }

    #[test]
    fn hot_signal_moves_the_needle() {
        let mut c = MasterCompressor::new(48_000.0);
        c.set_threshold_db(-20.0);
        c.set_ratio(SslBusRatio::R4);
        c.set_attack(SslBusAttack::Ms0_1);
        c.set_release(SslBusRelease::Ms100);
        c.set_makeup_db(0.0);
        for _ in 0..4096 {
            let _ = c.process_sample(0.8, 0.8);
        }
        assert!(c.gain_reduction_db() > 1.0, "needle {}", c.gain_reduction_db());
    }
}
