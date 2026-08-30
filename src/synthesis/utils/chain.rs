/// A trait for processing audio sample-by-sample and block-by-block.
pub trait Processor {
    /// Process a single sample.
    fn process_sample(&mut self, sample: f32) -> f32;

    /// Process a block of samples in-place.
    fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }
}

/// A sequential chain of processors.
pub struct ProcessorChain<P1: Processor, P2: Processor> {
    pub p1: P1,
    pub p2: P2,
}

impl<P1: Processor, P2: Processor> ProcessorChain<P1, P2> {
    pub fn new(p1: P1, p2: P2) -> Self {
        Self { p1, p2 }
    }
}

impl<P1: Processor, P2: Processor> Processor for ProcessorChain<P1, P2> {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        let intermediate = self.p1.process_sample(sample);
        self.p2.process_sample(intermediate)
    }

    #[inline]
    fn process_block(&mut self, buffer: &mut [f32]) {
        self.p1.process_block(buffer);
        self.p2.process_block(buffer);
    }
}

/// A parallel combination of processors (mixes their outputs).
pub struct ParallelProcessor<P1: Processor, P2: Processor> {
    pub p1: P1,
    pub p2: P2,
    pub mix: f32, // 0.0 to 1.0 mix factor? Or just sum. Let's do simple sum.
}

impl<P1: Processor, P2: Processor> ParallelProcessor<P1, P2> {
    pub fn new(p1: P1, p2: P2) -> Self {
        Self { p1, p2, mix: 0.5 }
    }
}

impl<P1: Processor, P2: Processor> Processor for ParallelProcessor<P1, P2> {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        let out1 = self.p1.process_sample(sample);
        let out2 = self.p2.process_sample(sample);
        out1 * self.mix + out2 * (1.0 - self.mix)
    }

    // Default process_block will fall back to process_sample which is okay,
    // but we can optimize it if we want.
}
