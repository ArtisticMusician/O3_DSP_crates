use crate::synthesis::utils::Processor;

/// A wrapper that turns an O3DSP `Processor` into a standard Rust `Iterator`.
/// This is highly useful for integrating with crates like `cpal` or `rodio`
/// that consume audio streams via iterators.
pub struct ProcessorIterator<'a, P: Processor, I: Iterator<Item = f32>> {
    pub processor: &'a mut P,
    /// If there's an input stream (for effects), provide it here.
    /// If it's a generator (oscillator), this can be an iterator of 0.0s.
    pub input_stream: I,
}

impl<'a, P: Processor, I: Iterator<Item = f32>> ProcessorIterator<'a, P, I> {
    /// Creates a new iterator for an effect processor that requires an input stream.
    pub fn new(processor: &'a mut P, input_stream: I) -> Self {
        Self {
            processor,
            input_stream,
        }
    }
}

impl<'a, P: Processor> ProcessorIterator<'a, P, std::iter::Repeat<f32>> {
    /// Creates a new iterator for a generator (like an Oscillator or LFO) that does not need input.
    pub fn generator(processor: &'a mut P) -> Self {
        Self {
            processor,
            input_stream: std::iter::repeat(0.0),
        }
    }
}

impl<'a, P: Processor, I: Iterator<Item = f32>> Iterator for ProcessorIterator<'a, P, I> {
    type Item = f32;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let input = self.input_stream.next()?;
        Some(self.processor.process_sample(input))
    }
}
