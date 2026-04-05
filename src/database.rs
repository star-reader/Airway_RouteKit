use crate::error::{Result, RouteKitError};
use crate::models::*;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OptionalExtension, Row};
use std::path::Path;

pub struct DatabasePool {
    pool: Pool<SqliteConnectionManager>,
}

impl DatabasePool {
    pub fn new<P: AsRef<Path>>(db_path: P, pool_size: u32) -> Result<Self> {
        let manager = SqliteConnectionManager::file(db_path);
        let pool = Pool::builder()
            .max_size(pool_size)
            .build(manager)?;

        Ok(Self { pool })
    }

    fn get_connection(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool.get().map_err(|e| e.into())
    }

    pub fn load_airport(&self, icao: &str) -> Result<Airport> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT icao_code, airport_identifier, airport_identifier_3letter, 
                    airport_name, airport_ref_latitude, airport_ref_longitude,
                    ifr_capability, elevation, transition_altitude, transition_level
             FROM tbl_airports 
             WHERE airport_identifier = ?1 OR icao_code = ?1"
        )?;

        let airport = stmt
            .query_row(params![icao], |row| {
                Ok(Airport {
                    icao_code: row.get(0)?,
                    identifier: row.get(1)?,
                    identifier_3letter: row.get(2)?,
                    name: row.get(3)?,
                    coordinate: Coordinate::new(
                        row.get::<_, f64>(4)?,
                        row.get::<_, f64>(5)?,
                    ).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    ifr_capability: row.get(6)?,
                    elevation: row.get(7)?,
                    transition_altitude: row.get(8)?,
                    transition_level: row.get(9)?,
                })
            })
            .optional()?
            .ok_or_else(|| RouteKitError::AirportNotFound(icao.to_string()))?;

        Ok(airport)
    }

    pub fn load_all_waypoints(&self) -> Result<Vec<Waypoint>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT waypoint_identifier, icao_code, waypoint_name, 
                    waypoint_latitude, waypoint_longitude, waypoint_type, 
                    waypoint_usage, id
             FROM tbl_enroute_waypoints"
        )?;

        let waypoints = stmt
            .query_map([], |row| {
                let lat: f64 = row.get(3)?;
                let lon: f64 = row.get(4)?;
                
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: row.get::<_, String>(1).unwrap_or_default(),
                    name: row.get::<_, Option<String>>(2)?,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Enroute,
                    usage: row.get::<_, Option<String>>(6)?,
                    id: row.get::<_, Option<String>>(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(waypoints)
    }

    pub fn find_waypoint(&self, identifier: &str) -> Result<Option<Waypoint>> {
        let conn = self.get_connection()?;
        
        // 先在航路航点中查找
        let mut stmt = conn.prepare(
            "SELECT waypoint_identifier, icao_code, waypoint_name, 
                    waypoint_latitude, waypoint_longitude, waypoint_type, 
                    waypoint_usage, id
             FROM tbl_enroute_waypoints 
             WHERE waypoint_identifier = ?1 
             LIMIT 1"
        )?;

        let waypoint = stmt
            .query_row(params![identifier], |row| {
                let lat: f64 = row.get(3)?;
                let lon: f64 = row.get(4)?;
                
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: row.get::<_, String>(1).unwrap_or_default(),
                    name: row.get::<_, Option<String>>(2)?,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Enroute,
                    usage: row.get::<_, Option<String>>(6)?,
                    id: row.get::<_, Option<String>>(7)?,
                })
            })
            .optional()?;

        if waypoint.is_some() {
            return Ok(waypoint);
        }

        // 在终端区航点中查找
        let mut stmt = conn.prepare(
            "SELECT waypoint_identifier, icao_code, waypoint_name, 
                    waypoint_latitude, waypoint_longitude, id
             FROM tbl_terminal_waypoints 
             WHERE waypoint_identifier = ?1 
             LIMIT 1"
        )?;

        let waypoint = stmt
            .query_row(params![identifier], |row| {
                let lat: f64 = row.get(3)?;
                let lon: f64 = row.get(4)?;
                
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: row.get::<_, String>(1).unwrap_or_default(),
                    name: row.get::<_, Option<String>>(2)?,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Terminal,
                    usage: None,
                    id: row.get::<_, Option<String>>(5)?,
                })
            })
            .optional()?;

        Ok(waypoint)
    }

    /// 查找航路段
    pub fn find_airway_segments(&self, route_identifier: &str) -> Result<Vec<AirwaySegment>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT route_identifier, seqno, waypoint_identifier, 
                    waypoint_latitude, waypoint_longitude, icao_code,
                    route_type, flightlevel, direction_restriction,
                    minimum_altitude1, minimum_altitude2, maximum_altitude,
                    outbound_course, inbound_course, inbound_distance
             FROM tbl_enroute_airways 
             WHERE route_identifier = ?1 
             ORDER BY seqno"
        )?;

        let segments = stmt
            .query_map(params![route_identifier], |row| {
                let route_type = match row.get::<_, Option<String>>(6)?.as_deref() {
                    Some("H") => RouteType::High,
                    Some("L") => RouteType::Low,
                    Some("R") => RouteType::RNAV,
                    _ => RouteType::Other,
                };

                let lat: f64 = row.get(3)?;
                let lon: f64 = row.get(4)?;

                Ok(AirwaySegment {
                    route_identifier: row.get::<_, String>(0)?,
                    sequence: row.get::<_, i32>(1)?,
                    waypoint: Waypoint {
                        identifier: row.get::<_, String>(2)?,
                        coordinate: Coordinate::new(lat, lon)
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        icao_code: row.get::<_, String>(5).unwrap_or_default(),
                        name: None,
                        waypoint_type: WaypointType::Enroute,
                        usage: None,
                        id: None,
                    },
                    route_type,
                    flight_level: row.get::<_, Option<String>>(7)?,
                    direction_restriction: row.get::<_, Option<String>>(8)?,
                    minimum_altitude1: row.get::<_, Option<i32>>(9)?,
                    minimum_altitude2: row.get::<_, Option<i32>>(10)?,
                    maximum_altitude: row.get::<_, Option<i32>>(11)?,
                    outbound_course: row.get::<_, Option<f64>>(12)?,
                    inbound_course: row.get::<_, Option<f64>>(13)?,
                    inbound_distance: row.get::<_, Option<f64>>(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(segments)
    }

    /// 查找机场的SID
    pub fn find_sids(&self, airport_icao: &str) -> Result<Vec<Sid>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT procedure_identifier, transition_identifier
             FROM tbl_sids 
             WHERE airport_identifier = ?1
             LIMIT 5"
        )?;

        let mut sids = Vec::new();
        let rows = stmt.query_map(params![airport_icao], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })?;

        for row in rows {
            let (procedure_id, transition_id) = row?;
            let waypoints = self.load_sid_waypoints(airport_icao, &procedure_id, transition_id.as_deref())?;
            sids.push(Sid {
                airport_identifier: airport_icao.to_string(),
                procedure_identifier: procedure_id,
                transition_identifier: transition_id,
                waypoints,
            });
        }

        Ok(sids)
    }

    /// 加载SID航点
    fn load_sid_waypoints(
        &self,
        airport_icao: &str,
        procedure_id: &str,
        transition_id: Option<&str>,
    ) -> Result<Vec<SidStarWaypoint>> {
        let conn = self.get_connection()?;
        let query = if transition_id.is_some() {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_sids 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier = ?3
               AND waypoint_identifier IS NOT NULL
             ORDER BY seqno"
        } else {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_sids 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier IS NULL
               AND waypoint_identifier IS NOT NULL
             ORDER BY seqno"
        };

        let mut stmt = conn.prepare(query)?;
        
        let waypoints = if let Some(trans) = transition_id {
            stmt.query_map(params![airport_icao, procedure_id, trans], Self::parse_sid_star_waypoint)?
        } else {
            stmt.query_map(params![airport_icao, procedure_id], Self::parse_sid_star_waypoint)?
        }
        .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(waypoints)
    }

    /// 查找机场的STAR
    pub fn find_stars(&self, airport_icao: &str) -> Result<Vec<Star>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT procedure_identifier, transition_identifier
             FROM tbl_stars 
             WHERE airport_identifier = ?1
             LIMIT 5"
        )?;

        let mut stars = Vec::new();
        let rows = stmt.query_map(params![airport_icao], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })?;

        for row in rows {
            let (procedure_id, transition_id) = row?;
            let waypoints = self.load_star_waypoints(airport_icao, &procedure_id, transition_id.as_deref())?;
            stars.push(Star {
                airport_identifier: airport_icao.to_string(),
                procedure_identifier: procedure_id,
                transition_identifier: transition_id,
                waypoints,
            });
        }

        Ok(stars)
    }

    /// 加载STAR航点
    fn load_star_waypoints(
        &self,
        airport_icao: &str,
        procedure_id: &str,
        transition_id: Option<&str>,
    ) -> Result<Vec<SidStarWaypoint>> {
        let conn = self.get_connection()?;
        let query = if transition_id.is_some() {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_stars 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier = ?3
               AND waypoint_identifier IS NOT NULL
             ORDER BY seqno"
        } else {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_stars 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier IS NULL
               AND waypoint_identifier IS NOT NULL
             ORDER BY seqno"
        };

        let mut stmt = conn.prepare(query)?;
        
        let waypoints = if let Some(trans) = transition_id {
            stmt.query_map(params![airport_icao, procedure_id, trans], Self::parse_sid_star_waypoint)?
        } else {
            stmt.query_map(params![airport_icao, procedure_id], Self::parse_sid_star_waypoint)?
        }
        .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(waypoints)
    }

    /// 解析SID/STAR航点
    fn parse_sid_star_waypoint(row: &Row) -> rusqlite::Result<SidStarWaypoint> {
        Ok(SidStarWaypoint {
            sequence: row.get(0)?,
            waypoint_identifier: row.get(1)?,
            coordinate: Coordinate::new(
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
            ).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            path_termination: row.get(4)?,
            magnetic_course: row.get(5)?,
            route_distance: row.get(6)?,
            altitude_description: row.get(7)?,
            altitude1: row.get(8)?,
            altitude2: row.get(9)?,
            speed_limit: row.get(10)?,
        })
    }

    /// 查找连接两个航点的航路
    pub fn find_airways_between(
        &self,
        from_waypoint: &str,
        to_waypoint: &str,
    ) -> Result<Vec<String>> {
        let conn = self.get_connection()?;
        
        // 查找包含这两个航点的航路
        let mut stmt = conn.prepare(
            "SELECT DISTINCT a1.route_identifier
             FROM tbl_enroute_airways a1
             JOIN tbl_enroute_airways a2 ON a1.route_identifier = a2.route_identifier
             WHERE a1.waypoint_identifier = ?1 
               AND a2.waypoint_identifier = ?2
               AND a1.seqno < a2.seqno"
        )?;

        let airways = stmt
            .query_map(params![from_waypoint, to_waypoint], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;

        Ok(airways)
    }

    /// 查找在航路网络中且靠近指定坐标的航点
    pub fn find_airway_waypoints_near(
        &self,
        latitude: f64,
        longitude: f64,
        radius_degrees: f64,
    ) -> Result<Vec<Waypoint>> {
        let conn = self.get_connection()?;
        
        let lat_min = latitude - radius_degrees;
        let lat_max = latitude + radius_degrees;
        let lon_min = longitude - radius_degrees;
        let lon_max = longitude + radius_degrees;
        
        let mut stmt = conn.prepare(
            "SELECT DISTINCT waypoint_identifier, icao_code, waypoint_latitude, waypoint_longitude
             FROM tbl_enroute_airways
             WHERE waypoint_latitude BETWEEN ?1 AND ?2
               AND waypoint_longitude BETWEEN ?3 AND ?4
             LIMIT 30"
        )?;
        
        let waypoints = stmt
            .query_map(params![lat_min, lat_max, lon_min, lon_max], |row| {
                let lat: f64 = row.get(2)?;
                let lon: f64 = row.get(3)?;
                
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: row.get::<_, String>(1).unwrap_or_default(),
                    name: None,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Enroute,
                    usage: None,
                    id: None,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        Ok(waypoints)
    }

    /// 查找从指定航点出发的航路段
    pub fn find_outbound_airway_segments(
        &self,
        waypoint_identifier: &str,
    ) -> Result<Vec<AirwaySegment>> {
        let conn = self.get_connection()?;
        
        // 简化查询：找到当前航点之后的所有航点（seqno更大的）
        let mut stmt = conn.prepare(
            "SELECT DISTINCT a2.route_identifier, a2.seqno, a2.waypoint_identifier,
                    a2.waypoint_latitude, a2.waypoint_longitude, a2.icao_code,
                    a2.route_type, a2.flightlevel, a2.direction_restriction,
                    a2.minimum_altitude1, a2.minimum_altitude2, a2.maximum_altitude,
                    a2.outbound_course, a2.inbound_course, a2.inbound_distance,
                    a1.seqno as from_seqno
             FROM tbl_enroute_airways a1
             JOIN tbl_enroute_airways a2 ON a1.route_identifier = a2.route_identifier
             WHERE a1.waypoint_identifier = ?1 
               AND a2.seqno > a1.seqno
             ORDER BY a1.route_identifier, (a2.seqno - a1.seqno)
             LIMIT 100"
        )?;

        let segments = stmt
            .query_map(params![waypoint_identifier], |row| {
                let route_type = match row.get::<_, Option<String>>(6)?.as_deref() {
                    Some("H") => RouteType::High,
                    Some("L") => RouteType::Low,
                    Some("R") => RouteType::RNAV,
                    _ => RouteType::Other,
                };

                let lat: f64 = row.get(3)?;
                let lon: f64 = row.get(4)?;

                Ok(AirwaySegment {
                    route_identifier: row.get::<_, String>(0)?,
                    sequence: row.get::<_, i32>(1)?,
                    waypoint: Waypoint {
                        identifier: row.get::<_, String>(2)?,
                        coordinate: Coordinate::new(lat, lon)
                            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        icao_code: row.get::<_, String>(5).unwrap_or_default(),
                        name: None,
                        waypoint_type: WaypointType::Enroute,
                        usage: None,
                        id: None,
                    },
                    route_type,
                    flight_level: row.get::<_, Option<String>>(7)?,
                    direction_restriction: row.get::<_, Option<String>>(8)?,
                    minimum_altitude1: row.get::<_, Option<i32>>(9)?,
                    minimum_altitude2: row.get::<_, Option<i32>>(10)?,
                    maximum_altitude: row.get::<_, Option<i32>>(11)?,
                    outbound_course: row.get::<_, Option<f64>>(12)?,
                    inbound_course: row.get::<_, Option<f64>>(13)?,
                    inbound_distance: row.get::<_, Option<f64>>(14)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(segments)
    }
    
    /// 快速查找机场的SID出口航点（在航路网络中的航点）
    pub fn find_sid_exit_waypoints(&self, airport_icao: &str) -> Result<Vec<Waypoint>> {
        let conn = self.get_connection()?;
        
        let mut stmt = conn.prepare(
            "SELECT DISTINCT s.waypoint_identifier, s.waypoint_latitude, s.waypoint_longitude
             FROM tbl_sids s
             WHERE s.airport_identifier = ?1 
               AND s.waypoint_identifier IS NOT NULL
               AND s.waypoint_identifier IN (
                 SELECT DISTINCT waypoint_identifier FROM tbl_enroute_airways
               )
             LIMIT 20"
        )?;
        
        let waypoints = stmt
            .query_map(params![airport_icao], |row| {
                let lat: f64 = row.get(1)?;
                let lon: f64 = row.get(2)?;
                
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: String::new(),
                    name: None,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Enroute,
                    usage: None,
                    id: None,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        Ok(waypoints)
    }
    
    /// 快速查找机场的STAR入口航点（在航路网络中的航点）
    pub fn find_star_entry_waypoints(&self, airport_icao: &str) -> Result<Vec<Waypoint>> {
        let conn = self.get_connection()?;
        
        let mut stmt = conn.prepare(
            "SELECT DISTINCT s.waypoint_identifier, s.waypoint_latitude, s.waypoint_longitude
             FROM tbl_stars s
             WHERE s.airport_identifier = ?1 
               AND s.waypoint_identifier IS NOT NULL
               AND s.waypoint_identifier IN (
                 SELECT DISTINCT waypoint_identifier FROM tbl_enroute_airways
               )
             LIMIT 20"
        )?;
        
        let waypoints = stmt
            .query_map(params![airport_icao], |row| {
                let lat: f64 = row.get(1)?;
                let lon: f64 = row.get(2)?;
                
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: String::new(),
                    name: None,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Enroute,
                    usage: None,
                    id: None,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        Ok(waypoints)
    }

    /// 批量加载整个航路网络的边（用于Dijkstra算法图构建）
    /// 返回: Vec<(航路名, 起点标识符, 起点坐标, 终点标识符, 终点坐标)>
    pub fn load_all_airway_edges(&self) -> Result<Vec<(String, String, Coordinate, String, Coordinate)>> {
        let conn = self.get_connection()?;
        
        // 加载所有航路航点，按航路和seqno排序
        let mut stmt = conn.prepare(
            "SELECT route_identifier, waypoint_identifier, waypoint_latitude, waypoint_longitude, seqno
             FROM tbl_enroute_airways
             ORDER BY route_identifier, seqno"
        )?;
        
        // 收集所有航点
        let waypoints: Vec<(String, String, f64, f64, i32)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, i32>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        // 在Rust中构建边（相同航路的连续航点形成边）
        let mut edges = Vec::new();
        let mut prev: Option<(String, String, f64, f64)> = None;
        
        for (route_id, wp_id, lat, lon, _seqno) in waypoints {
            if let Some((prev_route, prev_wp, prev_lat, prev_lon)) = &prev {
                if prev_route == &route_id {
                    // 相同航路，形成边
                    let coord1 = Coordinate::new(*prev_lat, *prev_lon)?;
                    let coord2 = Coordinate::new(lat, lon)?;
                    edges.push((route_id.clone(), prev_wp.clone(), coord1, wp_id.clone(), coord2));
                }
            }
            prev = Some((route_id, wp_id, lat, lon));
        }
        
        Ok(edges)
    }

    /// 加载所有航路航点（仅标识符和坐标，用于快速图构建）
    pub fn load_all_airway_waypoints(&self) -> Result<Vec<(String, Coordinate)>> {
        let conn = self.get_connection()?;
        
        let mut stmt = conn.prepare(
            "SELECT DISTINCT waypoint_identifier, waypoint_latitude, waypoint_longitude
             FROM tbl_enroute_airways"
        )?;
        
        let waypoints = stmt
            .query_map([], |row| {
                let lat: f64 = row.get(1)?;
                let lon: f64 = row.get(2)?;
                
                let coord = Coordinate::new(lat, lon)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                
                Ok((row.get::<_, String>(0)?, coord))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        
        Ok(waypoints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_pool_creation() {
        if std::path::Path::new("raw_data/e_dfd_PMDG.s3db").exists() {
            let pool = DatabasePool::new("raw_data/e_dfd_PMDG.s3db", 5);
            assert!(pool.is_ok());
        }
    }
}
