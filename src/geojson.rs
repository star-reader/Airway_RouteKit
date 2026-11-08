use crate::error::{Result, RouteKitError};
use crate::geo::{calculate_bearing, calculate_destination, haversine_distance_nm};
use crate::models::{Coordinate, Route};
use serde_json::{json, Value};

fn between(a: &Coordinate, b: &Coordinate, fraction: f64) -> Result<Coordinate> {
    if fraction == 1.0 {
        return Ok(*b);
    }
    calculate_destination(
        a,
        haversine_distance_nm(a, b) * fraction,
        calculate_bearing(a, b),
    )
}
fn unwrapped(lon: f64, origin: f64) -> f64 {
    origin + (lon - origin + 180.0).rem_euclid(360.0) - 180.0
}

/// Enroute geometry only. Densification is a display operation and never changes
/// the searched path or route distance. Crossings are split at +/-180 degrees.
pub fn route_geojson(route: &Route, spacing_nm: f64) -> Result<Value> {
    if !spacing_nm.is_finite() || !(1.0..=600.0).contains(&spacing_nm) || route.segments.is_empty()
    {
        return Err(RouteKitError::ConfigError(
            "GeoJSON needs a route and spacing of 1..=600 NM".into(),
        ));
    }
    let mut dense = vec![route.segments[0].from.coordinate];
    for segment in &route.segments {
        let a = &segment.from.coordinate;
        let b = &segment.to.coordinate;
        Coordinate::new(a.latitude, a.longitude)?;
        Coordinate::new(b.latitude, b.longitude)?;
        if haversine_distance_nm(dense.last().unwrap(), a) > 1e-6 {
            return Err(RouteKitError::InvalidSegment(
                "Disconnected GeoJSON geometry".into(),
            ));
        }
        let distance = haversine_distance_nm(a, b);
        if distance >= 10_800.0 {
            return Err(RouteKitError::InvalidSegment(
                "Antipodal GeoJSON leg has ambiguous geometry".into(),
            ));
        }
        let count = (distance / spacing_nm).ceil().max(1.0) as usize;
        if dense.len() + count > 100_000 {
            return Err(RouteKitError::ConfigError(
                "GeoJSON exceeds 100000 display points".into(),
            ));
        }
        for i in 1..=count {
            dense.push(between(a, b, i as f64 / count as f64)?);
        }
    }
    let mut parts: Vec<Vec<[f64; 2]>> = Vec::new();
    let first = dense[0];
    let mut part = vec![[first.longitude, first.latitude]];
    for pair in dense.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if (b.longitude - a.longitude).abs() > 180.0 {
            let boundary = if a.longitude >= 0.0 { 180.0 } else { -180.0 };
            let east = unwrapped(b.longitude, a.longitude) > a.longitude;
            let mut low = 0.0;
            let mut high = 1.0;
            for _ in 0..48 {
                let mid = (low + high) / 2.0;
                let c = between(a, b, mid)?;
                if (unwrapped(c.longitude, a.longitude) < boundary) == east {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            let c = between(a, b, (low + high) / 2.0)?;
            let end = [boundary, c.latitude];
            if part.last() != Some(&end) {
                part.push(end);
            }
            if part.len() >= 2 {
                parts.push(part);
            }
            part = vec![[-boundary, c.latitude]];
        }
        let end = [b.longitude, b.latitude];
        if part.last() != Some(&end) {
            part.push(end);
        }
    }
    if part.len() >= 2 {
        parts.push(part);
    }
    let geometry = if parts.len() == 1 {
        json!({"type":"LineString", "coordinates":parts[0]})
    } else {
        json!({"type":"MultiLineString", "coordinates":parts})
    };
    Ok(json!({"type":"Feature", "properties": {
        "route":route.to_route_string(), "distance_nm":route.total_distance_nm, "scope":"enroute"
    }, "geometry":geometry}))
}
