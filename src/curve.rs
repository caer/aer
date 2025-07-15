//! Unstable

use palette::Oklab;

/// TODO:
pub fn bezier_point(t: f32, start: f32, control: f32, end: f32) -> f32 {
    let one_minus_t = 1.0 - t;
    one_minus_t * one_minus_t * start + 2.0 * one_minus_t * t * control + t * t * end
}

/// Solves for `t` such that the `L` component of the
/// Bézier curve defined by `p0`, `p1`, and `p2` equals `target_l`.
pub fn find_t_for_l(target_l: f32, p0: Oklab, p1: Oklab, p2: Oklab) -> Option<f32> {
    // Quadratic Bezier in L channel:
    // L(t) = (1 - t)^2 * p0.l + 2(1 - t)t * p1.l + t^2 * p2.l
    // Rearranged as: At^2 + Bt + C = 0
    let a = p0.l - 2.0 * p1.l + p2.l;
    let b = -2.0 * p0.l + 2.0 * p1.l;
    let c = p0.l - target_l;

    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None; // No real root
    }

    let sqrt_disc = discriminant.sqrt();
    let t1 = (-b + sqrt_disc) / (2.0 * a);
    let t2 = (-b - sqrt_disc) / (2.0 * a);

    // Return the root in [0, 1]
    [t1, t2].into_iter().find(|&t| (0.0..=1.0).contains(&t))
}

/// TODO:
pub fn generate_oklab_samples(p1: Oklab, desired_l_values: &[f32]) -> Vec<Oklab> {
    let start_point = Oklab {
        l: 1.0,
        a: 0.0,
        b: 0.0,
    }; // Pure white
    let end_point = Oklab {
        l: 0.0,
        a: 0.0,
        b: 0.0,
    }; // Pure black
    let control_point = p1;

    desired_l_values
        .iter()
        .filter_map(|&l| {
            find_t_for_l(l, start_point, p1, end_point).map(|t| {
                let l = bezier_point(t, start_point.l, control_point.l, end_point.l);
                let a = bezier_point(t, start_point.a, control_point.a, end_point.a);
                let b = bezier_point(t, start_point.b, control_point.b, end_point.b);

                Oklab { l, a, b }
            })
        })
        .collect()
}
