pub mod chain;
pub mod hermite_interpolator;
pub mod parameter_smoother;
pub mod resampler;

pub use chain::{ParallelProcessor, Processor, ProcessorChain};
pub use hermite_interpolator::HermiteInterpolator;
pub use parameter_smoother::ParameterSmoother;
pub use resampler::HermiteResampler;
