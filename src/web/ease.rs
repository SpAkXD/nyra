//! Easing for the motion words of `use web`: the presets and springs.
//!
//! A spring is simulated at build time (a damped harmonic oscillator going from 0 to 1, mass 1,
//! starting at rest) and written as a CSS `linear(...)` easing with as few points as keep it within
//! a small error, plus the time it takes to settle, which becomes the duration. Springs therefore
//! need no JavaScript. Browsers without `linear()` get the closest `cubic-bezier(...)`, found by a
//! small search (see [`Curve::bezier`]).

// The build (WP5) calls this through `motion`; until it lands only the tests use all of it.
#![allow(dead_code)]

/// The default easing: an expo-out that reads as smooth without feeling floaty.
pub const OUT: &str = "cubic-bezier(.16,1,.3,1)";
/// For panels and things that leave and arrive: slow at both ends.
pub const IN_OUT: &str = "cubic-bezier(.76,0,.24,1)";

/// The easing preset words, in the order the docs list them.
pub const PRESETS: &[&str] = &["out", "in-out", "spring", "bounce", "snappy", "linear"];

/// The springs behind the preset words `spring`, `bounce` and `snappy`: (stiffness, damping).
pub const SPRING: (f64, f64) = (210.0, 20.0);
pub const BOUNCE: (f64, f64) = (240.0, 11.0);
pub const SNAPPY: (f64, f64) = (520.0, 40.0);

/// A spring has settled when it stays this close to its target.
const EPS: f64 = 0.001;
/// The largest error allowed between the `linear()` points and the simulated curve.
const TOLERANCE: f64 = 0.002;
/// A spring that takes longer than this to settle is refused: it would wobble for seconds.
pub const MAX_SECONDS: f64 = 4.0;

/// A damped spring pulling from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub stiffness: f64,
    pub damping: f64,
}

impl Spring {
    pub fn new(stiffness: f64, damping: f64) -> Spring {
        Spring { stiffness, damping }
    }

    /// Why this spring cannot be used, if it cannot: the values must be positive and it must
    /// settle within [`MAX_SECONDS`].
    pub fn check(&self) -> Result<(), String> {
        let (k, d) = (self.stiffness, self.damping);
        if !(k.is_finite() && d.is_finite()) || !(1.0..=5000.0).contains(&k) {
            return Err(format!("the stiffness must be from 1 to 5000 (found {})", num(k, 2)));
        }
        if !(0.5..=500.0).contains(&d) {
            return Err(format!("the damping must be from 0.5 to 500 (found {})", num(d, 2)));
        }
        let t = self.settle();
        if t > MAX_SECONDS {
            return Err(format!(
                "`spring {} {}` takes {}s to settle; more damping (or more stiffness) makes it settle sooner",
                num(k, 2),
                num(d, 2),
                num(t, 1)
            ));
        }
        Ok(())
    }

    /// The damping ratio: below 1 the spring overshoots, 1 is critical, above 1 it creeps in.
    pub fn ratio(&self) -> f64 {
        self.damping / (2.0 * self.stiffness.sqrt())
    }

    /// The position at `t` seconds: 0 at the start, 1 at rest.
    pub fn at(&self, t: f64) -> f64 {
        let w0 = self.stiffness.sqrt();
        let z = self.ratio();
        if (z - 1.0).abs() < 1e-9 {
            1.0 - (-w0 * t).exp() * (1.0 + w0 * t)
        } else if z < 1.0 {
            let wd = w0 * (1.0 - z * z).sqrt();
            1.0 - (-z * w0 * t).exp() * ((wd * t).cos() + (z * w0 / wd) * (wd * t).sin())
        } else {
            let s = (z * z - 1.0).sqrt();
            let (r1, r2) = (-w0 * (z - s), -w0 * (z + s));
            // x(t) = 1 + a e^(r1 t) + b e^(r2 t) with x(0) = 0 and x'(0) = 0
            let a = r2 / (r1 - r2);
            let b = -1.0 - a;
            1.0 + a * (r1 * t).exp() + b * (r2 * t).exp()
        }
    }

    /// Seconds until the spring stays within `EPS` of its target, rounded up to 10 ms.
    pub fn settle(&self) -> f64 {
        let (mut last, mut t) = (0.0, 0.0);
        let dt = 0.001;
        while t < MAX_SECONDS * 3.0 {
            if (self.at(t) - 1.0).abs() > EPS {
                last = t;
            } else if t - last > 0.5 && t > 6.0 / self.stiffness.sqrt() {
                // half a second quiet after the slowest part of the motion: settled
                break;
            }
            t += dt;
        }
        ((last + dt) * 100.0).ceil() / 100.0
    }

