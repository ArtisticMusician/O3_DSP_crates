use super::biquad::{BiquadFilter, BiquadType};
use crate::synthesis::utils::Processor;

/// A multi-band parametric EQ using cascaded Biquad filters.
pub struct ParametricEq {
    bands: Vec<BiquadFilter>,
}

impl ParametricEq {
    pub fn new(sample_rate: f32, num_bands: usize) -> Self {
        let mut bands = Vec::with_capacity(num_bands);
        for _ in 0..num_bands {
            bands.push(BiquadFilter::new(sample_rate, BiquadType::Peaking));
        }
        Self { bands }
    }

    pub fn set_band(
        &mut self,
        index: usize,
        cutoff_hz: f32,
        q: f32,
        gain_db: f32,
        filter_type: BiquadType,
    ) {
        if let Some(band) = self.bands.get_mut(index) {
            band.filter_type = filter_type;
            band.set_parameters(cutoff_hz, q, gain_db);
        }
    }
}

impl Processor for ParametricEq {
    #[inline(always)]
    fn process_sample(&mut self, mut sample: f32) -> f32 {
        for band in self.bands.iter_mut() {
            sample = band.process_sample(sample);
        }
        sample
    }
}
