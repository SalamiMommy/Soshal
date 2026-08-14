//! Lifestyle & values compatibility scoring (smoking, drinking, politics, intent).

#[doc(hidden)]
pub fn score_smoking(self_smoking: Option<&str>, other_smoking: Option<&str>) -> f64 {
    match (self_smoking, other_smoking) {
        (Some(s), Some(o)) => {
            if s == o {
                return 1.0;
            }
            let never = ["occasionally"];
            let occasionally = ["never", "regularly"];
            let regularly = ["occasionally"];
            let adjacent = match s {
                "never" => &never[..],
                "occasionally" => &occasionally[..],
                "regularly" => &regularly[..],
                _ => &[][..],
            };
            if adjacent.contains(&o) {
                0.5
            } else {
                0.0
            }
        }
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_drinking(self_drinking: Option<&str>, other_drinking: Option<&str>) -> f64 {
    match (self_drinking, other_drinking) {
        (Some(s), Some(o)) => {
            if s == o {
                return 1.0;
            }
            let never = ["socially"];
            let socially = ["never", "regularly"];
            let regularly = ["socially"];
            let adjacent = match s {
                "never" => &never[..],
                "socially" => &socially[..],
                "regularly" => &regularly[..],
                _ => &[][..],
            };
            if adjacent.contains(&o) {
                0.5
            } else {
                0.0
            }
        }
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_politics(self_politics: Option<&str>, other_politics: Option<&str>) -> f64 {
    match (self_politics, other_politics) {
        (Some(s), Some(o)) => {
            if s == "prefer not to say" || o == "prefer not to say" {
                return 0.5;
            }
            if s == o {
                return 1.0;
            }
            let liberal = ["moderate"];
            let moderate = ["liberal", "conservative"];
            let conservative = ["moderate", "libertarian"];
            let libertarian = ["conservative", "other"];
            let other = ["moderate", "libertarian"];
            let adjacent = match s {
                "liberal" => &liberal[..],
                "moderate" => &moderate[..],
                "conservative" => &conservative[..],
                "libertarian" => &libertarian[..],
                "other" => &other[..],
                _ => &[][..],
            };
            if adjacent.contains(&o) {
                0.5
            } else {
                0.0
            }
        }
        _ => 0.5,
    }
}

#[doc(hidden)]
pub fn score_relationship_intent(self_intent: Option<&str>, other_intent: Option<&str>) -> f64 {
    match (self_intent, other_intent) {
        (Some(s), Some(o)) => {
            if s == o {
                return 1.0;
            }
            if s == "still figuring out" || o == "still figuring out" {
                return 0.5;
            }
            if (s == "serious" && o == "casual") || (s == "casual" && o == "serious") {
                return 0.0;
            }
            0.5
        }
        _ => 0.5,
    }
}
