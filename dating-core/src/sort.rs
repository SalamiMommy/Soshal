//! Dating profile sorting by compatibility score.

use crate::scoring::compute_mutual_score;
#[doc(hidden)]
pub use crate::MAX_PROFILES;
use crate::{SortProfilesInput, SortedProfileOut};
use soshal_spatial_core::distance::{decode_geohash_coords, haversine_km};
use std::cmp::Ordering;
use std::collections::HashSet;

pub fn sort_dating_profiles(input: SortProfilesInput) -> Vec<SortedProfileOut> {
    if input.profiles.len() > MAX_PROFILES {
        return Vec::new();
    }
    let self_contacts_set: HashSet<&str> = input.self_contacts.iter().map(|s| s.as_str()).collect();
    let self_coords = input
        .self_profile
        .location_geohash
        .as_deref()
        .and_then(decode_geohash_coords);

    // (result, age, height, distance_km)
    let mut results: Vec<(SortedProfileOut, Option<f64>, Option<f64>, f64)> =
        Vec::with_capacity(input.profiles.len().min(MAX_PROFILES));
    for profile in &input.profiles {
        let is_contact = self_contacts_set.contains(profile.pubkey.as_str());
        let mutual_friends: Vec<String> = profile
            .verified_mutual_friends
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|f| self_contacts_set.contains(f.as_str()))
            .cloned()
            .collect();

        let score = compute_mutual_score(&input.self_profile, profile);
        let other_coords = profile
            .location_geohash
            .as_deref()
            .and_then(decode_geohash_coords);
        let distance_km = match (self_coords, other_coords) {
            (Some((lat1, lon1)), Some((lat2, lon2))) => haversine_km(lat1, lon1, lat2, lon2),
            _ => f64::from(if is_contact { 1 } else { 2 }),
        };
        results.push((
            SortedProfileOut {
                event_id: profile.event_id.clone(),
                pubkey: profile.pubkey.clone(),
                compatibility_score: score,
                mutual_friends,
                distance: if is_contact { 1 } else { 2 },
                liked_by_me: profile.liked_by_me.unwrap_or(false),
                liked_me: profile.liked_me.unwrap_or(false),
                liker_total_likes: profile.liker_total_likes.unwrap_or(u32::MAX),
            },
            profile.age,
            profile.height,
            distance_km,
        ));
    }
    match input.sort_by.as_deref() {
        Some("age") => results.sort_by(|a, b| match (a.1, b.1) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (None, None) => Ordering::Equal,
        }),
        Some("height") => results.sort_by(|a, b| match (a.2, b.2) {
            (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(Ordering::Equal),
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (None, None) => Ordering::Equal,
        }),
        Some("distance") => {
            results.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(Ordering::Equal))
        }
        _ => results.sort_by(|a, b| {
            let cmp = b.0.compatibility_score.cmp(&a.0.compatibility_score);
            if cmp != Ordering::Equal {
                return cmp;
            }
            let cmp = b.0.liked_me.cmp(&a.0.liked_me);
            if cmp != Ordering::Equal {
                return cmp;
            }
            let cmp = a.0.liker_total_likes.cmp(&b.0.liker_total_likes);
            if cmp != Ordering::Equal {
                return cmp;
            }
            a.0.distance.cmp(&b.0.distance)
        }),
    }
    results.into_iter().map(|(r, _, _, _)| r).collect()
}
