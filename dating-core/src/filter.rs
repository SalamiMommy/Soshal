//! Dating profile filtering by gender, seeking, and distance.

use crate::{FilterDatingProfilesInput, FilteredDatingProfileOut, MAX_PROFILES};
use soshal_spatial_core::distance::haversine_distance;
use std::collections::HashSet;

pub fn filter_dating_profiles(input: FilterDatingProfilesInput) -> Vec<FilteredDatingProfileOut> {
    if input.profiles.len() > MAX_PROFILES {
        return Vec::new();
    }
    let gender_map: Vec<&str> = match input.own_seeking.as_deref() {
        Some("male") => vec!["male"],
        Some("female") => vec!["female"],
        Some("non-binary") => vec!["non-binary"],
        Some("other") => vec!["other"],
        _ => vec!["male", "female", "non-binary", "other"],
    };
    let self_contacts_set: HashSet<&str> = input.self_contacts.iter().map(|s| s.as_str()).collect();
    let own_gender_lower = input.own_gender.as_ref().map(|g| g.to_lowercase());
    input
        .profiles
        .iter()
        .enumerate()
        .map(|(i, profile)| {
            let is_contact = self_contacts_set.contains(profile.pubkey.as_str());
            let mutual_friends: Vec<String> = profile
                .verified_mutual_friends
                .clone()
                .unwrap_or_default()
                .into_iter()
                .filter(|f| self_contacts_set.contains(f.as_str()))
                .collect();

            let passes = (|| -> bool {
                if input.hide_friends.unwrap_or(false)
                    && self_contacts_set.contains(profile.pubkey.as_str())
                {
                    return false;
                }
                if let Some(ref seeking) = input.own_seeking {
                    if seeking != "All" {
                        if let Some(ref other_gender) = profile.gender {
                            if !gender_map.contains(&other_gender.as_str()) {
                                return false;
                            }
                        }
                    }
                }
                if let Some(ref other_seeking) = profile.seeking {
                    if !other_seeking.eq_ignore_ascii_case("All") {
                        if let Some(ref own_g) = own_gender_lower {
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
                    }
                }
                if let (Some(own_gh), Some(ref own_max)) = (
                    input.own_location_geohash.as_ref(),
                    input.own_max_distance_km,
                ) {
                    if *own_max > 0.0 {
                        if let Some(ref other_gh) = profile.location_geohash {
                            if haversine_distance(own_gh, other_gh) > *own_max {
                                return false;
                            }
                        }
                    }
                }
                if let (Some(other_gh), Some(ref other_max)) =
                    (profile.location_geohash.as_ref(), profile.max_distance_km)
                {
                    if *other_max > 0.0 {
                        if let Some(ref own_gh) = input.own_location_geohash {
                            if haversine_distance(other_gh, own_gh) > *other_max {
                                return false;
                            }
                        }
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
                true
            })();
            FilteredDatingProfileOut {
                index: i,
                passes,
                is_contact,
                mutual_friends,
            }
        })
        .collect()
}
