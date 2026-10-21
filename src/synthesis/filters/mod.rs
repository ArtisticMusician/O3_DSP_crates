pub mod allpass;
pub mod biquad;
pub mod comb;
pub mod dc_blocker;
pub mod filter_bank;
pub mod fir;
pub mod ladder_filter;
pub mod ladder_filter_impl;
pub mod moog_ladder;
pub mod one_pole;
pub mod parametric_eq;
pub mod sallen_key;
pub mod shelf;
pub mod linkwitz_riley;
pub mod formant;
pub mod state_variable;
pub mod biquad_bank;
pub mod diode_ladder;

pub use allpass::AllpassFilter;
pub use biquad::{BiquadFilter, BiquadType};
pub use comb::CombFilter;
pub use dc_blocker::DcBlocker;
pub use filter_bank::{AllpassBank, BankMode, CombBank};
pub use fir::FirFilter;
pub use ladder_filter::LadderFilter;
pub use ladder_filter_impl::LadderFilterProcessor;
pub use moog_ladder::MoogLadderFilter;
pub use one_pole::OnePoleFilter;
pub use parametric_eq::ParametricEq;
pub use sallen_key::{SallenKeyFilter, SallenKeyMode};
pub use shelf::{ShelfFilter, ShelfMode};
pub use linkwitz_riley::{LinkwitzRileyCrossover, CrossoverOrder};
pub use formant::FormantFilter;
pub use state_variable::{StateVariableFilter, SvfMode};
pub use biquad_bank::{BiquadBank, GraphicEq};
pub use diode_ladder::DiodeLadderFilter;

use crate::synthesis::utils::Processor;

/// One active topology. For N combs / N allpasses use `CombBank` / `AllpassBank`
/// (or `SchroederReverb`), not this enum.
#[derive(Debug, Clone)]
pub enum FilterModule {
    OnePole(OnePoleFilter),
    Ladder(LadderFilterProcessor),
    Biquad(BiquadFilter),
    StateVariable(StateVariableFilter),
    MoogLadder(MoogLadderFilter),
    DcBlocker(DcBlocker),
    Comb(CombFilter),
    Allpass(AllpassFilter),
    CombBank(CombBank),
    AllpassBank(AllpassBank),
    SallenKey(SallenKeyFilter),
    Shelf(ShelfFilter),
    LinkwitzRiley(LinkwitzRileyCrossover),
    Formant(FormantFilter),
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
            FilterModule::DcBlocker(flt) => flt.process(sample),
            FilterModule::Comb(flt) => flt.process(sample),
            FilterModule::Allpass(flt) => flt.process(sample),
            FilterModule::CombBank(flt) => flt.process(sample),
            FilterModule::AllpassBank(flt) => flt.process(sample),
            FilterModule::SallenKey(flt) => flt.process_sample(sample),
            FilterModule::Shelf(flt) => flt.process_sample(sample),
            FilterModule::LinkwitzRiley(flt) => flt.process_sample(sample),
            FilterModule::Formant(flt) => flt.process_sample(sample),
        }
    }
}
