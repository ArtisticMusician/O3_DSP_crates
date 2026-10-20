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
        // dasp requires an exhausted signal to keep returning equilibrium.
        if self.source.is_exhausted() {
            return 0.0;
        }

        let input = self.source.next();
        self.processor.process_sample(input)
    }

    /// Mirror the source. A `Processor` has no tail or exhaustion contract of its own, so the
    /// adapter stops exactly when the thing feeding it does.
    #[inline(always)]
    fn is_exhausted(&self) -> bool {
        self.source.is_exhausted()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Passthrough;

    impl Processor for Passthrough {
        fn process_sample(&mut self, sample: f32) -> f32 {
            sample
        }
    }

    #[test]
    fn a_finite_source_exhausts_the_adapter_and_holds_equilibrium() {
        let source = dasp::signal::from_iter(vec![0.5f32, -0.25].into_iter());
        let mut adapter = DaspEffectAdapter::new(source, Passthrough);

        assert!(!adapter.is_exhausted(), "should not start exhausted");
        assert_eq!(adapter.next(), 0.5);
        assert_eq!(adapter.next(), -0.25);
        assert!(adapter.is_exhausted(), "the source is spent");
        assert_eq!(adapter.next(), 0.0, "an exhausted signal must return equilibrium");
    }
}
