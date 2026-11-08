use crate::geo::{haversine_distance_nm, EARTH_RADIUS_NM};
use crate::models::{Coordinate, Waypoint};
use rstar::{PointDistance, RTree, RTreeObject, AABB};

fn xyz(c: &Coordinate) -> [f64; 3] {
    let (lat, lon) = (c.latitude.to_radians(), c.longitude.to_radians());
    [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()]
}

struct Entry {
    point: [f64; 3],
    index: usize,
}
impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 3]>;
    fn envelope(&self) -> Self::Envelope {
        AABB::from_point(self.point)
    }
}
impl PointDistance for Entry {
    fn distance_2(&self, p: &[f64; 3]) -> f64 {
        (0..3).map(|i| (self.point[i] - p[i]).powi(2)).sum()
    }
}

#[derive(Default)]
pub struct SpatialIndex {
    tree: RTree<Entry>,
    waypoints: Vec<Waypoint>,
}
impl SpatialIndex {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn bulk_insert(&mut self, waypoints: Vec<Waypoint>) {
        self.tree = RTree::bulk_load(
            waypoints
                .iter()
                .enumerate()
                .map(|(index, waypoint)| Entry {
                    point: xyz(&waypoint.coordinate),
                    index,
                })
                .collect(),
        );
        self.waypoints = waypoints;
    }
    pub fn insert(&mut self, waypoint: Waypoint) {
        self.tree.insert(Entry {
            point: xyz(&waypoint.coordinate),
            index: self.waypoints.len(),
        });
        self.waypoints.push(waypoint);
    }
    pub fn find_nearest(&self, coord: &Coordinate) -> Option<Waypoint> {
        self.tree
            .nearest_neighbor(&xyz(coord))
            .map(|e| self.waypoints[e.index].clone())
    }
    pub fn find_within_radius(&self, coord: &Coordinate, radius_nm: f64) -> Vec<Waypoint> {
        if !radius_nm.is_finite() || radius_nm < 0.0 {
            return Vec::new();
        }
        let chord = 2.0
            * (radius_nm / EARTH_RADIUS_NM / 2.0)
                .min(std::f64::consts::FRAC_PI_2)
                .sin();
        let mut points: Vec<_> = self
            .tree
            .locate_within_distance(xyz(coord), chord * chord + 1e-14)
            .filter_map(|e| {
                let point = &self.waypoints[e.index];
                let d = haversine_distance_nm(coord, &point.coordinate);
                (d <= radius_nm + 1e-9).then_some((d, point))
            })
            .collect();
        points.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.identifier.cmp(&b.1.identifier))
        });
        points.into_iter().map(|(_, p)| p.clone()).collect()
    }
    pub fn find_k_nearest(&self, coord: &Coordinate, k: usize) -> Vec<Waypoint> {
        self.tree
            .nearest_neighbor_iter(&xyz(coord))
            .take(k)
            .map(|e| self.waypoints[e.index].clone())
            .collect()
    }
    pub fn find_by_identifier(&self, identifier: &str) -> Vec<Waypoint> {
        self.waypoints
            .iter()
            .filter(|p| p.identifier.eq_ignore_ascii_case(identifier))
            .cloned()
            .collect()
    }
    pub fn size(&self) -> usize {
        self.tree.size()
    }
    pub fn clear(&mut self) {
        self.tree = RTree::new();
        self.waypoints.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_indices_survive_insert_replace_and_clear() {
        let mut index = SpatialIndex::new();
        index.insert(Waypoint::simple("OLD", 0.0, 0.0).unwrap());
        index.bulk_insert(vec![Waypoint::simple("NEW", 1.0, 1.0).unwrap()]);
        index.insert(Waypoint::simple("NEXT", 2.0, 2.0).unwrap());
        assert_eq!(index.size(), 2);
        assert!(index.find_by_identifier("OLD").is_empty());
        let nearest = index.find_k_nearest(&Coordinate::new(2.0, 2.0).unwrap(), 2);
        assert_eq!(nearest[0].identifier, "NEXT");
        assert_eq!(nearest[1].identifier, "NEW");
        index.clear();
        assert_eq!(index.size(), 0);
        assert!(index
            .find_nearest(&Coordinate::new(0.0, 0.0).unwrap())
            .is_none());
        index.insert(Waypoint::simple("RESET", 0.0, 0.0).unwrap());
        assert_eq!(index.find_by_identifier("reset")[0].identifier, "RESET");
    }
    #[test]
    fn high_latitude_and_dateline() {
        let mut index = SpatialIndex::new();
        index.bulk_insert(vec![
            Waypoint::simple("POLAR", 60.0, 1.5).unwrap(),
            Waypoint::simple("DATE", 0.0, -179.9).unwrap(),
        ]);
        assert_eq!(
            index.find_within_radius(&Coordinate::new(60.0, 0.0).unwrap(), 60.0)[0].identifier,
            "POLAR"
        );
        assert_eq!(
            index.find_within_radius(&Coordinate::new(0.0, 179.9).unwrap(), 20.0)[0].identifier,
            "DATE"
        );
    }
}
