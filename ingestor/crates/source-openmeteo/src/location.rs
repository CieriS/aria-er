use std::collections::BTreeMap;

/// A point weather is requested for.
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    /// `<lat>_<lon>` of the coordinates scaled by `10^decimals` and rounded,
    /// e.g. `445_114` for 44.5 N 11.4 E at one decimal.
    pub id: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Collapses coordinates that round to the same point at `decimals` decimals.
///
/// One decimal is about 11 km in latitude, the order of the archive grid:
/// stations of the same city share one location. Output is sorted by id.
pub fn dedup_locations(
    coordinates: impl IntoIterator<Item = (f64, f64)>,
    decimals: u32,
) -> Vec<Location> {
    let scale = 10f64.powi(decimals.min(6) as i32);
    let mut locations = BTreeMap::new();
    for (latitude, longitude) in coordinates {
        if !latitude.is_finite() || !longitude.is_finite() {
            continue;
        }
        let (lat_index, lon_index) = (
            (latitude * scale).round() as i64,
            (longitude * scale).round() as i64,
        );
        let id = format!("{lat_index}_{lon_index}");
        locations.entry(id.clone()).or_insert(Location {
            id,
            latitude: lat_index as f64 / scale,
            longitude: lon_index as f64 / scale,
        });
    }
    locations.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearby_stations_collapse_to_one_location() {
        // Two points 3 km apart in Bologna and one in Parma.
        let coordinates = [(44.4827, 11.3541), (44.4712, 11.3893), (44.7937, 10.3306)];
        let locations = dedup_locations(coordinates, 1);
        let ids: Vec<_> = locations.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["445_114", "448_103"]);
        assert_eq!(
            (locations[0].latitude, locations[0].longitude),
            (44.5, 11.4)
        );
    }

    #[test]
    fn more_decimals_keep_stations_apart() {
        let coordinates = [(44.4827, 11.3541), (44.4712, 11.3893)];
        assert_eq!(dedup_locations(coordinates, 2).len(), 2);
    }

    #[test]
    fn non_finite_coordinates_are_ignored() {
        assert!(dedup_locations([(f64::NAN, 11.0)], 1).is_empty());
    }
}
