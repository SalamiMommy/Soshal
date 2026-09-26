//! Geospatial polygon geofencing and geodesic calculations.
//!
//! Provides topological point-in-polygon geofence evaluation, bounding box
//! indexing, and high-precision ellipsoidal geodesic distance computations
//! using `geo`.

use geo::{
    BoundingRect, Contains, Coord, Distance, Geodesic, Haversine, LineString, Point, Polygon,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A geographic geofence polygon defined by (latitude, longitude) vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct GeofencePolygon {
    vertices: Vec<(f64, f64)>,
    polygon: Polygon<f64>,
}

impl GeofencePolygon {
    /// Construct a validated geofence polygon from a list of `(latitude, longitude)` vertices.
    ///
    /// Requires at least 3 distinct vertices within valid geographic bounds
    /// (latitude [-90, 90], longitude [-180, 180]). If the exterior ring is not
    /// closed (first != last), it will be automatically closed.
    pub fn from_lat_lon(vertices: &[(f64, f64)]) -> Result<Self, String> {
        if vertices.len() < 3 {
            return Err("geofence polygon requires at least 3 vertices".to_string());
        }

        for &(lat, lon) in vertices {
            if lat.is_nan() || lon.is_nan() || lat.is_infinite() || lon.is_infinite() {
                return Err(format!("invalid vertex coordinate ({lat}, {lon})"));
            }
            if !(-90.0..=90.0).contains(&lat) {
                return Err(format!("latitude {lat} out of range [-90, 90]"));
            }
            if !(-180.0..=180.0).contains(&lon) {
                return Err(format!("longitude {lon} out of range [-180, 180]"));
            }
        }

        let mut ring = vertices.to_vec();
        let first = ring[0];
        let last = *ring.last().unwrap();
        if (first.0 - last.0).abs() > 1e-9 || (first.1 - last.1).abs() > 1e-9 {
            ring.push(first);
        }

        // Distinct check: at least 4 points in closed ring
        if ring.len() < 4 {
            return Err("geofence polygon requires at least 3 distinct vertices".to_string());
        }

        // In geo, Coord.x is longitude, Coord.y is latitude
        let coords: Vec<Coord<f64>> = ring
            .iter()
            .map(|&(lat, lon)| Coord { x: lon, y: lat })
            .collect();

        let line_string = LineString::new(coords);
        let polygon = Polygon::new(line_string, Vec::new());

        Ok(Self {
            vertices: ring,
            polygon,
        })
    }

    /// Returns `true` if the point `(lat, lon)` is inside or on the boundary of the geofence.
    pub fn contains_point(&self, lat: f64, lon: f64) -> bool {
        if lat.is_nan()
            || lon.is_nan()
            || !(-90.0..=90.0).contains(&lat)
            || !(-180.0..=180.0).contains(&lon)
        {
            return false;
        }
        let pt = Point::new(lon, lat);
        self.polygon.contains(&pt)
    }

    /// Compute the axis-aligned bounding box `(min_lat, min_lon, max_lat, max_lon)`.
    pub fn bounding_box(&self) -> Option<(f64, f64, f64, f64)> {
        self.polygon
            .bounding_rect()
            .map(|rect| (rect.min().y, rect.min().x, rect.max().y, rect.max().x))
    }

    /// Return the vertices defining the exterior boundary ring.
    pub fn vertices(&self) -> &[(f64, f64)] {
        &self.vertices
    }

    /// Number of vertices in the exterior boundary ring (including closure).
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Calculate the high-precision ellipsoidal geodesic distance in meters between two points
    /// using Karney's geodesic algorithms.
    pub fn geodesic_distance_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        if lat1.is_nan() || lon1.is_nan() || lat2.is_nan() || lon2.is_nan() {
            return f64::NAN;
        }
        if !(-90.0..=90.0).contains(&lat1)
            || !(-90.0..=90.0).contains(&lat2)
            || !(-180.0..=180.0).contains(&lon1)
            || !(-180.0..=180.0).contains(&lon2)
        {
            return f64::NAN;
        }
        let p1 = Point::new(lon1, lat1);
        let p2 = Point::new(lon2, lat2);
        Geodesic.distance(p1, p2)
    }

    /// Calculate the fast spherical haversine distance in meters between two points.
    pub fn haversine_distance_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        if lat1.is_nan() || lon1.is_nan() || lat2.is_nan() || lon2.is_nan() {
            return f64::NAN;
        }
        if !(-90.0..=90.0).contains(&lat1)
            || !(-90.0..=90.0).contains(&lat2)
            || !(-180.0..=180.0).contains(&lon1)
            || !(-180.0..=180.0).contains(&lon2)
        {
            return f64::NAN;
        }
        let p1 = Point::new(lon1, lat1);
        let p2 = Point::new(lon2, lat2);
        Haversine.distance(p1, p2)
    }
}

