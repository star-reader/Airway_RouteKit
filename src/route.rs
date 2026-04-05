use crate::database::DatabasePool;
use crate::error::{Result, RouteKitError};
use crate::geo::haversine_distance_nm;
use crate::models::*;
use crate::spatial::SpatialIndex;
use parking_lot::RwLock;
use std::collections::{BinaryHeap, HashMap};
use std::cmp::Ordering;
use std::sync::Arc;

/// 航路图中的边
#[derive(Debug, Clone)]
struct Edge {
    /// 目标节点索引
    to_idx: usize,
    /// 航路名称
    airway: String,
    /// 距离（海里）
    distance_nm: f64,
}

/// Dijkstra搜索节点
#[derive(Debug, Clone)]
struct SearchNode {
    /// 节点索引
    idx: usize,
    /// 从起点到此节点的累计距离
    dist: f64,
    /// 路径记录: (航路名, 航点名, 节点索引)
    route_list: Vec<(String, String, usize)>,
}

impl PartialEq for SearchNode {
    fn eq(&self, other: &Self) -> bool {
        self.dist == other.dist
    }
}

impl Eq for SearchNode {}

impl PartialOrd for SearchNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SearchNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // 最小堆：距离小的优先
        other.dist.partial_cmp(&self.dist).unwrap_or(Ordering::Equal)
    }
}

/// 计算坐标哈希值（用于区分同名但不同位置的航点）
fn calc_coord_hash(coord: &Coordinate) -> i64 {
    // 将坐标转换为整数（保留4位小数精度）
    let lat_int = (coord.latitude * 10000.0).round() as i64;
    let lon_int = (coord.longitude * 10000.0).round() as i64;
    // 组合成唯一哈希值
    lat_int * 10000000 + lon_int
}

/// 生成唯一节点key（名称+坐标哈希）
fn make_node_key(identifier: &str, coord: &Coordinate) -> String {
    format!("{}_{}", identifier, calc_coord_hash(coord))
}

/// 航路图
struct AirwayGraph {
    /// 节点列表：(航点标识符, 坐标)
    nodes: Vec<(String, Coordinate)>,
    /// 节点唯一key -> 节点索引
    node_index: HashMap<String, usize>,
    /// 邻接表：每个节点的出边列表
    adjacency: Vec<Vec<Edge>>,
}

impl AirwayGraph {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            node_index: HashMap::new(),
            adjacency: Vec::new(),
        }
    }

    /// 获取或创建节点索引（使用名称+坐标哈希作为唯一标识）
    fn get_or_create_node(&mut self, identifier: &str, coord: Coordinate) -> usize {
        let key = make_node_key(identifier, &coord);
        if let Some(&idx) = self.node_index.get(&key) {
            return idx;
        }
        let idx = self.nodes.len();
        self.nodes.push((identifier.to_string(), coord));
        self.node_index.insert(key, idx);
        self.adjacency.push(Vec::new());
        idx
    }

    /// 添加边
    fn add_edge(&mut self, from_idx: usize, to_idx: usize, airway: &str, distance_nm: f64) {
        self.adjacency[from_idx].push(Edge {
            to_idx,
            airway: airway.to_string(),
            distance_nm,
        });
    }

    /// 获取节点坐标
    fn get_coord(&self, idx: usize) -> &Coordinate {
        &self.nodes[idx].1
    }

    /// 获取节点标识符
    fn get_identifier(&self, idx: usize) -> &str {
        &self.nodes[idx].0
    }

    /// 通过名称和坐标获取节点索引
    fn get_index_by_coord(&self, identifier: &str, coord: &Coordinate) -> Option<usize> {
        let key = make_node_key(identifier, coord);
        self.node_index.get(&key).copied()
    }
    
    // fn find_nearest_node(&self, identifier: &str, near_coord: &Coordinate) -> Option<usize> {
    //     let mut best_idx: Option<usize> = None;
    //     let mut best_dist = f64::INFINITY;
        
    //     for (idx, (name, coord)) in self.nodes.iter().enumerate() {
    //         if name == identifier {
    //             let dist = haversine_distance_nm(near_coord, coord);
    //             if dist < best_dist {
    //                 best_dist = dist;
    //                 best_idx = Some(idx);
    //             }
    //         }
    //     }
    //     best_idx
    // }
}

