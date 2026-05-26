pub mod ladder_filter;
pub mod one_pole;

pub use ladder_filter::LadderFilter;
pub use one_pole::OnePoleFilter;

#[derive(Debug, Clone, Copy)]
pub enum FilterModule {
    OnePole(OnePoleFilter),
    Ladder(LadderFilter),
}

impl FilterModule {
    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        match self {
            FilterModule::OnePole(flt) => flt.process(sample),
            FilterModule::Ladder(flt) => flt.process(sample),
        }
    }
}