impl Serialize for GeofencePolygon {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.vertices.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GeofencePolygon {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vertices = Vec::<(f64, f64)>::deserialize(deserializer)?;
        Self::from_lat_lon(&vertices).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_in_polygon_containment() {
        // Square centered around (10.0, 20.0) with half-width 2.0
        let coords = vec![(8.0, 18.0), (8.0, 22.0), (12.0, 22.0), (12.0, 18.0)];

        let geofence = GeofencePolygon::from_lat_lon(&coords).unwrap();
        // Auto-closed ring has 5 vertices
        assert_eq!(geofence.vertex_count(), 5);

        // Center point is inside
        assert!(geofence.contains_point(10.0, 20.0));

        // Interior point
        assert!(geofence.contains_point(9.0, 19.0));

        // Outside points
        assert!(!geofence.contains_point(7.0, 20.0));
        assert!(!geofence.contains_point(13.0, 20.0));
        assert!(!geofence.contains_point(10.0, 25.0));

        // Invalid coordinates return false
        assert!(!geofence.contains_point(f64::NAN, 20.0));
        assert!(!geofence.contains_point(95.0, 20.0));
    }

    #[test]
    fn bounding_box_calculation() {
        let coords = vec![(10.0, 20.0), (15.0, 30.0), (5.0, 25.0)];
        let geofence = GeofencePolygon::from_lat_lon(&coords).unwrap();
        let (min_lat, min_lon, max_lat, max_lon) = geofence.bounding_box().unwrap();

        assert_eq!(min_lat, 5.0);
        assert_eq!(max_lat, 15.0);
        assert_eq!(min_lon, 20.0);
        assert_eq!(max_lon, 30.0);
    }

    #[test]
    fn distance_calculations() {
        // Paris (48.8566, 2.3522) to London (51.5074, -0.1278)
        let paris = (48.8566, 2.3522);
        let london = (51.5074, -0.1278);

        let geo_m = GeofencePolygon::geodesic_distance_meters(paris.0, paris.1, london.0, london.1);
        let hav_m =
            GeofencePolygon::haversine_distance_meters(paris.0, paris.1, london.0, london.1);

        // Distance between Paris and London is ~343 km (343,000 meters)
        assert!(
            (340_000.0..=346_000.0).contains(&geo_m),
            "geodesic distance: {geo_m}"
        );
        assert!(
            (340_000.0..=346_000.0).contains(&hav_m),
            "haversine distance: {hav_m}"
        );

        // Difference between Karney geodesic and spherical haversine is small (< 0.5%)
        let diff = (geo_m - hav_m).abs();
        assert!(diff < 2000.0, "difference was {diff}");
    }

    #[test]
    fn serde_roundtrip() {
        let coords = vec![(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)];
        let orig = GeofencePolygon::from_lat_lon(&coords).unwrap();

        let json = serde_json::to_string(&orig).unwrap();
        let restored: GeofencePolygon = serde_json::from_str(&json).unwrap();

        assert_eq!(orig, restored);
        assert!(restored.contains_point(5.0, 5.0));
        assert!(!restored.contains_point(15.0, 5.0));
    }

    #[test]
    fn validation_rejects_invalid_inputs() {
        // Less than 3 vertices
        assert!(GeofencePolygon::from_lat_lon(&[(0.0, 0.0), (1.0, 1.0)]).is_err());

        // NaN
        assert!(GeofencePolygon::from_lat_lon(&[(f64::NAN, 0.0), (1.0, 1.0), (2.0, 2.0)]).is_err());

        // Out of range latitude
        assert!(GeofencePolygon::from_lat_lon(&[(95.0, 0.0), (1.0, 1.0), (2.0, 2.0)]).is_err());

        // Out of range longitude
        assert!(GeofencePolygon::from_lat_lon(&[(0.0, 200.0), (1.0, 1.0), (2.0, 2.0)]).is_err());
    }
}
