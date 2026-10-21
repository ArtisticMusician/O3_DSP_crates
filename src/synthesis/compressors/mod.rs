pub mod drum_compressor;
pub mod gate_expander;
pub mod lookahead_limiter;
pub mod peak_limiter;
pub mod ssl_glue_compressor;
pub mod vocal_compressor;

pub use drum_compressor::DrumCompressor;
pub use gate_expander::GateExpander;
pub use lookahead_limiter::LookaheadLimiter;
pub use peak_limiter::PeakLimiter;
pub use ssl_glue_compressor::SslGlueCompressor;
pub use vocal_compressor::VocalCompressor;

/// One dynamics processor, selected at runtime.
///
/// Same role `FilterModule` plays for filters: one type the routing code can hold without knowing
/// which compressor it is. Every dynamics processor in this module takes a stereo pair and returns
/// one, so this is a stereo stage rather than a [`Processor`](crate::synthesis::utils::Processor).
#[derive(Debug, Clone)]
pub enum DynamicsModule {
    DrumCompressor(DrumCompressor),
    GateExpander(GateExpander),
    LookaheadLimiter(LookaheadLimiter),
    PeakLimiter(PeakLimiter),
    SslGlueCompressor(SslGlueCompressor),
    VocalCompressor(VocalCompressor),
}

impl DynamicsModule {
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        match self {
            DynamicsModule::DrumCompressor(dynamics) => dynamics.process_sample(left, right),
            DynamicsModule::GateExpander(dynamics) => dynamics.process_sample(left, right),
            DynamicsModule::LookaheadLimiter(dynamics) => dynamics.process_sample(left, right),
            DynamicsModule::PeakLimiter(dynamics) => dynamics.process_sample(left, right),
            DynamicsModule::SslGlueCompressor(dynamics) => dynamics.process_sample(left, right),
            DynamicsModule::VocalCompressor(dynamics) => dynamics.process_sample(left, right),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_passes_a_stereo_pair_and_stays_finite() {
        let mut modules = vec![
            DynamicsModule::DrumCompressor(DrumCompressor::new(48_000.0)),
            DynamicsModule::GateExpander(GateExpander::new(48_000.0)),
            DynamicsModule::LookaheadLimiter(LookaheadLimiter::new(48_000.0)),
            DynamicsModule::PeakLimiter(PeakLimiter::new(48_000.0)),
            DynamicsModule::SslGlueCompressor(SslGlueCompressor::new(48_000.0)),
            DynamicsModule::VocalCompressor(VocalCompressor::new(48_000.0)),
        ];

        for dynamics in modules.iter_mut() {
            let mut peak = 0.0f32;

            for index in 0..4_800 {
                let t = index as f32 / 48_000.0;
                let sample = (std::f32::consts::TAU * 220.0 * t).sin() * 0.5;
                let (left, right) = dynamics.process_stereo(sample, -sample);

                assert!(
                    left.is_finite() && right.is_finite(),
                    "a dynamics variant produced a non-finite sample"
                );
                peak = peak.max(left.abs().max(right.abs()));
            }

            assert!(peak > 1e-3, "a dynamics variant passed nothing");
        }
    }
}
