//! # RouteKit
//!
//! 2025-01-29 @ Jerry Jin
//!
//!
//! ```no_run
//! use routekit::{RouteKit, RouteRequest, FlightLevel, RoutePreference};
//!
//! // 创建RouteKit实例
//! let kit = RouteKit::new("path/to/database.s3db").unwrap();
//!
//! // 查询航路
//! let request = RouteRequest {
//!     departure_icao: "ZBAA".to_string(),
//!     destination_icao: "ZSPD".to_string(),
//!     flight_level: Some(FlightLevel::High),
//!     route_preference: RoutePreference::Balanced,
//!     max_routes: 3,
//! };
//!
//! let routes = kit.find_routes(&request).unwrap();
//! println!("找到 {} 条航路", routes.len());
//!
//! // 解析航路字符串
//! let parsed = kit.parse_route("ZBAA SID TEPID G212 VYK STAR ZSPD").unwrap();
//! println!("解析结果: {:?}", parsed);
//! ```

pub mod config;
pub mod database;
pub mod error;
pub mod ffi;
pub mod geo;
pub mod models;
pub mod parser;
pub mod route;
pub mod spatial;
pub mod utils;

pub use config::Config;
pub use error::{Result, RouteKitError};
pub use models::*;
pub use route::RouteSearcher;

use database::DatabasePool;
use parser::RouteParser;
use parking_lot::RwLock;
use spatial::SpatialIndex;
use std::path::Path;
use std::sync::Arc;

pub struct RouteKit {
    db_pool: Arc<DatabasePool>,
    spatial_index: Arc<RwLock<SpatialIndex>>,
    route_searcher: RouteSearcher,
    route_parser: RouteParser,
    config: Config,
}

