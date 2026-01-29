use serde::{Deserialize, Serialize};

/// 地理坐标
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinate {
    /// 纬度（度）
    pub latitude: f64,
    /// 经度（度）
    pub longitude: f64,
}

impl Coordinate {
    pub fn new(latitude: f64, longitude: f64) -> crate::error::Result<Self> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(crate::error::RouteKitError::InvalidCoordinate {
                lat: latitude,
                lon: longitude,
            });
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(crate::error::RouteKitError::InvalidCoordinate {
                lat: latitude,
                lon: longitude,
            });
        }
        Ok(Self {
            latitude,
            longitude,
        })
    }
}

/// 机场信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Airport {
    pub icao_code: String,
    pub identifier: String,
    pub identifier_3letter: Option<String>,
    pub name: Option<String>,
    pub coordinate: Coordinate,
    pub ifr_capability: Option<String>,
    pub elevation: Option<i32>,
    pub transition_altitude: Option<i32>,
    pub transition_level: Option<i32>,
}

/// 航点类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaypointType {
    Enroute,
    Terminal,
    VOR,
    NDB,
    Other(String),
}

/// 航点信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waypoint {
    pub identifier: String,
    pub icao_code: String,
    pub name: Option<String>,
    pub coordinate: Coordinate,
    pub waypoint_type: WaypointType,
    pub usage: Option<String>,
    pub id: Option<String>,
}

impl Waypoint {
    pub fn simple(identifier: &str, lat: f64, lon: f64) -> crate::error::Result<Self> {
        Ok(Self {
            identifier: identifier.to_string(),
            icao_code: String::new(),
            name: None,
            coordinate: Coordinate::new(lat, lon)?,
            waypoint_type: WaypointType::Enroute,
            usage: None,
            id: None,
        })
    }
}

/// 航路类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteType {
    High,
    Low,
    RNAV,
    Other,
}

/// 航路段信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirwaySegment {
    pub route_identifier: String,
    pub sequence: i32,
    pub waypoint: Waypoint,
    pub route_type: RouteType,
    pub flight_level: Option<String>,
    pub direction_restriction: Option<String>,
    pub minimum_altitude1: Option<i32>,
    pub minimum_altitude2: Option<i32>,
    pub maximum_altitude: Option<i32>,
    pub outbound_course: Option<f64>,
    pub inbound_course: Option<f64>,
    pub inbound_distance: Option<f64>,
}

/// SID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sid {
    pub airport_identifier: String,
    pub procedure_identifier: String,
    pub transition_identifier: Option<String>,
    pub waypoints: Vec<SidStarWaypoint>,
}

/// STAR
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Star {
    pub airport_identifier: String,
    pub procedure_identifier: String,
    pub transition_identifier: Option<String>,
    pub waypoints: Vec<SidStarWaypoint>,
}

/// SID/STAR航点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidStarWaypoint {
    pub sequence: i32,
    pub waypoint_identifier: String,
    pub coordinate: Coordinate,
    pub path_termination: Option<String>,
    pub magnetic_course: Option<f64>,
    pub route_distance: Option<f64>,
    pub altitude_description: Option<String>,
    pub altitude1: Option<i32>,
    pub altitude2: Option<i32>,
    pub speed_limit: Option<i32>,
}

/// 航段信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteSegment {
    pub from: Waypoint,
    pub to: Waypoint,
    pub airway: Option<String>,
    pub distance_nm: f64,
    pub magnetic_course: f64,
    pub minimum_altitude: Option<i32>,
    pub maximum_altitude: Option<i32>,
}

/// 完整航路
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub departure: Airport,
    pub destination: Airport,
    pub sid: Option<Sid>,
    pub star: Option<Star>,
    pub segments: Vec<RouteSegment>,
    pub total_distance_nm: f64,
    pub estimated_time_minutes: Option<f64>,
}

impl Route {
    /// 计算航路总距离
    pub fn calculate_total_distance(&self) -> f64 {
        self.segments.iter().map(|s| s.distance_nm).sum()
    }
    
    /// 生成航路字符串
    pub fn to_route_string(&self) -> String {
        let mut parts = Vec::new();
        
        // 起飞机场
        parts.push(self.departure.identifier.clone());
        
        // SID
        if let Some(sid) = &self.sid {
            parts.push(sid.procedure_identifier.clone());
        }
        
        // 航路段
        for segment in &self.segments {
            if let Some(airway) = &segment.airway {
                parts.push(airway.clone());
            }
            parts.push(segment.to.identifier.clone());
        }
        
        // STAR
        if let Some(star) = &self.star {
            parts.push(star.procedure_identifier.clone());
        }
        
        // 目的机场
        parts.push(self.destination.identifier.clone());
        
        parts.join(" ")
    }
}

/// 简化的航路结果（只包含字符串和距离）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResult {
    pub route_string: String,
    pub total_distance_nm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    pub departure_icao: String,
    pub destination_icao: String,
    pub flight_level: Option<FlightLevel>,
    pub route_preference: RoutePreference,
    pub max_routes: usize,
}

/// 飞行高度层
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlightLevel {
    Low,
    High,
    Custom(i32),
}

/// 偏好
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutePreference {
    AirwayPreferred,
    DirectPreferred,
    Balanced,
}

impl Default for RouteRequest {
    fn default() -> Self {
        Self {
            departure_icao: String::new(),
            destination_icao: String::new(),
            flight_level: Some(FlightLevel::High),
            route_preference: RoutePreference::Balanced,
            max_routes: 3,
        }
    }
}

/// 解析后的航路
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedRoute {
    pub raw_input: String,
    pub departure: Option<Airport>,
    pub destination: Option<Airport>,
    pub sid: Option<String>,
    pub star: Option<String>,
    pub elements: Vec<RouteElement>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub is_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RouteElement {
    Waypoint(Waypoint),
    /// 航路
    Airway {
        identifier: String,
        segments: Vec<AirwaySegment>,
    },
    /// DCT
    Direct {
        from: Waypoint,
        to: Waypoint,
    },
    SID(String),
    STAR(String),
    Unknown(String),
}
