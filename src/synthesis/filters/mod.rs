pub mod ladder_filter;
pub mod one_pole;
pub mod biquad;
pub mod state_variable;
pub mod moog_ladder;
pub mod parametric_eq;
pub mod fir;
pub mod ladder_filter_impl;

pub use ladder_filter::LadderFilter;
pub use one_pole::OnePoleFilter;
pub use biquad::{BiquadFilter, BiquadType};
pub use state_variable::{StateVariableFilter, SvfMode};
pub use moog_ladder::MoogLadderFilter;
pub use parametric_eq::ParametricEq;
pub use fir::FirFilter;
pub use ladder_filter_impl::LadderFilterProcessor;

use crate::synthesis::utils::Processor;

pub enum FilterModule {
    OnePole(OnePoleFilter),
    Ladder(LadderFilterProcessor),
    Biquad(BiquadFilter),
    StateVariable(StateVariableFilter),
    MoogLadder(MoogLadderFilter),
}

impl Processor for FilterModule {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        match self {
            FilterModule::OnePole(flt) => flt.process(sample),
            FilterModule::Ladder(flt) => flt.process_sample(sample),
            FilterModule::Biquad(flt) => flt.process_sample(sample),
            FilterModule::StateVariable(flt) => flt.process_sample(sample),
            FilterModule::MoogLadder(flt) => flt.process_sample(sample),
        }
    }
}