/// 航路搜索器
pub struct RouteSearcher {
    db_pool: Arc<DatabasePool>,
    #[allow(dead_code)]
    spatial_index: Arc<RwLock<SpatialIndex>>,
    #[allow(dead_code)]
    config: crate::config::Config,
    /// 预加载的航路图
    graph: RwLock<Option<AirwayGraph>>,
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
            graph: RwLock::new(None),
        }
    }

    /// 确保航路图已加载
    fn ensure_graph_loaded(&self) -> Result<()> {
        {
            let graph = self.graph.read();
            if graph.is_some() {
                return Ok(());
            }
        }
        
        log::info!("开始加载航路网络图...");
        let start_time = std::time::Instant::now();
        
        let mut graph = AirwayGraph::new();
        
        // 批量加载所有航路边
        let edges = self.db_pool.load_all_airway_edges()?;
        log::info!("从数据库加载了 {} 条航路边", edges.len());
        
        for (airway, from_id, from_coord, to_id, to_coord) in edges {
            let from_idx = graph.get_or_create_node(&from_id, from_coord);
            let to_idx = graph.get_or_create_node(&to_id, to_coord);
            let distance = haversine_distance_nm(&from_coord, &to_coord);
            
            // 添加双向边（大多数航路是双向的）
            graph.add_edge(from_idx, to_idx, &airway, distance);
            graph.add_edge(to_idx, from_idx, &airway, distance);
        }
        
        let elapsed = start_time.elapsed();
        log::info!(
            "航路网络图加载完成: {} 个节点, {} 条边, 耗时 {:.2}s",
            graph.nodes.len(),
            graph.adjacency.iter().map(|v| v.len()).sum::<usize>(),
            elapsed.as_secs_f64()
        );
        
        let mut graph_lock = self.graph.write();
        *graph_lock = Some(graph);
        
        Ok(())
    }

    /// 搜索航路
    pub fn search_routes_simple(&self, request: &RouteRequest) -> Result<Vec<RouteResult>> {
        log::info!("开始航路搜索: {} -> {}", request.departure_icao, request.destination_icao);
        let start_time = std::time::Instant::now();
        
        // 确保图已加载
        self.ensure_graph_loaded()?;
        
        // 加载起飞和目的机场
        let departure = self.db_pool.load_airport(&request.departure_icao)?;
        let destination = self.db_pool.load_airport(&request.destination_icao)?;
        
        log::debug!("起飞机场: {} at ({:.4}, {:.4})", 
            departure.identifier, departure.coordinate.latitude, departure.coordinate.longitude);
        log::debug!("目的机场: {} at ({:.4}, {:.4})", 
            destination.identifier, destination.coordinate.latitude, destination.coordinate.longitude);
        
        // 查找SID出口航点
        let sid_exit_waypoints = self.db_pool.find_sid_exit_waypoints(&departure.identifier)?;
        log::info!("找到 {} 个SID出口航点", sid_exit_waypoints.len());
        
        // 查找STAR入口航点
        let star_entry_waypoints = self.db_pool.find_star_entry_waypoints(&destination.identifier)?;
        log::info!("找到 {} 个STAR入口航点", star_entry_waypoints.len());
        
        // 如果没有SID/STAR航点，使用最近的航路航点
        let start_waypoints = if sid_exit_waypoints.is_empty() {
            log::warn!("没有SID出口航点，使用机场附近的航路航点");
            self.db_pool.find_airway_waypoints_near(
                departure.coordinate.latitude,
                departure.coordinate.longitude,
                1.0,
            )?
        } else {
            sid_exit_waypoints
        };
        
        let end_waypoints = if star_entry_waypoints.is_empty() {
            log::warn!("没有STAR入口航点，使用机场附近的航路航点");
            self.db_pool.find_airway_waypoints_near(
                destination.coordinate.latitude,
                destination.coordinate.longitude,
                1.0,
            )?
        } else {
            star_entry_waypoints
        };
        
        if start_waypoints.is_empty() || end_waypoints.is_empty() {
            return Err(RouteKitError::RouteNotFound {
                from: request.departure_icao.clone(),
                to: request.destination_icao.clone(),
            });
        }
        
        // 使用Dijkstra算法搜索
        let graph = self.graph.read();
        let graph = graph.as_ref().unwrap();
        
        let mut results: Vec<RouteResult> = Vec::new();
        let mut seen_routes: std::collections::HashSet<String> = std::collections::HashSet::new();
        
        // 尝试不同的起点/终点组合
        for start_wp in start_waypoints.iter().take(3) {
            let start_idx = match graph.get_index_by_coord(&start_wp.identifier, &start_wp.coordinate) {
                Some(idx) => idx,
                None => continue,
            };
            
            for end_wp in end_waypoints.iter().take(3) {
                let end_idx = match graph.get_index_by_coord(&end_wp.identifier, &end_wp.coordinate) {
                    Some(idx) => idx,
                    None => continue,
                };
                
                log::debug!("尝试路线: {} -> {}", start_wp.identifier, end_wp.identifier);
                
                if let Some((route_list, total_dist)) = self.dijkstra(graph, start_idx, end_idx) {
                    // 构建航路字符串
                    let route_string = self.build_route_string(
                        &departure.identifier,
                        &destination.identifier,
                        &start_wp.identifier,
                        &route_list,
                    );
                    
                    // 避免重复
                    if seen_routes.contains(&route_string) {
                        continue;
                    }
                    seen_routes.insert(route_string.clone());
                    
                    log::info!("找到路线: {}, 距离: {:.1}nm", route_string, total_dist);
                    
                    results.push(RouteResult {
                        route_string,
                        total_distance_nm: total_dist,
                    });
                    
                    // 限制返回数量
                    if results.len() >= request.max_routes {
                        break;
                    }
                }
            }
            
            if results.len() >= request.max_routes {
                break;
            }
        }
        
        // 按距离排序
        results.sort_by(|a, b| a.total_distance_nm.partial_cmp(&b.total_distance_nm).unwrap_or(Ordering::Equal));
        
        let elapsed = start_time.elapsed();
        log::info!("航路搜索完成，找到 {} 条航路，耗时 {:.2}s", results.len(), elapsed.as_secs_f64());
        
        if results.is_empty() {
            return Err(RouteKitError::RouteNotFound {
                from: request.departure_icao.clone(),
                to: request.destination_icao.clone(),
            });
        }
        
        Ok(results)
    }

    /// 搜索航路（兼容旧接口）
    pub fn search_routes(&self, request: &RouteRequest) -> Result<Vec<Route>> {
        log::info!("开始航路搜索: {} -> {}", request.departure_icao, request.destination_icao);
        let start_time = std::time::Instant::now();
        
        // 确保图已加载
        self.ensure_graph_loaded()?;
        
        // 加载起飞和目的机场
        let departure = self.db_pool.load_airport(&request.departure_icao)?;
        let destination = self.db_pool.load_airport(&request.destination_icao)?;
        
        // 查找SID出口航点和STAR入口航点
        let sid_exit_waypoints = self.db_pool.find_sid_exit_waypoints(&departure.identifier)?;
        let star_entry_waypoints = self.db_pool.find_star_entry_waypoints(&destination.identifier)?;
        
        // 如果没有SID/STAR航点，使用最近的航路航点
        let start_waypoints = if sid_exit_waypoints.is_empty() {
            self.db_pool.find_airway_waypoints_near(
                departure.coordinate.latitude,
                departure.coordinate.longitude,
                1.0,
            )?
        } else {
            sid_exit_waypoints
        };
        
        let end_waypoints = if star_entry_waypoints.is_empty() {
            self.db_pool.find_airway_waypoints_near(
                destination.coordinate.latitude,
                destination.coordinate.longitude,
                1.0,
            )?
        } else {
            star_entry_waypoints
        };
        
        if start_waypoints.is_empty() || end_waypoints.is_empty() {
            return Err(RouteKitError::RouteNotFound {
                from: request.departure_icao.clone(),
                to: request.destination_icao.clone(),
            });
        }
        
        let graph = self.graph.read();
        let graph = graph.as_ref().unwrap();
        
        let mut best_route: Option<(Vec<(String, String, usize)>, f64, String, usize)> = None;
        
        for start_wp in start_waypoints.iter().take(3) {
            let start_idx = match graph.get_index_by_coord(&start_wp.identifier, &start_wp.coordinate) {
                Some(idx) => idx,
                None => continue,
            };
            
            for end_wp in end_waypoints.iter().take(3) {
                let end_idx = match graph.get_index_by_coord(&end_wp.identifier, &end_wp.coordinate) {
                    Some(idx) => idx,
                    None => continue,
                };
                
                if let Some((route_list, total_dist)) = self.dijkstra(graph, start_idx, end_idx) {
                    // 保留最短的
                    if best_route.is_none() || total_dist < best_route.as_ref().unwrap().1 {
                        best_route = Some((route_list, total_dist, start_wp.identifier.clone(), start_idx));
                    }
                }
            }
        }
        
        let elapsed = start_time.elapsed();
        log::info!("航路搜索完成，耗时 {:.2}s", elapsed.as_secs_f64());
        
        match best_route {
            Some((route_list, total_dist, start_wp_id, start_idx)) => {
                let route = self.build_route(&departure, &destination, graph, &route_list, total_dist, &start_wp_id, start_idx)?;
                Ok(vec![route])
            }
            None => Err(RouteKitError::RouteNotFound {
                from: request.departure_icao.clone(),
                to: request.destination_icao.clone(),
            }),
        }
    }

    /// Dijkstra算法
    fn dijkstra(
        &self,
        graph: &AirwayGraph,
        start_idx: usize,
        end_idx: usize,
    ) -> Option<(Vec<(String, String, usize)>, f64)> {
        // 距离数组
        let mut dist: Vec<f64> = vec![f64::INFINITY; graph.nodes.len()];
        dist[start_idx] = 0.0;
        
        // 优先队列
        let mut heap = BinaryHeap::new();
        heap.push(SearchNode {
            idx: start_idx,
            dist: 0.0,
            route_list: Vec::new(),
        });
        
        let mut iterations = 0;
        let max_iterations = 500000;
        
        while let Some(current) = heap.pop() {
            iterations += 1;
            if iterations > max_iterations {
                log::warn!("Dijkstra达到最大迭代次数限制");
                break;
            }
            
            // 找到目标
            if current.idx == end_idx {
                log::debug!("Dijkstra完成: {} 次迭代", iterations);
                return Some((current.route_list, current.dist));
            }
            
            // 如果当前距离大于已知最短距离，跳过
            if current.dist > dist[current.idx] {
                continue;
            }
            
            // 遍历邻居
            for edge in &graph.adjacency[current.idx] {
                let next_dist = current.dist + edge.distance_nm;
                
                // 松弛操作
                if next_dist < dist[edge.to_idx] {
                    dist[edge.to_idx] = next_dist;
                    
                    let mut new_route_list = current.route_list.clone();
                    new_route_list.push((
                        edge.airway.clone(),
                        graph.get_identifier(edge.to_idx).to_string(),
                        edge.to_idx,
                    ));
                    
                    heap.push(SearchNode {
                        idx: edge.to_idx,
                        dist: next_dist,
                        route_list: new_route_list,
                    });
                }
            }
        }
        
        None
    }

    /// 构建航路字符串（合并连续相同的航路，格式：机场 航点 航路 航点 航路 航点 机场）
    fn build_route_string(
        &self,
        departure_icao: &str,
        destination_icao: &str,
        first_waypoint: &str,
        route_list: &[(String, String, usize)],
    ) -> String {
        let mut parts: Vec<String> = Vec::new();
        
        // 起飞机场
        parts.push(departure_icao.to_string());
        
        // 第一个航点
        parts.push(first_waypoint.to_string());
        
        if route_list.is_empty() {
            parts.push(destination_icao.to_string());
            return parts.join(" ");
        }
        
        // 合并连续相同的航路，在航路切换时输出航点
        // 格式: DEP WP1 AWY1 WP2 AWY2 WP3 ARR
        let mut last_airway: Option<&str> = None;
        let mut last_waypoint: Option<&str> = None;
        
        for (airway, waypoint, _) in route_list {
            if last_airway.map(|a| a != airway.as_str()).unwrap_or(true) {
                // 航路改变了
                // 如果有上一个航路的最后航点，先输出它
                if let Some(wp) = last_waypoint {
                    parts.push(wp.to_string());
                }
                // 输出新航路
                parts.push(airway.clone());
            }
            last_airway = Some(airway.as_str());
            last_waypoint = Some(waypoint.as_str());
        }
        
        // 添加最后一个航点
        if let Some(wp) = last_waypoint {
            parts.push(wp.to_string());
        }
        
        // 目的机场
        parts.push(destination_icao.to_string());
        
        parts.join(" ")
    }

    /// 构建Route对象
    fn build_route(
        &self,
        departure: &Airport,
        destination: &Airport,
        graph: &AirwayGraph,
        route_list: &[(String, String, usize)],
        total_dist: f64,
        first_waypoint_id: &str,
        first_waypoint_idx: usize,
    ) -> Result<Route> {
        let mut segments = Vec::new();
        
        if route_list.is_empty() {
            return Ok(Route {
                departure: departure.clone(),
                destination: destination.clone(),
                sid: None,
                star: None,
                segments,
                total_distance_nm: total_dist,
                estimated_time_minutes: Some(self.estimate_flight_time(total_dist)),
            });
        }
        
        // 构建航段
        let mut prev_idx = first_waypoint_idx;
        let mut prev_id = first_waypoint_id.to_string();
        
        for (airway, waypoint_id, idx) in route_list {
            let from_coord = graph.get_coord(prev_idx);
            let to_coord = graph.get_coord(*idx);
            
            let distance = haversine_distance_nm(from_coord, to_coord);
            let bearing = crate::geo::calculate_bearing(from_coord, to_coord);
            
            segments.push(RouteSegment {
                from: Waypoint::simple(&prev_id, from_coord.latitude, from_coord.longitude)?,
                to: Waypoint::simple(waypoint_id, to_coord.latitude, to_coord.longitude)?,
                airway: Some(airway.clone()),
                distance_nm: distance,
                magnetic_course: bearing,
                minimum_altitude: None,
                maximum_altitude: None,
            });
            
            prev_idx = *idx;
            prev_id = waypoint_id.clone();
        }
        
        Ok(Route {
            departure: departure.clone(),
            destination: destination.clone(),
            sid: None,
            star: None,
            segments,
            total_distance_nm: total_dist,
            estimated_time_minutes: Some(self.estimate_flight_time(total_dist)),
        })
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
