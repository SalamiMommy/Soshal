//! Physical & background metric compatibility scoring.

/// Scoring for physical distance between two profiles.
/// Missing distance never scores (caller skips the dimension).
#[doc(hidden)]
pub fn score_distance(distance_km: f64) -> f64 {
    if !distance_km.is_finite() {
        return 0.5;
    }
    if distance_km <= 10.0 {
        1.0
    } else if distance_km <= 50.0 {
        0.8
    } else if distance_km <= 100.0 {
        0.6
    } else if distance_km <= 250.0 {
        0.4
    } else {
        0.2
    }
}

#[doc(hidden)]
pub fn score_age(self_age: Option<f64>, other_age: Option<f64>) -> f64 {
    match (self_age, other_age) {
        (Some(s), Some(o)) => {
            if !s.is_finite() || !o.is_finite() {
                return 0.5;
            }
            let diff = (s - o).abs();
            if diff <= 3.0 {
                1.0
            } else if diff <= 5.0 {
                0.5
            } else if diff <= 10.0 {
                0.25
            } else {
                0.0
            }
        }
        (None, None) => 1.0,
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_height(self_height: Option<f64>, other_height: Option<f64>) -> f64 {
    match (self_height, other_height) {
        (Some(s), Some(o)) => {
            if !s.is_finite() || !o.is_finite() {
                return 0.5;
            }
            let diff = (s - o).abs();
            if diff <= 10.0 {
                1.0
            } else if diff <= 20.0 {
                0.5
            } else {
                0.0
            }
        }
        (None, None) => 1.0,
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_body_type(self_type: Option<&str>, other_type: Option<&str>) -> f64 {
    match (self_type, other_type) {
        (Some(s), Some(o)) => {
            if s == o {
                return 1.0;
            }
            let slim = ["athletic", "average"];
            let athletic = ["slim", "average", "muscular"];
            let average = ["slim", "athletic", "curvy"];
            let curvy = ["average", "muscular"];
            let muscular = ["athletic", "average", "curvy"];
            let similar = match s {
                "slim" => &slim[..],
                "athletic" => &athletic[..],
                "average" => &average[..],
                "curvy" => &curvy[..],
                "muscular" => &muscular[..],
                _ => &[][..],
            };
            let reverse = match o {
                "slim" => &slim[..],
                "athletic" => &athletic[..],
                "average" => &average[..],
                "curvy" => &curvy[..],
                "muscular" => &muscular[..],
                _ => &[][..],
            };
            // Symmetric: scores must not depend on caller order. Two body
            // types are "similar" if either adjacency list says so.
            if similar.contains(&o) || reverse.contains(&s) {
                0.5
            } else {
                0.0
            }
        }
        (None, None) => 1.0,
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_ethnicity(self_ethnicity: Option<&str>, other_ethnicity: Option<&str>) -> f64 {
    match (self_ethnicity, other_ethnicity) {
        (Some(s), Some(o)) if s == o => 1.0,
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_education(self_education: Option<&str>, other_education: Option<&str>) -> f64 {
    match (self_education, other_education) {
        (Some(s), Some(o)) => {
            if s == o {
                return 1.0;
            }
            let get_tier = |ed: &str| -> i32 {
                match ed {
                    "high school" => 0,
                    "some college" | "associate" | "trade school" => 1,
                    "bachelor's" => 2,
                    "master's" => 3,
                    "doctorate" => 4,
                    _ => 0,
                }
            };
            let diff = (get_tier(s) - get_tier(o)).abs();
            if diff <= 1 {
                0.5
            } else {
                0.25
            }
        }
        _ => 0.5,
    }
}
