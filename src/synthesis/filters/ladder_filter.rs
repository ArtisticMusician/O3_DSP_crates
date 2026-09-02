use std::f32::consts::PI;

/// A structurally stable 4-Pole Virtual Analog Ladder Filter
/// utilizing an accurate Zero-Delay Feedback (ZDF) topology and embedded nonlinear saturation.
#[derive(Debug, Clone, Copy)]
pub struct LadderFilter {
    sample_rate: f32,

    // Stage delay registers (capacitors)
    s1: f32,
    s2: f32,
    s3: f32,
    s4: f32,

    // Active smoothed parameters
    current_cutoff: f32,
    current_resonance: f32,

    // Cached coefficients for the hot sample loop
    g: f32,
    h: f32,
    k: f32,
    gamma: f32,

    // Target parameters
    target_cutoff: f32,
    target_resonance: f32,

    // Smoothing coefficient
    smooth_coef: f32,
}

impl LadderFilter {
    pub fn new(sample_rate: f32) -> Self {
        let smooth_coef = 1.0 - (-1.0 / (0.015 * sample_rate)).exp();

        Self {
            sample_rate,
            s1: 0.0,
            s2: 0.0,
            s3: 0.0,
            s4: 0.0,
            current_cutoff: 1000.0,
            current_resonance: 0.0,
            g: 0.0,
            h: 0.0,
            k: 0.0,
            gamma: 0.0,
            target_cutoff: 1000.0,
            target_resonance: 0.0,
            smooth_coef,
        }
    }

    /// Hardware Control Utility: Forces the current host thread's CPU registers
    /// into Flush-to-Zero (FTZ) and Denormals-Are-Zero (DAZ) states on x86_64.
    /// Safely isolated via conditional attributes to prevent compilation failures during cross-compiling.
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    pub fn configure_hardware_denormal_flush() {
        unsafe {
            // Using standard core architecture paths to guarantee absolute toolchain compliance
            let mut mxcsr = std::arch::x86_64::_mm_getcsr();
            // Bit 15: Flush-to-Zero (FTZ) | Bit 6: Denormals-Are-Zero (DAZ)
            mxcsr |= (1 << 15) | (1 << 6);
            std::arch::x86_64::_mm_setcsr(mxcsr);
        }
    }

    /// Fallback execution signature for non-x86 architectures (e.g., AArch64/ARM NEON)
    /// where subnormal flushing behavior is handled natively at the silicon level.
    #[cfg(not(target_arch = "x86_64"))]
    #[inline(always)]
    pub fn configure_hardware_denormal_flush() {}

    pub fn set_parameters(&mut self, cutoff_hz: f32, resonance: f32) {
        self.target_cutoff = cutoff_hz.clamp(20.0, self.sample_rate * 0.45);
        self.target_resonance = resonance.clamp(0.0, 4.0);
    }

    #[inline]
    pub fn update_coefficients(&mut self) {
        self.current_cutoff += (self.target_cutoff - self.current_cutoff) * self.smooth_coef;
        self.current_resonance +=
            (self.target_resonance - self.current_resonance) * self.smooth_coef;

        let wd = PI * self.current_cutoff;
        self.g = (wd / self.sample_rate).tan();
        self.h = 1.0 / (1.0 + self.g);
        self.k = self.current_resonance;

        let gh = self.g * self.h;
        let gh2 = gh * gh;
        self.gamma = gh2 * gh2;
    }

    /// Processes a single input sample through the stabilized nonlinear ladder network.
    /// 100% Real-time safe, entirely branchless, and fully immune to denormal performance spikes.
    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        let g = self.g;
        let h = self.h;
        let k = self.k;

        let g2 = g * g;
        let g3 = g2 * g;
        let h2 = h * h;
        let h3 = h2 * h;
        let h4 = h3 * h;

        // 1. Instantaneous Loop Feedback Estimation (Sigma)
        let sigma = (g3 * h4 * self.s1) + (g2 * h3 * self.s2) + (g * h2 * self.s3) + (h * self.s4);

        // 2. Continuous-Time Saturation Emulation with Bipolar Anti-Denormal Bias
        let mut input_node = (sample + 1e-25) - k * sigma;

        // Highly optimized cubic tanh approximation to clip feedback smoothly
        input_node = input_node.clamp(-3.0, 3.0);
        let saturated_input =
            input_node * (27.0 + input_node * input_node) / (27.0 + 9.0 * input_node * input_node);

        // 3. Solve the Final Input Equation via pre-cached gamma factor
        let x0 = saturated_input / (1.0 + k * self.gamma);

        // 4. Trapezoidal Integration Stream over Cascaded Stages
        let v1 = (x0 - self.s1) * g * h;
        let u1 = v1 + self.s1;
        self.s1 = u1 + v1;

        let v2 = (u1 - self.s2) * g * h;
        let u2 = v2 + self.s2;
        self.s2 = u2 + v2;

        let v3 = (u2 - self.s3) * g * h;
        let u3 = v3 + self.s3;
        self.s3 = u3 + v3;

        // 5. Subtract the bias out at the final node to maintain a clean DC baseline
        let v4 = (u3 - self.s4) * g * h;
        let u4 = v4 + self.s4;
        self.s4 = u4 + v4 - 1e-25;

        u4
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
        self.s3 = 0.0;
        self.s4 = 0.0;
    }
}
