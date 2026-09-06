/// Converts dB to voltage gain ratio: linear = 10^(db/20). Real-time safe.
#[inline(always)]
pub fn db_to_linear(decibels: f32) -> f32 {
    if decibels <= -144.0 {
        0.0
    } else {
        10.0_f32.powf(decibels * 0.05)
    }
}

/// Converts gain ratio to dB: db = 20 * log10(linear). Real-time safe.
#[inline(always)]
pub fn linear_to_db(linear: f32) -> f32 {
    if linear <= 0.0 {
        -144.0
    } else {
        20.0 * linear.log10()
    }
}

/// Fast approximate conversion from dB to linear gain using exp.
/// Equivalent to 10^(db/20) but avoids powf on the audio thread.
#[inline(always)]
pub fn db_to_linear_approx(db: f32) -> f32 {
    (db * 0.11512925).exp()
}

/// Fast approximate conversion from linear to dB using ln.
/// Equivalent to 20*log10(lin). Clamps low values to avoid -inf.
#[inline(always)]
pub fn linear_to_db_approx(lin: f32) -> f32 {
    lin.max(1e-12).ln() * 8.6858896
}

/// Mathematical constants for oscillator and filter calculations.
pub const PI: f32 = std::f32::consts::PI;
pub const TWO_PI: f32 = PI * 2.0;
