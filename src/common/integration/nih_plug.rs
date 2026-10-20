use crate::synthesis::utils::Processor;

/// Helper trait to seamlessly process a `nih_plug::Buffer` using an O3DSP `Processor`.
/// This trait is automatically implemented for any type that implements `Processor`.
pub trait NihPlugProcessorExt {
    /// Process a multi-channel `nih_plug` buffer in-place.
    ///
    /// **Important:** The standard `Processor` trait in O3DSP is fundamentally single-channel (mono).
    /// If you pass a multi-channel buffer here, the same stateful processor will process all channels
    /// sequentially. For true multi-channel processing without state smearing, you must instantiate
    /// one `Processor` per channel and process them individually.
    ///
    /// This method is primarily useful for simple utility processors or when the buffer is mono.
    fn process_nih_buffer(&mut self, buffer: &mut [&mut [f32]]);
}

impl<P: Processor> NihPlugProcessorExt for P {
    #[inline(always)]
    fn process_nih_buffer(&mut self, buffer: &mut [&mut [f32]]) {
        // Iterate through all channels in the buffer
        for channel_data in buffer.iter_mut() {
            // Process the block for this specific channel
            self.process_block(*channel_data);
        }
    }
}

/// A wrapper to easily process a stereo `nih_plug::Buffer` using two independent O3DSP `Processor`s.
/// This prevents state smearing between the left and right channels.
pub struct StereoProcessorWrapper<P: Processor> {
    pub left: P,
    pub right: P,
}

impl<P: Processor> StereoProcessorWrapper<P> {
    pub fn new(left: P, right: P) -> Self {
        Self { left, right }
    }

    /// Process a stereo `nih_plug` buffer in-place.
    /// Expects exactly 2 channels (Left, Right). If fewer or more channels are provided,
    /// it will only process up to 2 channels.
    #[inline(always)]
    pub fn process_nih_buffer(&mut self, buffer: &mut [&mut [f32]]) {
        if buffer.len() >= 2 {
            // Safe split to get mutable references to both channels simultaneously
            let (left_channels, right_channels) = buffer.split_at_mut(1);
            let left_slice = &mut left_channels[0];
            let right_slice = &mut right_channels[0];

            self.left.process_block(left_slice);
            self.right.process_block(right_slice);
        } else if buffer.len() == 1 {
            // Mono fallback
            self.left.process_block(buffer[0]);
        }
    }
}
