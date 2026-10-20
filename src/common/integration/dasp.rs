use crate::synthesis::utils::Processor;
use dasp::signal::Signal;

/// A wrapper that adapts any O3DSP generator (a `Processor` that ignores input)
/// into a `dasp::Signal` that outputs `f32` frames.
pub struct DaspGeneratorAdapter<P: Processor> {
    pub processor: P,
}

impl<P: Processor> DaspGeneratorAdapter<P> {
    pub fn new(processor: P) -> Self {
        Self { processor }
    }
}

impl<P: Processor> Signal for DaspGeneratorAdapter<P> {
    type Frame = f32;

    #[inline(always)]
    fn next(&mut self) -> Self::Frame {
        // Pass 0.0 as input since it's acting as a generator.
        self.processor.process_sample(0.0)
    }
}

/// A wrapper that adapts an O3DSP effect (a `Processor` that needs input)
/// so that it consumes an upstream `dasp::Signal` and outputs the filtered result.
pub struct DaspEffectAdapter<S: Signal<Frame = f32>, P: Processor> {
    pub source: S,
    pub processor: P,
}

impl<S: Signal<Frame = f32>, P: Processor> DaspEffectAdapter<S, P> {
    pub fn new(source: S, processor: P) -> Self {
        Self { source, processor }
    }
}

impl<S: Signal<Frame = f32>, P: Processor> Signal for DaspEffectAdapter<S, P> {
    type Frame = f32;

    #[inline(always)]
    fn next(&mut self) -> Self::Frame {
        let input = self.source.next();
        self.processor.process_sample(input)
    }
}
