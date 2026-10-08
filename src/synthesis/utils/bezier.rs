/// Cubic Bezier curve primitives.
///
/// Three forms, each answering a different need:
/// - [`Bezier::evaluate`] - the raw parametric cubic point, for drawing or tracing a curve
/// - [`Bezier::ease`] - a normalised easing curve `y = f(x)`, x and y both in 0..=1, the form an
///   envelope segment needs. This is the CSS `cubic-bezier(x1, y1, x2, y2)` curve, with the
///   implicit endpoints `P0 = (0, 0)` and `P3 = (1, 1)`.
/// - [`Bezier::shape`] - control points picked from a single bipolar "shape" dial, so a UI knob
///   can sweep a segment from logarithmic through linear to exponential.
///
/// Real-time safe: no allocation, fixed iteration counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bezier;

/// Newton iterations used to invert x(t). Eight is well past the point where f32 stops moving.
const SOLVER_ITERATIONS: usize = 8;

impl Bezier {
    /// A point on the cubic Bezier through `p0`, `p1`, `p2`, `p3` at parameter `t` (0..=1).
    #[inline]
    pub fn evaluate(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        let mt = 1.0 - t;

        mt * mt * mt * p0 + 3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t * p3
    }

    /// One axis of a normalised cubic Bezier with endpoints 0 and 1 and control values `a1`, `a2`.
    #[inline]
    fn axis(t: f32, a1: f32, a2: f32) -> f32 {
        let mt = 1.0 - t;
        3.0 * mt * mt * t * a1 + 3.0 * mt * t * t * a2 + t * t * t
    }

    /// Find the curve parameter `t` whose x lands on `x`, by Newton iteration from `t = x`.
    #[inline]
    fn solve_t(x: f32, x1: f32, x2: f32) -> f32 {
        let mut t = x;

        for _ in 0..SOLVER_ITERATIONS {
            let error = Self::axis(t, x1, x2) - x;
            if error.abs() < 1e-7 {
                break;
            }

            let mt = 1.0 - t;
            let slope = 3.0 * mt * mt * x1 + 6.0 * mt * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2);

            if slope.abs() < 1e-6 {
                break;
            }
            t -= error / slope;
        }

        t.clamp(0.0, 1.0)
    }

    /// Normalised easing curve: `y` for an input `x`, both in 0..=1.
    /// `ease(0.0, ..) == 0.0` and `ease(1.0, ..) == 1.0` exactly, whatever the control points.
    #[inline]
    pub fn ease(x: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }

        let t = Self::solve_t(x, x1, x2);
        Self::axis(t, y1, y2)
    }

    /// Control points for a single bipolar `shape` dial:
    /// `-1.0` = logarithmic (fast start), `0.0` = linear, `1.0` = exponential (slow start).
    ///
    /// Interpolates between three well-known easing curves. Every x control point stays inside
    /// 0..=1, which keeps x(t) monotonic and the solver stable.
    pub fn shape(shape: f32) -> (f32, f32, f32, f32) {
        const LINEAR: (f32, f32, f32, f32) = (1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
        const LOGARITHMIC: (f32, f32, f32, f32) = (0.0, 0.0, 0.58, 1.0);
        const EXPONENTIAL: (f32, f32, f32, f32) = (0.42, 0.0, 1.0, 1.0);

        let amount = shape.clamp(-1.0, 1.0);
        let (from, to, t) = if amount < 0.0 {
            (LINEAR, LOGARITHMIC, -amount)
        } else {
            (LINEAR, EXPONENTIAL, amount)
        };

        (
            from.0 + (to.0 - from.0) * t,
            from.1 + (to.1 - from.1) * t,
            from.2 + (to.2 - from.2) * t,
            from.3 + (to.3 - from.3) * t,
        )
    }

    /// Convenience: apply the [`Self::shape`] dial directly to `x`.
    #[inline]
    pub fn shaped(x: f32, shape: f32) -> f32 {
        let (x1, y1, x2, y2) = Self::shape(shape);
        Self::ease(x, x1, y1, x2, y2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_exact() {
        for params in [
            (0.25, 0.1, 0.25, 1.0),
            (0.42, 0.0, 0.58, 1.0),
            (0.0, 0.0, 1.0, 1.0),
        ] {
            let (x1, y1, x2, y2) = params;
            assert_eq!(Bezier::ease(0.0, x1, y1, x2, y2), 0.0);
            assert_eq!(Bezier::ease(1.0, x1, y1, x2, y2), 1.0);
        }
    }

    #[test]
    fn symmetric_curve_crosses_the_middle() {
        // Symmetric control points must pass through (0.5, 0.5).
        let mid = Bezier::ease(0.5, 0.42, 0.0, 0.58, 1.0);
        assert!((mid - 0.5).abs() < 1e-4, "ease(0.5) = {mid}");
    }

    #[test]
    fn linear_control_points_are_the_identity() {
        for step in 0..=100 {
            let x = step as f32 / 100.0;
            let y = Bezier::ease(x, 1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0);
            assert!((y - x).abs() < 1e-4, "ease({x}) = {y}");
        }
    }

    #[test]
    fn monotonic_control_points_give_a_monotonic_curve() {
        for (x1, y1, x2, y2) in [(0.1, 0.0, 0.9, 1.0), (0.42, 0.0, 0.58, 1.0), (0.6, 0.2, 0.9, 0.9)] {
            let mut previous = 0.0;
            for step in 0..=200 {
                let x = step as f32 / 200.0;
                let y = Bezier::ease(x, x1, y1, x2, y2);
                assert!(y.is_finite(), "non-finite at x = {x}");
                assert!(y >= previous - 1e-4, "not monotonic at x = {x}: {previous} -> {y}");
                previous = y;
            }
        }
    }

    #[test]
    fn shape_dial_bends_the_curve_both_ways() {
        // Exponential (slow start) sits below linear; logarithmic sits above.
        let exponential = Bezier::shaped(0.25, 1.0);
        let logarithmic = Bezier::shaped(0.25, -1.0);
        let linear = Bezier::shaped(0.25, 0.0);

        assert!(exponential < 0.25, "exponential should lag: {exponential}");
        assert!(logarithmic > 0.25, "logarithmic should lead: {logarithmic}");
        assert!((linear - 0.25).abs() < 0.02, "shape 0 should be linear: {linear}");
    }

    #[test]
    fn evaluate_hits_its_endpoints() {
        assert!((Bezier::evaluate(0.0, -1.0, 0.0, 0.0, 1.0) - (-1.0)).abs() < 1e-6);
        assert!((Bezier::evaluate(1.0, -1.0, 0.0, 0.0, 1.0) - 1.0).abs() < 1e-6);
    }
}
