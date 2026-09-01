// Adapter to make LadderFilter work with Processor trait
use crate::synthesis::filters::ladder_filter::LadderFilter;
use crate::synthesis::utils::Processor;

pub struct LadderFilterProcessor(pub LadderFilter);

impl Processor for LadderFilterProcessor {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        self.0.process(sample)
    }
}
