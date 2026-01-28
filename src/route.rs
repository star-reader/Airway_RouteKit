use crate::database::DatabasePool;
use crate::error::{Result, RouteKitError};
use crate::geo::haversine_distance_nm;
use crate::models::*;
use crate::spatial::SpatialIndex;
use parking_lot::RwLock;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::Arc;

/// 航路搜索器
pub struct RouteSearcher {
    db_pool: Arc<DatabasePool>,
    spatial_index: Arc<RwLock<SpatialIndex>>,
    config: crate::config::Config,
}

impl RouteSearcher {
    pub fn new(
        db_pool: Arc<DatabasePool>,
        spatial_index: Arc<RwLock<SpatialIndex>>,
        config: crate::config::Config,
    ) -> Self {
        Self {
            db_pool,
            spatial_index,
            config,
        }
    }

    /// 搜索航路
    pub fn search_routes(&self, request: &RouteRequest) -> Result<Vec<Route>> {
        // 加载起飞和目的机场
        let departure = self.db_pool.load_airport(&request.departure_icao)?;
        let destination = self.db_pool.load_airport(&request.destination_icao)?;

        // 查找SID和STAR
        let sids = self.db_pool.find_sids(&request.departure_icao)?;
        let stars = self.db_pool.find_stars(&request.destination_icao)?;

        // 执行A*搜索
        let mut routes = Vec::new();

        // 尝试不同的SID/STAR组合
        for sid in sids.iter().take(3) {
            for star in stars.iter().take(3) {
                if let Some(route) = self.search_route_with_procedures(
                    &departure,
                    &destination,
                    Some(sid),
                    Some(star),
                    request,
                )? {
                    routes.push(route);
                    if routes.len() >= request.max_routes {
                        return Ok(routes);
                    }
                }
            }
        }

        // 如果没有找到足够的航路，尝试不使用程序
        if routes.is_empty() {
            if let Some(route) = self.search_route_with_procedures(
                &departure,
                &destination,
                None,
                None,
                request,
            )? {
                routes.push(route);
            }
        }

        if routes.is_empty() {
            return Err(RouteKitError::RouteNotFound {
                from: request.departure_icao.clone(),
                to: request.destination_icao.clone(),
            });
        }

        Ok(routes)
    }

    /// 使用指定的SID/STAR搜索航路
    fn search_route_with_procedures(
        &self,
        departure: &Airport,
        destination: &Airport,
        sid: Option<&Sid>,
        star: Option<&Star>,
        request: &RouteRequest,
    ) -> Result<Option<Route>> {
        // 确定起始和结束航点
        let start_waypoint = if let Some(sid) = sid {
            if let Some(last) = sid.waypoints.last() {
                self.find_or_create_waypoint(&last.waypoint_identifier, &last.coordinate)?
            } else {
                self.find_nearest_waypoint(&departure.coordinate)?
            }
        } else {
            self.find_nearest_waypoint(&departure.coordinate)?
        };

        let end_waypoint = if let Some(star) = star {
            if let Some(first) = star.waypoints.first() {
                self.find_or_create_waypoint(&first.waypoint_identifier, &first.coordinate)?
            } else {
                self.find_nearest_waypoint(&destination.coordinate)?
            }
        } else {
            self.find_nearest_waypoint(&destination.coordinate)?
        };

        // 使用A*算法搜索航路
        let segments = self.a_star_search(&start_waypoint, &end_waypoint, request)?;

        if segments.is_empty() {
            return Ok(None);
        }

        let total_distance_nm = segments.iter().map(|s| s.distance_nm).sum();

        Ok(Some(Route {
            departure: departure.clone(),
            destination: destination.clone(),
            sid: sid.cloned(),
            star: star.cloned(),
            segments,
            total_distance_nm,
            estimated_time_minutes: Some(self.estimate_flight_time(total_distance_nm)),
        }))
    }

    /// A*搜索算法
    fn a_star_search(
        &self,
        start: &Waypoint,
        goal: &Waypoint,
        request: &RouteRequest,
    ) -> Result<Vec<RouteSegment>> {
        self.a_star_search_internal(start, goal, request)
    }

