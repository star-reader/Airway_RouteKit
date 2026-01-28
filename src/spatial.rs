use crate::models::{Coordinate, Waypoint};
use rstar::{RTree, RTreeObject, AABB};

/// 空间索引项
#[derive(Debug, Clone)]
struct SpatialWaypoint {
    waypoint: Waypoint,
}

impl RTreeObject for SpatialWaypoint {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        let point = [
            self.waypoint.coordinate.longitude,
            self.waypoint.coordinate.latitude,
        ];
        AABB::from_point(point)
    }
}

/// 空间索引（基于R-tree）
pub struct SpatialIndex {
    rtree: RTree<SpatialWaypoint>,
}

impl SpatialIndex {
    /// 创建新的空间索引
    pub fn new() -> Self {
        Self {
            rtree: RTree::new(),
        }
    }

    /// 批量插入航点
    pub fn bulk_insert(&mut self, waypoints: Vec<Waypoint>) {
        let spatial_waypoints: Vec<SpatialWaypoint> = waypoints
            .into_iter()
            .map(|waypoint| SpatialWaypoint { waypoint })
            .collect();

        self.rtree = RTree::bulk_load(spatial_waypoints);
    }

    /// 插入单个航点
    pub fn insert(&mut self, waypoint: Waypoint) {
        self.rtree.insert(SpatialWaypoint { waypoint });
    }

    /// 查找最近的航点
    pub fn find_nearest(&self, coord: &Coordinate) -> Option<Waypoint> {
        let point = [coord.longitude, coord.latitude];
        self.rtree
            .nearest_neighbor(&point)
            .map(|sp| sp.waypoint.clone())
    }

    /// 查找指定半径内的所有航点
    /// radius_nm: 搜索半径（海里）
    pub fn find_within_radius(
        &self,
        coord: &Coordinate,
        radius_nm: f64,
    ) -> Vec<Waypoint> {
        use crate::geo::haversine_distance_nm;

        // 将海里转换为大约的经纬度偏移
        // 1海里 ≈ 1/60度纬度
        let delta = radius_nm / 60.0;

        let min_point = [coord.longitude - delta, coord.latitude - delta];
        let max_point = [coord.longitude + delta, coord.latitude + delta];

        let envelope = AABB::from_corners(min_point, max_point);

        // 获取边界框内的所有点
        let candidates: Vec<Waypoint> = self
            .rtree
            .locate_in_envelope(&envelope)
            .map(|sp| sp.waypoint.clone())
            .collect();

        // 过滤出真正在半径内的点（使用精确的大圆距离）
        candidates
            .into_iter()
            .filter(|wp| {
                haversine_distance_nm(coord, &wp.coordinate) <= radius_nm
            })
            .collect()
    }

    /// 查找k个最近的航点
    pub fn find_k_nearest(&self, coord: &Coordinate, k: usize) -> Vec<Waypoint> {
        use crate::geo::haversine_distance_nm;

        let point = [coord.longitude, coord.latitude];

        self.rtree
            .nearest_neighbor_iter(&point)
            .take(k)
            .map(|sp| sp.waypoint.clone())
            .collect()
    }

    /// 查找指定标识符的航点
    pub fn find_by_identifier(&self, identifier: &str) -> Vec<Waypoint> {
        self.rtree
            .iter()
            .filter(|sp| sp.waypoint.identifier == identifier)
            .map(|sp| sp.waypoint.clone())
            .collect()
    }

    /// 获取索引中的航点数量
    pub fn size(&self) -> usize {
        self.rtree.size()
    }

    /// 清空索引
    pub fn clear(&mut self) {
        self.rtree = RTree::new();
    }
}

impl Default for SpatialIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WaypointType;

    fn create_test_waypoint(id: &str, lat: f64, lon: f64) -> Waypoint {
        Waypoint {
            identifier: id.to_string(),
            icao_code: "ZB".to_string(),
            name: Some(id.to_string()),
            coordinate: Coordinate::new(lat, lon).unwrap(),
            waypoint_type: WaypointType::Enroute,
            usage: None,
            id: None,
        }
    }

    #[test]
    fn test_spatial_index_insert_and_search() {
        let mut index = SpatialIndex::new();

        let waypoints = vec![
            create_test_waypoint("WP1", 40.0, 116.0),
            create_test_waypoint("WP2", 40.5, 116.5),
            create_test_waypoint("WP3", 41.0, 117.0),
        ];

        index.bulk_insert(waypoints);

        assert_eq!(index.size(), 3);

        let center = Coordinate::new(40.0, 116.0).unwrap();
        let nearest = index.find_nearest(&center);
        assert!(nearest.is_some());
        assert_eq!(nearest.unwrap().identifier, "WP1");
    }

    #[test]
    fn test_find_within_radius() {
        let mut index = SpatialIndex::new();

        let waypoints = vec![
            create_test_waypoint("NEAR", 40.0, 116.0),
            create_test_waypoint("FAR", 50.0, 120.0),
        ];

        index.bulk_insert(waypoints);

        let center = Coordinate::new(40.0, 116.0).unwrap();
        let nearby = index.find_within_radius(&center, 100.0);

        assert_eq!(nearby.len(), 1);
        assert_eq!(nearby[0].identifier, "NEAR");
    }

    #[test]
    fn test_find_k_nearest() {
        let mut index = SpatialIndex::new();

        let waypoints = vec![
            create_test_waypoint("WP1", 40.0, 116.0),
            create_test_waypoint("WP2", 40.1, 116.1),
            create_test_waypoint("WP3", 40.2, 116.2),
            create_test_waypoint("WP4", 40.3, 116.3),
        ];

        index.bulk_insert(waypoints);

        let center = Coordinate::new(40.0, 116.0).unwrap();
        let nearest_3 = index.find_k_nearest(&center, 3);

        assert_eq!(nearest_3.len(), 3);
        assert_eq!(nearest_3[0].identifier, "WP1");
    }

    #[test]
    fn test_find_by_identifier() {
        let mut index = SpatialIndex::new();

        let waypoints = vec![
            create_test_waypoint("TEPID", 40.0, 116.0),
            create_test_waypoint("VYK", 30.0, 120.0),
        ];

        index.bulk_insert(waypoints);

        let found = index.find_by_identifier("TEPID");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].identifier, "TEPID");
    }
}