impl RouteKit {
    /// 使用默认配置创建新的RouteKit实例
    ///
    /// # 参数
    ///
    /// * `db_path` - SQLite数据库文件路径
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use routekit::RouteKit;
    ///
    /// let kit = RouteKit::new("raw_data/e_dfd_PMDG.s3db").unwrap();
    /// ```
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let config = Config::default();
        Self::with_config(db_path, config)
    }

    /// 使用自定义配置创建RouteKit实例
    ///
    /// # 参数
    ///
    /// * `db_path` - SQLite数据库文件路径
    /// * `config` - 自定义配置
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use routekit::{RouteKit, Config};
    ///
    /// let config = Config::builder()
    ///     .db_pool_size(20)
    ///     .max_search_depth(2000)
    ///     .build()
    ///     .unwrap();
    ///
    /// let kit = RouteKit::with_config("raw_data/e_dfd_PMDG.s3db", config).unwrap();
    /// ```
    pub fn with_config<P: AsRef<Path>>(db_path: P, config: Config) -> Result<Self> {
        config.validate()?;

        let db_pool = Arc::new(DatabasePool::new(db_path.as_ref(), config.db_pool_size)?);
        let spatial_index = Arc::new(RwLock::new(SpatialIndex::new()));

        let route_searcher = RouteSearcher::new(
            Arc::clone(&db_pool),
            Arc::clone(&spatial_index),
            config.clone(),
        );

        let route_parser = RouteParser::new(Arc::clone(&db_pool));

        let instance = Self {
            db_pool,
            spatial_index,
            route_searcher,
            route_parser,
            config,
        };

        // 初始化空间索引
        instance.initialize_spatial_index()?;

        log::info!("RouteKit实例初始化成功");

        Ok(instance)
    }

    /// 初始化空间索引（加载所有航点）
    fn initialize_spatial_index(&self) -> Result<()> {
        log::info!("开始初始化空间索引...");
        
        let waypoints = self.db_pool.load_all_waypoints()?;
        let waypoint_count = waypoints.len();
        
        let mut index = self.spatial_index.write();
        index.bulk_insert(waypoints);
        
        log::info!("空间索引初始化完成，共加载 {} 个航点", waypoint_count);
        
        Ok(())
    }

    /// 查询航路
    ///
    /// # 参数
    ///
    /// * `request` - 航路查询请求
    ///
    /// # 返回
    ///
    /// 返回找到的航路列表，按优先级排序
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use routekit::{RouteKit, RouteRequest, FlightLevel, RoutePreference};
    ///
    /// let kit = RouteKit::new("raw_data/e_dfd_PMDG.s3db").unwrap();
    /// let request = RouteRequest {
    ///     departure_icao: "ZBAA".to_string(),
    ///     destination_icao: "ZSPD".to_string(),
    ///     flight_level: Some(FlightLevel::High),
    ///     route_preference: RoutePreference::AirwayPreferred,
    ///     max_routes: 3,
    /// };
    ///
    /// let routes = kit.find_routes(&request).unwrap();
    /// for route in routes {
    ///     println!("航路总距离: {} 海里", route.total_distance_nm);
    /// }
    /// ```
    pub fn find_routes(&self, request: &RouteRequest) -> Result<Vec<Route>> {
        log::debug!(
            "开始查询航路: {} -> {}",
            request.departure_icao,
            request.destination_icao
        );

        let routes = self.route_searcher.search_routes(request)?;

        log::debug!("找到 {} 条航路", routes.len());

        Ok(routes)
    }

    /// 解析航路字符串
    ///
    /// # 参数
    ///
    /// * `route_string` - 航路字符串（支持多种格式）
    ///
    /// # 返回
    ///
    /// 返回解析后的航路信息
    ///
    /// # 示例
    ///
    /// ```no_run
    /// use routekit::RouteKit;
    ///
    /// let kit = RouteKit::new("raw_data/e_dfd_PMDG.s3db").unwrap();
    /// let parsed = kit.parse_route("ZBAA SID TEPID G212 VYK STAR ZSPD").unwrap();
    ///
    /// if parsed.is_valid {
    ///     println!("起飞机场: {:?}", parsed.departure);
    ///     println!("目的机场: {:?}", parsed.destination);
    ///     println!("航路元素数量: {}", parsed.elements.len());
    /// }
    /// ```
    pub fn parse_route(&self, route_string: &str) -> Result<ParsedRoute> {
        log::debug!("开始解析航路字符串: {}", route_string);

        let parsed = self.route_parser.parse(route_string)?;

        log::debug!(
            "解析完成，有效性: {}, 元素数量: {}",
            parsed.is_valid,
            parsed.elements.len()
        );

        Ok(parsed)
    }

    /// 解析航路字符串（宽松模式，容错性更高）
    ///
    /// # 参数
    ///
    /// * `route_string` - 航路字符串
    ///
    /// # 返回
    ///
    /// 返回解析后的航路信息，即使部分解析失败也会返回结果
    pub fn parse_route_flexible(&self, route_string: &str) -> Result<ParsedRoute> {
        log::debug!("开始解析航路字符串（宽松模式）: {}", route_string);

        let parsed = self.route_parser.parse_flexible(route_string)?;

        log::debug!(
            "解析完成，有效性: {}, 警告数量: {}",
            parsed.is_valid,
            parsed.warnings.len()
        );

        Ok(parsed)
    }

    /// 加载机场信息
    ///
    /// # 参数
    ///
    /// * `icao` - 机场ICAO代码
    pub fn load_airport(&self, icao: &str) -> Result<Airport> {
        self.db_pool.load_airport(icao)
    }

    /// 查找航点
    ///
    /// # 参数
    ///
    /// * `identifier` - 航点标识符
    pub fn find_waypoint(&self, identifier: &str) -> Result<Option<Waypoint>> {
        self.db_pool.find_waypoint(identifier)
    }

    /// 查找最近的航点
    ///
    /// # 参数
    ///
    /// * `coordinate` - 坐标
    pub fn find_nearest_waypoint(&self, coordinate: &Coordinate) -> Option<Waypoint> {
        let index = self.spatial_index.read();
        index.find_nearest(coordinate)
    }

    /// 在指定半径内查找航点
    ///
    /// # 参数
    ///
    /// * `coordinate` - 中心坐标
    /// * `radius_nm` - 搜索半径（海里）
    pub fn find_waypoints_within_radius(
        &self,
        coordinate: &Coordinate,
        radius_nm: f64,
    ) -> Vec<Waypoint> {
        let index = self.spatial_index.read();
        index.find_within_radius(coordinate, radius_nm)
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn reload_spatial_index(&self) -> Result<()> {
        self.initialize_spatial_index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_routekit_creation() {
    }

    #[test]
    fn test_coordinate_creation() {
        let coord = Coordinate::new(40.0, 116.0).unwrap();
        assert_eq!(coord.latitude, 40.0);
        assert_eq!(coord.longitude, 116.0);

        // 测试无效坐标
        assert!(Coordinate::new(100.0, 0.0).is_err());
        assert!(Coordinate::new(0.0, 200.0).is_err());
    }

    #[test]
    fn test_route_request_default() {
        let request = RouteRequest::default();
        assert_eq!(request.max_routes, 3);
        assert_eq!(request.route_preference, RoutePreference::Balanced);
    }
}
