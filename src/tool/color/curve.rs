//! Unstable

use palette::Oklab;

/// Returns a set of OKLAB colors with `l_values`, sampled from
/// a quadratic Bézier starting at pure black (`L = 0.0`),
/// ending at pure white (`L = 1.0`), and controlled by `control`.
///
/// Iff `control` has `L <= 0.5`, the control color (with `L = 0.0`)
/// will be used as the starting point.
///
/// Iff `control` has `L >= 0.5`, the control color (with `L = 1.0`)
/// will be used as the ending point.
pub fn sample_quadratic_bezier_oklab_curve(control: Oklab, l_values: &[f32]) -> Vec<Oklab> {
    let start = if control.l <= 0.5 {
        let mut point = control;
        point.l = 0.0;
        point
    } else {
        Oklab {
            l: 0.0,
            a: 0.0,
            b: 0.0,
        }
    };

    let end = if control.l >= 0.5 {
        let mut point = control;
        point.l = 1.0;
        point
    } else {
        Oklab {
            l: 1.0,
            a: 0.0,
            b: 0.0,
        }
    };

    // Find a point `t` on the Bézier curve (defined by the
    // control points) satisfying each target L value.
    let mut l_points = Vec::with_capacity(l_values.len());
    for target_l in l_values {
        // Rearrange the Bézier curve formula:
        // L(t) = (1 - t)^2 * p0.l + 2(1 - t)t * p1.l + t^2 * p2.l
        // with respect to `t`:
        // At^2 + Bt + C = 0
        let a = start.l - 2.0 * control.l + end.l;
        let b = -2.0 * start.l + 2.0 * control.l;
        let c = start.l - target_l;

        let discriminant = b * b - 4.0 * a * c;
        assert!(discriminant >= 0.0, "no real root");

        let sqrt_disc = discriminant.sqrt();
        let t1 = (-b + sqrt_disc) / (2.0 * a);
        let t2 = (-b - sqrt_disc) / (2.0 * a);

        if (0.0..=1.0).contains(&t1) {
            l_points.push(t1);
        } else if (0.0..=1.0).contains(&t2) {
            l_points.push(t2);
        } else {
            unimplemented!()
        }
    }

    // Sample bezier curve for all L points.
    l_points
        .into_iter()
        .map(|t| {
            let l = quadratic_bezier_point(t, start.l, control.l, end.l);
            let a = quadratic_bezier_point(t, start.a, control.a, end.a);
            let b = quadratic_bezier_point(t, start.b, control.b, end.b);
            Oklab { l, a, b }
        })
        .collect()
}

/// Returns the value at point `t` on a quadratic Bézier
/// curve having a `start` point, intermediary `control`
/// point, and `end` point.
fn quadratic_bezier_point(t: f32, start: f32, control: f32, end: f32) -> f32 {
    assert!((0.0..=1.0).contains(&t));
    let one_minus_t = 1.0 - t;
    one_minus_t * one_minus_t * start + 2.0 * one_minus_t * t * control + t * t * end
}
