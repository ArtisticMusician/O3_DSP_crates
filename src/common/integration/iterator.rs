use crate::synthesis::utils::Processor;

/// A wrapper that turns an O3DSP `Processor` into a standard Rust `Iterator`.
/// This is highly useful for integrating with crates like `cpal` or `rodio`
/// that consume audio streams via iterators.
pub struct ProcessorIterator<'a, P: Processor> {
    pub processor: &'a mut P,
    /// If there's an input stream (for effects), provide it here.
    /// If it's a generator (oscillator), this can be an iterator of 0.0s.
    pub input_stream: Box<dyn Iterator<Item = f32> + 'a>,
}

impl<'a, P: Processor> ProcessorIterator<'a, P> {
    /// Creates a new iterator for an effect processor that requires an input stream.
    pub fn new<I>(processor: &'a mut P, input_stream: I) -> Self
    where
        I: Iterator<Item = f32> + 'a,
    {
        Self {
            processor,
            input_stream: Box::new(input_stream),
        }
    }

    /// Creates a new iterator for a generator (like an Oscillator or LFO) that does not need input.
    pub fn generator(processor: &'a mut P) -> Self {
        Self {
            processor,
            input_stream: Box::new(std::iter::repeat(0.0)),
        }
    }
}

impl<'a, P: Processor> Iterator for ProcessorIterator<'a, P> {
    type Item = f32;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let input = self.input_stream.next()?;
        Some(self.processor.process_sample(input))
    }
}
