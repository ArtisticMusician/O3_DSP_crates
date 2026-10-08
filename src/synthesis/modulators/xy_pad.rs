use crate::synthesis::utils::ParameterSmoother;
use std::f32::consts::PI;

/// XY Pad / Vector modulator for 2D control.
/// Maps X and Y coordinates to audio parameters using polar conversion.
///
/// Use cases:
/// - Mouse/touch control (knob controllers)
/// - Performance controllers
/// - Interactive UI modulation
/// - Dual-knob controllers
///
/// The pad is a control-rate device: setting X/Y recomputes the derived value immediately,
/// so `value()` / `angle()` / `magnitude()` always reflect the newest position. The output
/// gain is smoothed and only advanced by `process_sample()` on the audio path.
#[derive(Debug, Clone, Copy)]
pub struct XYPad {
    // Output value for the current mapping
    current_value: f32,

    // X and Y inputs (normalized -1.0 to 1.0)
    x_input: f32,
    y_input: f32,

    // Output mapping
    output_type: XYPadOutput,

    // Smoothed parameters
    output_gain_smoother: ParameterSmoother,

    // Current angle and magnitude
    current_angle: f32,
    current_magnitude: f32,
}

/// XY Pad output mapping types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XYPadOutput {
    /// Polar: angle as the primary output, magnitude alongside
    Polar,
    /// XY: X as the primary output, Y alongside
    Xy,
    /// Single: X and Y combined into one value (the magnitude)
    Single,
    /// Color: X and Y mapped to an HSV-like colour, returned as mean RGB
    Color,
}

impl Default for XYPad {
    fn default() -> Self {
        Self::new(48000.0, XYPadOutput::Polar)
    }
}

impl XYPad {
    /// Create a new XY pad.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `output_type` - How to map X/Y to output
    pub fn new(sample_rate: f32, output_type: XYPadOutput) -> Self {
        let mut pad = Self {
            current_value: 0.0,
            x_input: 0.0,
            y_input: 0.0,
            output_type,
            output_gain_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
            current_angle: 0.0,
            current_magnitude: 0.0,
        };
        pad.update_derived();
        pad
    }

    /// Set X input (-1.0 to 1.0)
    pub fn set_x(&mut self, x: f32) {
        self.x_input = x.clamp(-1.0, 1.0);
        self.update_derived();
    }

    /// Set Y input (-1.0 to 1.0)
    pub fn set_y(&mut self, y: f32) {
        self.y_input = y.clamp(-1.0, 1.0);
        self.update_derived();
    }

    /// Set both inputs at once
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.x_input = x.clamp(-1.0, 1.0);
        self.y_input = y.clamp(-1.0, 1.0);
        self.update_derived();
    }

    /// Set output gain (smoothed, linear)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.clamp(0.0, 8.0));
    }

    /// Set output type
    pub fn set_output_type(&mut self, output_type: XYPadOutput) {
        self.output_type = output_type;
        self.update_derived();
    }

    /// Recompute angle, magnitude and the mapped output for the current position.
    fn update_derived(&mut self) {
        self.current_angle = self.y_input.atan2(self.x_input);
        self.current_magnitude = (self.x_input * self.x_input + self.y_input * self.y_input)
            .sqrt()
            .min(1.0);

        self.current_value = match self.output_type {
            XYPadOutput::Polar => (self.current_angle / PI).clamp(-1.0, 1.0),
            XYPadOutput::Xy => self.x_input,
            XYPadOutput::Single => self.current_magnitude,
            XYPadOutput::Color => {
                // Hue from the angle, brightness from the magnitude.
                let hue = (self.current_angle / (2.0 * PI)) + 0.5;
                hsv_mean(hue, 1.0, self.current_magnitude)
            }
        };
    }

    /// Get current output
    pub fn value(&self) -> f32 {
        self.current_value * self.output_gain_smoother.current_value()
    }

    /// Get current angle (-PI to PI)
    pub fn angle(&self) -> f32 {
        self.current_angle
    }

    /// Get current magnitude (0 to 1)
    pub fn magnitude(&self) -> f32 {
        self.current_magnitude
    }

    /// Process a single sample, advancing the output-gain smoother.
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let output_gain = self.output_gain_smoother.process_sample();
        self.current_value * output_gain
    }

    /// Process a block of samples
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(0.0);
        }
    }
}

/// Mean of the RGB triple for an HSV colour, all channels in 0..=1.
fn hsv_mean(hue: f32, saturation: f32, value: f32) -> f32 {
    let h6 = hue.rem_euclid(1.0) * 6.0;
    let chroma = value * saturation;
    let second = chroma * (1.0 - ((h6 % 2.0) - 1.0).abs());
    let base = value - chroma;

    let (r, g, b) = match h6 as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };

    (r + g + b) / 3.0 + base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xy_pad_polar() {
        let mut pad = XYPad::new(48000.0, XYPadOutput::Polar);

        pad.set_xy(0.0, 1.0); // Top
        let value = pad.value();
        assert!((value - 0.5).abs() < 0.01, "straight up should be half scale: {value}");
        assert!((pad.angle() - PI / 2.0).abs() < 1e-5);
    }

    #[test]
    fn test_xy_pad_single() {
        let mut pad = XYPad::new(48000.0, XYPadOutput::Single);

        pad.set_xy(0.0, 0.0); // Center
        assert!(pad.magnitude() < 0.01);

        pad.set_xy(1.0, 0.0); // Right
        assert!(pad.magnitude() > 0.9);
        assert!((pad.value() - pad.magnitude()).abs() < 1e-6);
    }

    #[test]
    fn test_xy_pad_xy() {
        let mut pad = XYPad::new(48000.0, XYPadOutput::Xy);

        pad.set_xy(0.5, 0.0);
        assert!((pad.value() - 0.5).abs() < 0.01);
    }

    #[test]
    fn magnitude_is_clamped_to_the_unit_circle() {
        let mut pad = XYPad::new(48_000.0, XYPadOutput::Single);
        pad.set_xy(1.0, 1.0); // corner
        assert!(pad.magnitude() <= 1.0);
    }

    #[test]
    fn colour_output_stays_in_range() {
        let mut pad = XYPad::new(48_000.0, XYPadOutput::Color);

        for step in 0..64 {
            let angle = step as f32 / 64.0 * 2.0 * PI;
            pad.set_xy(angle.cos(), angle.sin());
            let value = pad.value();
            assert!((0.0..=1.0).contains(&value), "colour out of range: {value}");
        }
    }
}
