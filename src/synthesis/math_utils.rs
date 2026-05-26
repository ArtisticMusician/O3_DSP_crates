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