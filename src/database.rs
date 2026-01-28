use crate::error::{Result, RouteKitError};
use crate::models::*;
use parking_lot::Mutex;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::Path;
use std::sync::Arc;

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
                Ok(Waypoint {
                    identifier: row.get(0)?,
                    icao_code: row.get(1)?,
                    name: row.get(2)?,
                    coordinate: Coordinate::new(
                        row.get::<_, f64>(3)?,
                        row.get::<_, f64>(4)?,
                    ).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Enroute,
                    usage: row.get(6)?,
                    id: row.get(7)?,
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
                Ok(Waypoint {
                    identifier: row.get(0)?,
                    icao_code: row.get(1)?,
                    name: row.get(2)?,
                    coordinate: Coordinate::new(
                        row.get::<_, f64>(3)?,
                        row.get::<_, f64>(4)?,
                    ).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Enroute,
                    usage: row.get(6)?,
                    id: row.get(7)?,
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
                Ok(Waypoint {
                    identifier: row.get(0)?,
                    icao_code: row.get(1)?,
                    name: row.get(2)?,
                    coordinate: Coordinate::new(
                        row.get::<_, f64>(3)?,
                        row.get::<_, f64>(4)?,
                    ).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::Terminal,
                    usage: None,
                    id: row.get(5)?,
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

                Ok(AirwaySegment {
                    route_identifier: row.get(0)?,
                    sequence: row.get(1)?,
                    waypoint: Waypoint {
                        identifier: row.get(2)?,
                        coordinate: Coordinate::new(
                            row.get::<_, f64>(3)?,
                            row.get::<_, f64>(4)?,
                        ).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                        icao_code: row.get(5)?,
                        name: None,
                        waypoint_type: WaypointType::Enroute,
                        usage: None,
                        id: None,
                    },
                    route_type,
                    flight_level: row.get(7)?,
                    direction_restriction: row.get(8)?,
                    minimum_altitude1: row.get(9)?,
                    minimum_altitude2: row.get(10)?,
                    maximum_altitude: row.get(11)?,
                    outbound_course: row.get(12)?,
                    inbound_course: row.get(13)?,
                    inbound_distance: row.get(14)?,
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
             WHERE airport_identifier = ?1"
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
        let query = if let Some(trans) = transition_id {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_sids 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier = ?3
             ORDER BY seqno"
        } else {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_sids 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier IS NULL
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
             WHERE airport_identifier = ?1"
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
        let query = if let Some(trans) = transition_id {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_stars 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier = ?3
             ORDER BY seqno"
        } else {
            "SELECT seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude,
                    path_termination, magnetic_course, route_distance_holding_distance_time,
                    altitude_description, altitude1, altitude2, speed_limit
             FROM tbl_stars 
             WHERE airport_identifier = ?1 AND procedure_identifier = ?2 AND transition_identifier IS NULL
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