    /// A*搜索内部实现
    fn a_star_search_internal(
        &self,
        start: &Waypoint,
        goal: &Waypoint,
        request: &RouteRequest,
    ) -> Result<Vec<RouteSegment>> {
        #[derive(Clone)]
        struct Node {
            waypoint: Waypoint,
            g_cost: f64,
            f_cost: f64,
            parent: Option<Box<Node>>,
            airway: Option<String>,
        }

        impl PartialEq for Node {
            fn eq(&self, other: &Self) -> bool {
                self.waypoint.identifier == other.waypoint.identifier
            }
        }

        impl Eq for Node {}

        impl Ord for Node {
            fn cmp(&self, other: &Self) -> Ordering {
                other.f_cost.partial_cmp(&self.f_cost).unwrap_or(Ordering::Equal)
            }
        }

        impl PartialOrd for Node {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        let mut open_set = BinaryHeap::new();
        let mut closed_set = HashSet::new();
        let mut best_g_scores: HashMap<String, f64> = HashMap::new();

        let h_start = haversine_distance_nm(&start.coordinate, &goal.coordinate);
        open_set.push(Node {
            waypoint: start.clone(),
            g_cost: 0.0,
            f_cost: h_start,
            parent: None,
            airway: None,
        });
        best_g_scores.insert(start.identifier.clone(), 0.0);

        let mut iterations = 0;
        let max_iterations = self.config.max_search_depth;

        while let Some(current) = open_set.pop() {
            iterations += 1;
            if iterations > max_iterations {
                break;
            }

            // 到达目标
            if current.waypoint.identifier == goal.identifier {
                // 重建路径
                let mut segments = Vec::new();
                let mut current_node = Some(&current);

                while let Some(node) = current_node {
                    if let Some(parent) = &node.parent {
                        let distance = haversine_distance_nm(
                            &parent.waypoint.coordinate,
                            &node.waypoint.coordinate,
                        );
                        let bearing = crate::geo::calculate_bearing(
                            &parent.waypoint.coordinate,
                            &node.waypoint.coordinate,
                        );

                        segments.push(RouteSegment {
                            from: parent.waypoint.clone(),
                            to: node.waypoint.clone(),
                            airway: node.airway.clone(),
                            distance_nm: distance,
                            magnetic_course: bearing,
                            minimum_altitude: None,
                            maximum_altitude: None,
                        });

                        current_node = Some(parent.as_ref());
                    } else {
                        break;
                    }
                }

                segments.reverse();
                return Ok(segments);
            }

            if closed_set.contains(&current.waypoint.identifier) {
                continue;
            }

            closed_set.insert(current.waypoint.identifier.clone());

            // 查找邻居航点
            let neighbors = self.find_neighbor_waypoints(&current.waypoint, request)?;

            for (neighbor, airway) in neighbors {
                if closed_set.contains(&neighbor.identifier) {
                    continue;
                }

                let distance = haversine_distance_nm(
                    &current.waypoint.coordinate,
                    &neighbor.coordinate,
                );

                // 计算代价（根据偏好调整）
                let edge_cost = self.calculate_edge_cost(distance, &airway, request);
                let tentative_g = current.g_cost + edge_cost;

                if let Some(&best_g) = best_g_scores.get(&neighbor.identifier) {
                    if tentative_g >= best_g {
                        continue;
                    }
                }

                best_g_scores.insert(neighbor.identifier.clone(), tentative_g);

                let h = haversine_distance_nm(&neighbor.coordinate, &goal.coordinate);
                let f = tentative_g + h;

                open_set.push(Node {
                    waypoint: neighbor,
                    g_cost: tentative_g,
                    f_cost: f,
                    parent: Some(Box::new(current.clone())),
                    airway,
                });
            }
        }

        // 未找到航路，返回空
        Ok(Vec::new())
    }

    /// 查找邻居航点
    fn find_neighbor_waypoints(
        &self,
        waypoint: &Waypoint,
        request: &RouteRequest,
    ) -> Result<Vec<(Waypoint, Option<String>)>> {
        let mut neighbors = Vec::new();

        // 1. 通过航路查找
        if matches!(
            request.route_preference,
            RoutePreference::AirwayPreferred | RoutePreference::Balanced
        ) {
            // 查找该航点所在的所有航路
            if let Ok(segments) = self.find_airway_connections(waypoint) {
                for seg in segments {
                    neighbors.push((seg.waypoint, Some(seg.route_identifier)));
                }
            }
        }

        // 2. 直飞附近航点
        if matches!(
            request.route_preference,
            RoutePreference::DirectPreferred | RoutePreference::Balanced
        ) {
            let spatial_index = self.spatial_index.read();
            let nearby = spatial_index.find_within_radius(
                &waypoint.coordinate,
                self.config.spatial_search_radius_nm,
            );

            for wp in nearby {
                if wp.identifier != waypoint.identifier {
                    neighbors.push((wp, None));
                }
            }
        }

        Ok(neighbors)
    }

    /// 查找航路连接
    fn find_airway_connections(&self, _waypoint: &Waypoint) -> Result<Vec<AirwaySegment>> {
        // 简化实现：在实际数据库中查找包含该航点的航路段
        // 这里需要一个更复杂的查询来找到相邻的航路段
        Ok(Vec::new())
    }

    /// 计算边的代价
    fn calculate_edge_cost(
        &self,
        distance: f64,
        airway: &Option<String>,
        request: &RouteRequest,
    ) -> f64 {
        let mut cost = distance * self.config.search_weights.distance_weight;

        // 根据偏好调整代价
        if airway.is_some() {
            match request.route_preference {
                RoutePreference::AirwayPreferred => {
                    cost *= 1.0 - self.config.search_weights.airway_preference_weight;
                }
                RoutePreference::DirectPreferred => {
                    cost *= 1.0 + self.config.search_weights.airway_preference_weight;
                }
                RoutePreference::Balanced => {}
            }
        }

        cost
    }


    /// 查找最近的航点
    fn find_nearest_waypoint(&self, coord: &Coordinate) -> Result<Waypoint> {
        let spatial_index = self.spatial_index.read();
        spatial_index
            .find_nearest(coord)
            .ok_or_else(|| RouteKitError::WaypointNotFound("最近航点未找到".to_string()))
    }

    /// 查找或创建航点
    fn find_or_create_waypoint(
        &self,
        identifier: &str,
        coord: &Coordinate,
    ) -> Result<Waypoint> {
        if let Some(wp) = self.db_pool.find_waypoint(identifier)? {
            Ok(wp)
        } else {
            Waypoint::simple(identifier, coord.latitude, coord.longitude)
        }
    }

    /// 估算飞行时间（分钟）
    fn estimate_flight_time(&self, distance_nm: f64) -> f64 {
        // 假设巡航速度为450节
        let cruise_speed = 450.0;
        crate::utils::calculate_flight_time_minutes(distance_nm, cruise_speed)
    }
}

/// 航路请求构建器
pub struct RouteRequestBuilder {
    request: RouteRequest,
}

impl RouteRequestBuilder {
    pub fn new(departure: &str, destination: &str) -> Self {
        Self {
            request: RouteRequest {
                departure_icao: departure.to_uppercase(),
                destination_icao: destination.to_uppercase(),
                ..Default::default()
            },
        }
    }

    pub fn flight_level(mut self, level: FlightLevel) -> Self {
        self.request.flight_level = Some(level);
        self
    }

    pub fn route_preference(mut self, pref: RoutePreference) -> Self {
        self.request.route_preference = pref;
        self
    }

    pub fn max_routes(mut self, max: usize) -> Self {
        self.request.max_routes = max;
        self
    }

    pub fn build(self) -> RouteRequest {
        self.request
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_request_builder() {
        let request = RouteRequestBuilder::new("ZBAA", "ZSPD")
            .flight_level(FlightLevel::High)
            .route_preference(RoutePreference::AirwayPreferred)
            .max_routes(5)
            .build();

        assert_eq!(request.departure_icao, "ZBAA");
        assert_eq!(request.destination_icao, "ZSPD");
        assert_eq!(request.max_routes, 5);
    }
}
