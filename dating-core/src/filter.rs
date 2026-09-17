//! Dating profile filtering by gender, seeking, and distance.

use crate::{FilterDatingProfilesInput, FilteredDatingProfileOut, MAX_PROFILES};
use std::collections::HashSet;

pub fn filter_dating_profiles(input: FilterDatingProfilesInput) -> Vec<FilteredDatingProfileOut> {
    if input.profiles.len() > MAX_PROFILES {
        return Vec::new();
    }
    let gender_map: &[&str] = match input.own_seeking.as_deref() {
        Some(s) if s.eq_ignore_ascii_case("male") => &["male"],
        Some(s) if s.eq_ignore_ascii_case("female") => &["female"],
        Some(s) if s.eq_ignore_ascii_case("non-binary") => &["non-binary"],
        Some(s) if s.eq_ignore_ascii_case("other") => &["other"],
        _ => &["male", "female", "non-binary", "other"],
    };
    let self_contacts_set: HashSet<String> = input
        .self_contacts
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    let own_coords = input
        .own_location_geohash
        .as_deref()
        .and_then(soshal_spatial_core::distance::decode_geohash_coords);
    input
        .profiles
        .iter()
        .enumerate()
        .map(|(i, profile)| {
            let pubkey_lower = profile.pubkey.trim().to_ascii_lowercase();
            let is_contact = self_contacts_set.contains(&pubkey_lower);
            let passes = (|| -> bool {
                if input.hide_friends.unwrap_or(false) && is_contact {
                    return false;
                }
                if let Some(ref seeking) = input.own_seeking {
                    if !seeking.eq_ignore_ascii_case("all") {
                        match profile.gender.as_ref() {
                            Some(g) => {
                                if !gender_map.iter().any(|m| m.eq_ignore_ascii_case(g)) {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                    }
                }
                if let (Some(ref own_g), Some(ref other_seeking)) =
                    (&input.own_gender, &profile.seeking)
                {
                    // Mutual direction: the other's seeking preference must
                    // include our gender. Seeking values are the lowercase
                    // enum literals ("male"/"female"/"non-binary"/"other"),
                    // NOT "men"/"women" — must match gender_map above.
                    let other_map: &[&str] = if other_seeking.eq_ignore_ascii_case("male") {
                        &["male"]
                    } else if other_seeking.eq_ignore_ascii_case("female") {
                        &["female"]
                    } else if other_seeking.eq_ignore_ascii_case("non-binary") {
                        &["non-binary"]
                    } else if other_seeking.eq_ignore_ascii_case("other") {
                        &["other"]
                    } else {
                        &["male", "female", "non-binary", "other"]
                    };
                    if !other_map.iter().any(|m| m.eq_ignore_ascii_case(own_g)) {
                        return false;
                    }
                }
                if input.min_age.is_some() || input.max_age.is_some() {
                    match profile.age {
                        Some(age) => {
                            if !age.is_finite() || age <= 0.0 {
                                return false;
                            }
                            if let Some(min) = input.min_age {
                                if !min.is_finite() || age < min {
                                    return false;
                                }
                            }
                            if let Some(max) = input.max_age {
                                if !max.is_finite() || age > max {
                                    return false;
                                }
                            }
                        }
                        None => return false,
                    }
                }
                if input.height_min_cm.is_some() || input.height_max_cm.is_some() {
                    match profile.height {
                        Some(height) => {
                            if !height.is_finite() || height <= 0.0 {
                                return false;
                            }
                            if let Some(min) = input.height_min_cm {
                                if !min.is_finite() || height < min {
                                    return false;
                                }
                            }
                            if let Some(max) = input.height_max_cm {
                                if !max.is_finite() || height > max {
                                    return false;
                                }
                            }
                        }
                        None => return false,
                    }
                }
                let matches_trait = |filter: &Option<String>, value: &Option<String>| -> bool {
                    match filter {
                        Some(f) if !f.is_empty() && !f.eq_ignore_ascii_case("any") => match value {
                            Some(v) => v.eq_ignore_ascii_case(f),
                            None => false,
                        },
                        _ => true,
                    }
                };
                if !matches_trait(&input.body_type, &profile.body_type) {
                    return false;
                }
                if !matches_trait(&input.smoking, &profile.smoking) {
                    return false;
                }
                if !matches_trait(&input.drinking, &profile.drinking) {
                    return false;
                }
                if !matches_trait(&input.relationship_intent, &profile.relationship_intent) {
                    return false;
                }
                if !matches_trait(&input.politics, &profile.politics) {
                    return false;
                }
                if !matches_trait(&input.education, &profile.education) {
                    return false;
                }
                if let Some((own_lat, own_lon)) = own_coords {
                    if input.own_max_distance_km.is_some() || profile.max_distance_km.is_some() {
                        let other_coords = profile
                            .location_geohash
                            .as_deref()
                            .and_then(soshal_spatial_core::distance::decode_geohash_coords);
                        let dist = other_coords.map(|(other_lat, other_lon)| {
                            soshal_spatial_core::distance::haversine_km(
                                own_lat, own_lon, other_lat, other_lon,
                            )
                        });

                        if let Some(own_max) = input.own_max_distance_km {
                            if !own_max.is_finite() || own_max < 0.0 {
                                return false;
                            }
                            match dist {
                                Some(d) if d.is_finite() && d <= own_max => {}
                                _ => return false,
                            }
                        }

                        if let Some(other_max) = profile.max_distance_km {
                            if !other_max.is_finite() || other_max < 0.0 {
                                return false;
                            }
                            match dist {
                                Some(d) if d.is_finite() && d <= other_max => {}
                                _ => return false,
                            }
                        }
                    }
                }
                true
            })();

            let mutual_friends = if passes {
                profile
                    .verified_mutual_friends
                    .as_ref()
                    .map(|friends| {
                        let mut seen = HashSet::new();
                        friends
                            .iter()
                            .filter(|f| {
                                let f_lower = f.trim().to_ascii_lowercase();
                                self_contacts_set.contains(&f_lower) && seen.insert(f_lower)
                            })
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            FilteredDatingProfileOut {
                index: i,
                passes,
                is_contact,
                mutual_friends,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DatingProfileInput;

    #[test]
    fn nan_age_and_height_rejected() {
        let profile_nan_age = DatingProfileInput {
            pubkey: "p1".into(),
            age: Some(f64::NAN),
            height: Some(170.0),
            gender: Some("female".into()),
            seeking: Some("male".into()),
            ..Default::default()
        };
        let input = FilterDatingProfilesInput {
            profiles: vec![profile_nan_age],
            own_gender: Some("male".into()),
            own_seeking: Some("female".into()),
            min_age: Some(18.0),
            max_age: Some(50.0),
            height_min_cm: None,
            height_max_cm: None,
            body_type: None,
            smoking: None,
            drinking: None,
            relationship_intent: None,
            politics: None,
            education: None,
            self_contacts: vec![],
            hide_friends: None,
            own_location_geohash: None,
            own_max_distance_km: None,
        };
        let res = filter_dating_profiles(input);
        assert!(!res[0].passes);
    }

    #[test]
    fn mutual_friends_are_deduplicated() {
        let profile = DatingProfileInput {
            pubkey: "p1".into(),
            verified_mutual_friends: Some(vec![
                "friend_a".into(),
                "friend_a".into(),
                "friend_b".into(),
            ]),
            ..Default::default()
        };
        let input = FilterDatingProfilesInput {
            profiles: vec![profile],
            own_gender: None,
            own_seeking: None,
            min_age: None,
            max_age: None,
            height_min_cm: None,
            height_max_cm: None,
            body_type: None,
            smoking: None,
            drinking: None,
            relationship_intent: None,
            politics: None,
            education: None,
            self_contacts: vec!["friend_a".into(), "friend_b".into()],
            hide_friends: None,
            own_location_geohash: None,
            own_max_distance_km: None,
        };
        let res = filter_dating_profiles(input);
        assert_eq!(res[0].mutual_friends, vec!["friend_a", "friend_b"]);
    }
}