    /// The spring as a CSS easing over its settle time.
    pub fn curve(&self) -> Curve {
        let duration = self.settle().max(0.05);
        let n = 240;
        let pts: Vec<(f64, f64)> = (0..=n)
            .map(|i| {
                let p = i as f64 / n as f64;
                (p, if i == n { 1.0 } else { self.at(p * duration) })
            })
            .collect();
        let keep = simplify(&pts, TOLERANCE);
        Curve { duration, points: keep, bezier: fit_bezier(&pts) }
    }
}

/// An easing curve: points of (progress of time, progress of value), both from 0 to 1, the time
/// it should take, and its nearest cubic Bézier.
#[derive(Clone, Debug)]
pub struct Curve {
    /// Seconds.
    pub duration: f64,
    pub points: Vec<(f64, f64)>,
    /// `cubic-bezier(x1, y1, x2, y2)`
    pub bezier: [f64; 4],
}

impl Curve {
    /// `linear(0, .2 4%, ..., 1)`
    pub fn linear(&self) -> String {
        let last = self.points.len() - 1;
        let parts: Vec<String> = self
            .points
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| if i == 0 || i == last { num(y, 3) } else { format!("{} {}%", num(y, 3), num(x * 100.0, 1)) })
            .collect();
        format!("linear({})", parts.join(","))
    }

    /// `cubic-bezier(.3,1.4,.5,1)`: for browsers without `linear()`.
    pub fn bezier_css(&self) -> String {
        let b = self.bezier;
        format!("cubic-bezier({},{},{},{})", num(b[0], 3), num(b[1], 3), num(b[2], 3), num(b[3], 3))
    }

    /// The largest difference between the `linear()` points (joined by straight lines) and `f`.
    pub fn max_error(&self, f: impl Fn(f64) -> f64) -> f64 {
        (0..=1000)
            .map(|i| {
                let x = i as f64 / 1000.0;
                (piecewise(&self.points, x) - f(x)).abs()
            })
            .fold(0.0, f64::max)
    }
}

/// An easing a motion word can use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ease {
    Out,
    InOut,
    Linear,
    Spring(Spring),
}

/// The easing of a preset word (`out`, `in-out`, `spring`, `bounce`, `snappy`, `linear`).
pub fn preset(name: &str) -> Option<Ease> {
    Some(match name {
        "out" => Ease::Out,
        "in-out" => Ease::InOut,
        "linear" => Ease::Linear,
        "spring" => Ease::Spring(Spring::new(SPRING.0, SPRING.1)),
        "bounce" => Ease::Spring(Spring::new(BOUNCE.0, BOUNCE.1)),
        "snappy" => Ease::Spring(Spring::new(SNAPPY.0, SNAPPY.1)),
        _ => return None,
    })
}

impl Ease {
    /// The CSS `<easing-function>`: springs as `linear(...)`.
    pub fn css(&self) -> String {
        match self {
            Ease::Out => OUT.to_string(),
            Ease::InOut => IN_OUT.to_string(),
            Ease::Linear => "linear".to_string(),
            Ease::Spring(s) => s.curve().linear(),
        }
    }

    /// The easing for browsers without `linear()`: the same for every preset but the springs.
    pub fn fallback(&self) -> String {
        match self {
            Ease::Spring(s) => s.curve().bezier_css(),
            _ => self.css(),
        }
    }

    /// A spring's own duration in seconds (its settle time); the other easings take the word's.
    pub fn duration(&self) -> Option<f64> {
        match self {
            Ease::Spring(s) => Some(s.curve().duration),
            _ => None,
        }
    }
}

/// A number for CSS with at most `digits` decimals, without trailing zeros or a leading zero:
/// `0.500` is `.5`, `1.0` is `1`, `-0.25` is `-.25`.
pub fn num(x: f64, digits: usize) -> String {
    let mut s = format!("{x:.digits$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".to_string();
    }
    if let Some(rest) = s.strip_prefix("0.") {
        s = format!(".{rest}");
    } else if let Some(rest) = s.strip_prefix("-0.") {
        s = format!("-.{rest}");
    }
    s
}

/// The value of points joined by straight lines at `x`.
fn piecewise(pts: &[(f64, f64)], x: f64) -> f64 {
    for w in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return if x1 - x0 < 1e-12 { y1 } else { y0 + (y1 - y0) * (x - x0) / (x1 - x0) };
        }
    }
    pts[pts.len() - 1].1
}

