use crate::synthesis::utils::Processor;

/// Process a block of channel slices with an O3DSP [`Processor`].
///
/// Automatically implemented for every `Processor`. The slices are whatever layout the host hands
/// you (one slice per channel), so this works with any plugin framework or audio callback.
pub trait SliceProcessorExt {
    /// Process every channel of a block in place.
    ///
    /// **Important:** The standard `Processor` trait in O3DSP is fundamentally single-channel (mono).
    /// If you pass a multi-channel buffer here, the same stateful processor will process all channels
    /// sequentially. For true multi-channel processing without state smearing, you must instantiate
    /// one `Processor` per channel and process them individually.
    ///
    /// This method is primarily useful for simple utility processors or when the buffer is mono.
    fn process_slices(&mut self, channels: &mut [&mut [f32]]);
}

impl<P: Processor> SliceProcessorExt for P {
    #[inline(always)]
    fn process_slices(&mut self, channels: &mut [&mut [f32]]) {
        for channel_data in channels.iter_mut() {
            // Process the block for this specific channel
            self.process_block(*channel_data);
        }
    }
}

/// Two independent [`Processor`]s driven from one stereo block, so the channels keep their own
/// state instead of smearing into each other.
pub struct StereoProcessorWrapper<P: Processor> {
    pub left: P,
    pub right: P,
}

impl<P: Processor> StereoProcessorWrapper<P> {
    pub fn new(left: P, right: P) -> Self {
        Self { left, right }
    }

    /// Process a stereo block: the first two channels go to the left and right processors, and a
    /// single channel is treated as mono.
    #[inline(always)]
    pub fn process_slices(&mut self, channels: &mut [&mut [f32]]) {
        if channels.len() >= 2 {
            let (first, rest) = channels.split_at_mut(1);
            let (second, _) = rest.split_at_mut(1);

            self.left.process_block(first[0]);
            self.right.process_block(second[0]);
        } else if let Some(mono) = channels.first_mut() {
            self.left.process_block(mono);
        }
    }
}
