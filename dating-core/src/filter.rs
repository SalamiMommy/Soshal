//! Dating profile filtering by gender, seeking, and distance.

use crate::{FilterDatingProfilesInput, FilteredDatingProfileOut, MAX_PROFILES};
use std::collections::HashSet;

pub fn filter_dating_profiles(input: FilterDatingProfilesInput) -> Vec<FilteredDatingProfileOut> {
    if input.profiles.len() > MAX_PROFILES {
        return Vec::new();
    }
    let gender_map: &[&str] = match input.own_seeking.as_deref() {
        Some("male") => &["male"],
        Some("female") => &["female"],
        Some("non-binary") => &["non-binary"],
        Some("other") => &["other"],
        _ => &["male", "female", "non-binary", "other"],
    };
    let self_contacts_set: HashSet<&str> = input.self_contacts.iter().map(|s| s.as_str()).collect();
    let own_coords = input
        .own_location_geohash
        .as_deref()
        .and_then(soshal_spatial_core::distance::decode_geohash_coords);
    input
        .profiles
        .iter()
        .enumerate()
        .map(|(i, profile)| {
            let is_contact = self_contacts_set.contains(profile.pubkey.as_str());
            let passes = (|| -> bool {
                if input.hide_friends.unwrap_or(false) && is_contact {
                    return false;
                }
                if let Some(ref seeking) = input.own_seeking {
                    if !seeking.eq_ignore_ascii_case("all") {
                        match profile.gender.as_ref() {
                            Some(g) => {
                                if !gender_map.contains(&g.as_str()) {
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
                    if !other_map.contains(&own_g.as_str()) {
                        return false;
                    }
                }
                if input.min_age.is_some() || input.max_age.is_some() {
                    match profile.age {
                        Some(age) => {
                            if let Some(min) = input.min_age {
                                if age < min {
                                    return false;
                                }
                            }
                            if let Some(max) = input.max_age {
                                if age > max {
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
                            if let Some(min) = input.height_min_cm {
                                if height < min {
                                    return false;
                                }
                            }
                            if let Some(max) = input.height_max_cm {
                                if height > max {
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
                            if own_max > 0.0 {
                                match dist {
                                    Some(d) if d <= own_max => {}
                                    _ => return false,
                                }
                            }
                        }

                        if let Some(other_max) = profile.max_distance_km {
                            if other_max > 0.0 {
                                match dist {
                                    Some(d) if d <= other_max => {}
                                    _ => return false,
                                }
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
                        friends
                            .iter()
                            .filter(|f| self_contacts_set.contains(f.as_str()))
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