/// Ramer-Douglas-Peucker on the vertical distance: the fewest points whose straight lines stay
/// within `tol` of every original point. The first and last points are kept.
fn simplify(pts: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let ((x0, y0), (x1, y1)) = (pts[a], pts[b]);
        let (mut worst, mut at) = (0.0, a);
        for (i, &(x, y)) in pts.iter().enumerate().take(b).skip(a + 1) {
            let line = y0 + (y1 - y0) * (x - x0) / (x1 - x0);
            let d = (y - line).abs();
            if d > worst {
                worst = d;
                at = i;
            }
        }
        if worst > tol {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// The y of a CSS cubic Bézier (from (0,0) to (1,1)) at a given x.
pub fn bezier_at(b: [f64; 4], x: f64) -> f64 {
    let [x1, y1, x2, y2] = b;
    let coord = |t: f64, p1: f64, p2: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    // x(t) is monotonic for x1, x2 in [0, 1]: bisection always finds t
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if coord(mid, x1, x2) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    coord((lo + hi) / 2.0, y1, y2)
}

/// The cubic Bézier closest to a curve (least squares over the samples), found by a pattern
/// search from a few starting points. It cannot wobble, so for a bouncy spring it is the best
/// single overshoot.
fn fit_bezier(pts: &[(f64, f64)]) -> [f64; 4] {
    let samples: Vec<(f64, f64)> = pts.iter().step_by(4).copied().collect();
    let cost = |b: [f64; 4]| samples.iter().map(|&(x, y)| (bezier_at(b, x) - y).powi(2)).sum::<f64>();
    let starts = [[0.16, 1.0, 0.3, 1.0], [0.34, 1.56, 0.64, 1.0], [0.2, 1.4, 0.4, 1.0], [0.5, 2.0, 0.5, 0.8]];
    let mut best = (f64::MAX, starts[0]);
    for s in starts {
        let mut b = s;
        let mut c = cost(b);
        let mut step = 0.1;
        while step > 0.0005 {
            let mut moved = false;
            for i in 0..4 {
                for dir in [-1.0, 1.0] {
                    let mut nb = b;
                    nb[i] += dir * step;
                    if i % 2 == 0 {
                        nb[i] = nb[i].clamp(0.0, 1.0);
                    }
                    let nc = cost(nb);
                    if nc < c {
                        b = nb;
                        c = nc;
                        moved = true;
                    }
                }
            }
            if !moved {
                step /= 2.0;
            }
        }
        if c < best.0 {
            best = (c, b);
        }
    }
    best.1.map(|v| (v * 1000.0).round() / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spring_starts_at_zero_and_ends_at_one() {
        for (k, d) in [SPRING, BOUNCE, SNAPPY, (100.0, 20.0), (100.0, 40.0), (300.0, 60.0)] {
            let s = Spring::new(k, d);
            assert!(s.at(0.0).abs() < 1e-12, "{k} {d}");
            assert!((s.at(s.settle()) - 1.0).abs() <= EPS * 1.5, "{k} {d}: {}", s.at(s.settle()));
            // it starts at rest: the first millisecond barely moves
            assert!(s.at(0.001) < 0.001, "{k} {d}");
        }
    }

    #[test]
    fn critical_and_overdamped_springs_never_overshoot() {
        for (k, d) in [(100.0, 20.0), (100.0, 40.0), (400.0, 90.0)] {
            let s = Spring::new(k, d);
            assert!(s.ratio() >= 1.0);
            let max = (0..3000).map(|i| s.at(i as f64 / 1000.0)).fold(0.0, f64::max);
            assert!(max <= 1.0 + 1e-9, "{k} {d}: {max}");
        }
    }

    #[test]
    fn underdamped_springs_overshoot_as_the_ratio_says() {
        let s = Spring::new(BOUNCE.0, BOUNCE.1);
        let peak = (0..3000).map(|i| s.at(i as f64 / 1000.0)).fold(0.0, f64::max);
        // the first peak of a damped oscillator: 1 + exp(-pi z / sqrt(1 - z^2))
        let z = s.ratio();
        let expected = 1.0 + (-std::f64::consts::PI * z / (1.0 - z * z).sqrt()).exp();
        assert!((peak - expected).abs() < 0.002, "{peak} vs {expected}");
        assert!(peak > 1.2, "bounce must visibly bounce: {peak}");
        let gentle = Spring::new(SPRING.0, SPRING.1);
        let peak = (0..3000).map(|i| gentle.at(i as f64 / 1000.0)).fold(0.0, f64::max);
        assert!(peak > 1.02 && peak < 1.12, "spring overshoots a little: {peak}");
    }

    #[test]
    fn the_linear_easing_follows_the_spring_closely() {
        for (k, d) in [SPRING, BOUNCE, SNAPPY, (260.0, 16.0)] {
            let s = Spring::new(k, d);
            let c = s.curve();
            let err = c.max_error(|p| if p >= 1.0 { 1.0 } else { s.at(p * c.duration) });
            assert!(err <= TOLERANCE * 1.5, "{k} {d}: error {err}");
            assert!(c.points.len() < 60, "{k} {d}: {} points", c.points.len());
            let css = c.linear();
            assert!(css.starts_with("linear(0,") && css.ends_with(",1)"), "{css}");
            assert!(css.len() < 700, "{k} {d}: {} bytes", css.len());
        }
    }

    #[test]
    fn durations_follow_stiffness_and_damping() {
        let soft = Spring::new(100.0, 10.0).settle();
        let stiff = Spring::new(400.0, 20.0).settle();
        assert!(stiff < soft, "{stiff} {soft}");
        let gentle = Spring::new(SPRING.0, SPRING.1).settle();
        assert!((0.4..1.0).contains(&gentle), "{gentle}");
        let snappy = Spring::new(SNAPPY.0, SNAPPY.1).settle();
        assert!(snappy < gentle, "{snappy} {gentle}");
        let bounce = Spring::new(BOUNCE.0, BOUNCE.1).settle();
        assert!((0.8..2.0).contains(&bounce), "{bounce}");
    }

    #[test]
    fn the_bezier_fallback_is_close() {
        for (k, d, limit) in [(SPRING.0, SPRING.1, 0.07), (SNAPPY.0, SNAPPY.1, 0.05), (BOUNCE.0, BOUNCE.1, 0.25)] {
            let s = Spring::new(k, d);
            let c = s.curve();
            let err = (1..100)
                .map(|i| {
                    let x = i as f64 / 100.0;
                    (bezier_at(c.bezier, x) - s.at(x * c.duration)).abs()
                })
                .fold(0.0, f64::max);
            assert!(err < limit, "{k} {d}: {} off by {err}", c.bezier_css());
            assert!(c.bezier[0] >= 0.0 && c.bezier[0] <= 1.0 && c.bezier[2] >= 0.0 && c.bezier[2] <= 1.0);
        }
        // an overshooting spring gets a Bézier that overshoots too
        assert!(Spring::new(SPRING.0, SPRING.1).curve().bezier[1] > 1.0);
    }

    #[test]
    fn bezier_at_matches_known_curves() {
        let lin = [0.0, 0.0, 1.0, 1.0];
        for x in [0.0, 0.25, 0.5, 1.0] {
            assert!((bezier_at(lin, x) - x).abs() < 1e-6);
        }
        // ease-out-ish curves are ahead of linear in the middle
        assert!(bezier_at([0.16, 1.0, 0.3, 1.0], 0.3) > 0.8);
    }

    #[test]
    fn bad_springs_are_refused_with_a_reason() {
        assert!(Spring::new(SPRING.0, SPRING.1).check().is_ok());
        assert!(Spring::new(260.0, 16.0).check().is_ok());
        let e = Spring::new(100.0, 0.6).check().unwrap_err();
        assert!(e.contains("to settle"), "{e}");
        assert!(Spring::new(0.0, 10.0).check().unwrap_err().contains("stiffness"));
        assert!(Spring::new(100.0, 0.0).check().unwrap_err().contains("damping"));
        assert!(Spring::new(f64::NAN, 10.0).check().is_err());
    }

    #[test]
    fn presets() {
        assert_eq!(preset("out").unwrap().css(), OUT);
        assert_eq!(preset("in-out").unwrap().css(), IN_OUT);
        assert_eq!(preset("linear").unwrap().css(), "linear");
        assert!(preset("spring").unwrap().css().starts_with("linear(0,"));
        assert!(preset("bounce").unwrap().fallback().starts_with("cubic-bezier("));
        assert!(preset("snappy").unwrap().duration().unwrap() < 0.6);
        assert!(preset("wobbly").is_none());
        for p in PRESETS {
            assert!(preset(p).is_some(), "{p}");
        }
    }

    #[test]
    fn numbers_are_short() {
        assert_eq!(num(0.5, 3), ".5");
        assert_eq!(num(1.0, 3), "1");
        assert_eq!(num(-0.25, 3), "-.25");
        assert_eq!(num(-0.0001, 3), "0");
        assert_eq!(num(12.3456, 1), "12.3");
        assert_eq!(num(100.0, 1), "100");
    }
}
