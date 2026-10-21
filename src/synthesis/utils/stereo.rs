//! Run two independent copies of a mono stage, one per channel.

use crate::synthesis::utils::Processor;

/// A mono processor turned into a stereo one by keeping a second instance for the right channel.
///
/// This is the honest way to put a mono effect or filter in a stereo path: each channel needs its
/// own delay lines and filter state, and sharing one instance would smear the two channels
/// together. Both instances are built from the same prototype, so they are configured identically.
#[derive(Debug, Clone)]
pub struct StereoStage<S> {
    left: S,
    right: S,
}

impl<S: Clone> StereoStage<S> {
    /// Build both channels from the same configured stage.
    pub fn new(stage: S) -> Self {
        Self {
            left: stage.clone(),
            right: stage,
        }
    }

    /// Reach the left instance, for setting parameters on both at once.
    pub fn left_mut(&mut self) -> &mut S {
        &mut self.left
    }

    /// Reach the right instance.
    pub fn right_mut(&mut self) -> &mut S {
        &mut self.right
    }

    /// Reconfigure both channels with the same call.
    pub fn configure(&mut self, apply: impl Fn(&mut S)) {
        apply(&mut self.left);
        apply(&mut self.right);
    }
}

impl<S: Processor> StereoStage<S> {
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        (
            self.left.process_sample(left),
            self.right.process_sample(right),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-pole-ish leaky integrator, enough to prove the two instances keep separate state.
    struct Leak {
        state: f32,
    }

    impl Processor for Leak {
        fn process_sample(&mut self, sample: f32) -> f32 {
            self.state = (self.state + sample) * 0.5;
            self.state
        }
    }

    impl Clone for Leak {
        fn clone(&self) -> Self {
            Self { state: self.state }
        }
    }

    #[test]
    fn the_two_channels_do_not_share_state() {
        let mut stage = StereoStage::new(Leak { state: 0.0 });

        let (left, right) = stage.process_stereo(1.0, 0.0);
        assert_eq!((left, right), (0.5, 0.0), "the right channel saw the left input");

        let (left, right) = stage.process_stereo(0.0, 1.0);
        assert_eq!(left, 0.25, "left state was disturbed by the right channel");
        assert_eq!(right, 0.5);
    }

    #[test]
    fn configure_reaches_both_instances() {
        let mut stage = StereoStage::new(Leak { state: 0.0 });
        stage.configure(|leak| leak.state = 2.0);

        assert_eq!(stage.left_mut().state, 2.0);
        assert_eq!(stage.right_mut().state, 2.0);
    }
}
