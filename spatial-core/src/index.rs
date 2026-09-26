use crate::distance::haversine_km;
use rstar::primitives::GeomWithData;
use rstar::{RTree, AABB};

/// A spatial indexed point holding an associated data payload of type `T`.
pub type SpatialElement<T> = GeomWithData<[f64; 2], T>;

/// Fast $O(\log N)$ R*-tree spatial index for points on Earth.
/// Coordinates are stored as `[latitude, longitude]`.
pub struct SpatialIndex<T> {
    tree: RTree<SpatialElement<T>>,
}

impl<T: Clone + PartialEq> Default for SpatialIndex<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq> SpatialIndex<T> {
    /// Create a new, empty spatial index.
    pub fn new() -> Self {
        Self { tree: RTree::new() }
    }

    /// Insert an item at a specific (latitude, longitude).
    /// Returns an error if coordinates are non-finite or out of range.
    pub fn insert(&mut self, data: T, lat: f64, lon: f64) -> Result<(), &'static str> {
        if lat.is_nan() || lon.is_nan() || lat.is_infinite() || lon.is_infinite() {
            return Err("Coordinates must be finite numbers");
        }
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return Err("Coordinates out of range (-90..=90 lat, -180..=180 lon)");
        }
        self.tree.insert(GeomWithData::new([lat, lon], data));
        Ok(())
    }

    /// Returns the number of elements in the index.
    pub fn len(&self) -> usize {
        self.tree.size()
    }

    /// Returns whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    /// Find the nearest neighbor to `(lat, lon)`.
    /// Returns `(data, distance_in_km)`.
    pub fn nearest_neighbor(&self, lat: f64, lon: f64) -> Option<(T, f64)> {
        if lat.is_nan() || lon.is_nan() || lat.is_infinite() || lon.is_infinite() {
            return None;
        }
        let query_point = [lat, lon];
        let elem = self.tree.nearest_neighbor(query_point)?;
        let dist = haversine_km(lat, lon, elem.geom()[0], elem.geom()[1]);
        Some((elem.data.clone(), dist))
    }

    /// Find all items situated within `radius_km` of `(lat, lon)`.
    /// Uses bounding-box envelope pruning before exact Haversine calculation.
    pub fn locate_within_distance(&self, lat: f64, lon: f64, radius_km: f64) -> Vec<(T, f64)> {
        if lat.is_nan() || lon.is_nan() || radius_km.is_nan() || radius_km < 0.0 {
            return Vec::new();
        }

        // Approximate 1 deg lat ~ 111.0 km, 1 deg lon ~ 111.0 * cos(lat) km
        let lat_delta = radius_km / 111.0;
        let cos_lat = lat.to_radians().cos().abs().max(0.01);
        let lon_delta = radius_km / (111.0 * cos_lat);

        let min_corner = [(lat - lat_delta).max(-90.0), (lon - lon_delta).max(-180.0)];
        let max_corner = [(lat + lat_delta).min(90.0), (lon + lon_delta).min(180.0)];

        let envelope = AABB::from_corners(min_corner, max_corner);
        let candidates = self.tree.locate_in_envelope(envelope);

        let mut results = Vec::new();
        for elem in candidates {
            let item_lat = elem.geom()[0];
            let item_lon = elem.geom()[1];
            let dist = haversine_km(lat, lon, item_lat, item_lon);
            if dist <= radius_km {
                results.push((elem.data.clone(), dist));
            }
        }
        // Sort ascending by distance
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Find all items inside an axis-aligned bounding box.
    pub fn locate_in_bounding_box(
        &self,
        min_lat: f64,
        min_lon: f64,
        max_lat: f64,
        max_lon: f64,
    ) -> Vec<T> {
        if min_lat > max_lat || min_lon > max_lon {
            return Vec::new();
        }
        let envelope = AABB::from_corners([min_lat, min_lon], [max_lat, max_lon]);
        self.tree
            .locate_in_envelope(envelope)
            .map(|elem| elem.data.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spatial_index_insert_and_len() {
        let mut index = SpatialIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);

        index.insert("Paris", 48.8566, 2.3522).unwrap();
        index.insert("London", 51.5074, -0.1278).unwrap();
        index.insert("Berlin", 52.5200, 13.4050).unwrap();

        assert_eq!(index.len(), 3);
        assert!(!index.is_empty());

        // Reject invalid coords
        assert!(index.insert("InvalidLat", 95.0, 0.0).is_err());
        assert!(index.insert("InvalidLon", 0.0, 200.0).is_err());
        assert!(index.insert("NaN", f64::NAN, 0.0).is_err());
    }

    #[test]
    fn test_nearest_neighbor() {
        let mut index = SpatialIndex::new();
        index.insert("Paris", 48.8566, 2.3522).unwrap();
        index.insert("London", 51.5074, -0.1278).unwrap();
        index.insert("Tokyo", 35.6762, 139.6503).unwrap();

        // Point near London (Cambridge)
        let (city, dist) = index.nearest_neighbor(52.2053, 0.1218).unwrap();
        assert_eq!(city, "London");
        assert!(dist < 100.0);
    }

    #[test]
    fn test_locate_within_distance() {
        let mut index = SpatialIndex::new();
        index.insert("Paris", 48.8566, 2.3522).unwrap();
        index.insert("Versailles", 48.8049, 2.1204).unwrap();
        index.insert("Berlin", 52.5200, 13.4050).unwrap();

        // Within 30 km of Paris center: should find Paris and Versailles (~17km), not Berlin
        let nearby = index.locate_within_distance(48.8566, 2.3522, 30.0);
        assert_eq!(nearby.len(), 2);
        assert_eq!(nearby[0].0, "Paris");
        assert_eq!(nearby[1].0, "Versailles");
    }

    #[test]
    fn test_locate_in_bounding_box() {
        let mut index = SpatialIndex::new();
        index.insert("Rome", 41.9028, 12.4964).unwrap();
        index.insert("Milan", 45.4642, 9.1900).unwrap();
        index.insert("Madrid", 40.4168, -3.7038).unwrap();

        // Italy box: lat 36..47, lon 6..19
        let in_italy = index.locate_in_bounding_box(36.0, 6.0, 47.0, 19.0);
        assert_eq!(in_italy.len(), 2);
        assert!(in_italy.contains(&"Rome"));
        assert!(in_italy.contains(&"Milan"));
        assert!(!in_italy.contains(&"Madrid"));
    }
}
