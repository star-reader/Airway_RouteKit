use crate::error::{Result, RouteKitError};
use crate::models::*;
use crate::network::Network;
use parking_lot::RwLock;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OptionalExtension, Row};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub struct DatabasePool {
    pool: Pool<SqliteConnectionManager>,
    network: RwLock<Option<Arc<Network>>>,
    gateways: RwLock<HashMap<(String, bool), Vec<Waypoint>>>,
    waypoint_cache: RwLock<HashMap<String, Vec<Waypoint>>>,
}

impl DatabasePool {
    pub fn new<P: AsRef<Path>>(db_path: P, pool_size: u32) -> Result<Self> {
        if pool_size == 0 {
            return Err(RouteKitError::ConfigError(
                "Database pool size must be positive".into(),
            ));
        }
        std::fs::metadata(db_path.as_ref())?;
        let manager = SqliteConnectionManager::file(db_path).with_flags(
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        );
        let pool = Pool::builder().max_size(pool_size).build(manager)?;

        Ok(Self {
            pool,
            network: RwLock::new(None),
            gateways: RwLock::new(HashMap::new()),
            waypoint_cache: RwLock::new(HashMap::new()),
        })
    }

    fn get_connection(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool.get().map_err(|e| e.into())
    }

    pub(crate) fn network(&self) -> Result<Arc<Network>> {
        if let Some(graph) = self.network.read().as_ref() {
            return Ok(Arc::clone(graph));
        }
        let mut slot = self.network.write();
        if let Some(graph) = slot.as_ref() {
            return Ok(Arc::clone(graph));
        }
        let conn = self.get_connection()?;
        let graph = Arc::new(Network::load(&conn)?);
        *slot = Some(Arc::clone(&graph));
        Ok(graph)
    }

    pub(crate) fn invalidate(&self) {
        *self.network.write() = None;
        self.gateways.write().clear();
        self.waypoint_cache.write().clear();
    }

    fn airport_gateways(&self, airport: &str, departure: bool) -> Result<Vec<Waypoint>> {
        let key = (airport.trim().to_uppercase(), departure);
        if let Some(points) = self.gateways.read().get(&key) {
            return Ok(points.clone());
        }
        let conn = self.get_connection()?;
        let points = crate::gateways::load(&conn, &key.0, departure)?;
        self.gateways.write().insert(key, points.clone());
        Ok(points)
    }

    pub(crate) fn procedure_exists(
        &self,
        airport: &str,
        procedure: &str,
        departure: bool,
    ) -> Result<bool> {
        let table = if departure { "tbl_sids" } else { "tbl_stars" };
        let conn = self.get_connection()?;
        Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE airport_identifier=?1 AND procedure_identifier=?2)"),
            params![airport,procedure], |r|r.get(0))?)
    }

    pub fn load_airport(&self, icao: &str) -> Result<Airport> {
        let icao = icao.trim().to_uppercase();
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT icao_code, airport_identifier, airport_identifier_3letter, 
                    airport_name, airport_ref_latitude, airport_ref_longitude,
                    ifr_capability, elevation, transition_altitude, transition_level
             FROM tbl_airports 
             WHERE airport_identifier = ?1",
        )?;

        let airport = stmt
            .query_row(params![icao], |row| {
                Ok(Airport {
                    icao_code: row.get(0)?,
                    identifier: row.get(1)?,
                    identifier_3letter: row.get(2)?,
                    name: row.get(3)?,
                    coordinate: Coordinate::new(row.get::<_, f64>(4)?, row.get::<_, f64>(5)?)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
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
             FROM tbl_enroute_waypoints",
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
        let candidates = self.find_waypoints(identifier)?;
        Ok(candidates.into_iter().next())
    }

    pub fn find_waypoints(&self, identifier: &str) -> Result<Vec<Waypoint>> {
        let identifier = identifier.trim().to_uppercase();
        if let Some(points) = self.waypoint_cache.read().get(&identifier) {
            return Ok(points.clone());
        }
        let graph = self.network()?;
        let conn = self.get_connection()?;

        let mut waypoints = Vec::new();

        // 先在航路航点中查找（可能存在重名航点）
        let mut stmt = conn.prepare(
            "SELECT waypoint_identifier, icao_code, waypoint_name, 
                    waypoint_latitude, waypoint_longitude, waypoint_type, 
                    waypoint_usage, id
             FROM tbl_enroute_waypoints 
             WHERE waypoint_identifier = ?1",
        )?;

        let enroute_waypoints = stmt
            .query_map(params![identifier], |row| {
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

        waypoints.extend(enroute_waypoints);

        // 在终端区航点中查找（补充候选）
        let mut stmt = conn.prepare(
            "SELECT waypoint_identifier, icao_code, waypoint_name, 
                    waypoint_latitude, waypoint_longitude, id
             FROM tbl_terminal_waypoints 
             WHERE waypoint_identifier = ?1",
        )?;

        let terminal_waypoints = stmt
            .query_map(params![identifier], |row| {
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
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        waypoints.extend(terminal_waypoints);

        // VOR/VHF 台站（真实表：tbl_vhfnavaids；兼容旧表名：tbl_vor）
        self.extend_waypoints_from_vhf_table(&conn, &mut waypoints, "tbl_vhfnavaids", &identifier)?;
        self.extend_waypoints_from_vhf_table(&conn, &mut waypoints, "tbl_vor", &identifier)?;

        // NDB 台站（真实表：tbl_*_ndbnavaids；兼容旧表名：tbl_*_ndb）
        self.extend_waypoints_from_ndb_table(
            &conn,
            &mut waypoints,
            "tbl_enroute_ndbnavaids",
            &identifier,
        )?;
        self.extend_waypoints_from_ndb_table(
            &conn,
            &mut waypoints,
            "tbl_terminal_ndbnavaids",
            &identifier,
        )?;
        self.extend_waypoints_from_ndb_table(
            &conn,
            &mut waypoints,
            "tbl_enroute_ndb",
            &identifier,
        )?;
        self.extend_waypoints_from_ndb_table(
            &conn,
            &mut waypoints,
            "tbl_terminal_ndb",
            &identifier,
        )?;
        for &n in graph.by_name.get(&identifier).into_iter().flatten() {
            let wp = &graph.nodes[n];
            if !waypoints.iter().any(|p| {
                p.icao_code == wp.icao_code
                    && crate::geo::haversine_distance_nm(&p.coordinate, &wp.coordinate) < 0.05
            }) {
                waypoints.push(wp.clone());
            }
        }
        waypoints.sort_by(|a, b| {
            a.icao_code
                .cmp(&b.icao_code)
                .then(a.coordinate.latitude.total_cmp(&b.coordinate.latitude))
                .then(a.coordinate.longitude.total_cmp(&b.coordinate.longitude))
                .then(a.id.cmp(&b.id))
        });
        let mut cache = self.waypoint_cache.write();
        if cache.len() >= 4096 {
            cache.clear();
        }
        cache.insert(identifier, waypoints.clone());
        Ok(waypoints)
    }

    fn extend_waypoints_from_vhf_table(
        &self,
        conn: &r2d2::PooledConnection<SqliteConnectionManager>,
        waypoints: &mut Vec<Waypoint>,
        table_name: &str,
        identifier: &str,
    ) -> Result<()> {
        let query = format!(
            "SELECT vor_identifier, icao_code, vor_name, vor_latitude, vor_longitude, id
             FROM {table}
             WHERE vor_identifier = ?1",
            table = table_name
        );

        let mut stmt = match conn.prepare(&query) {
            Ok(stmt) => stmt,
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("no such table") {
                    return Ok(());
                }
                return Err(e.into());
            }
        };

        let found = stmt
            .query_map(params![identifier], |row| {
                let lat: f64 = row.get(3)?;
                let lon: f64 = row.get(4)?;
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: row.get::<_, String>(1).unwrap_or_default(),
                    name: row.get::<_, Option<String>>(2)?,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::VOR,
                    usage: None,
                    id: row.get::<_, Option<String>>(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        waypoints.extend(found);
        Ok(())
    }

    fn extend_waypoints_from_ndb_table(
        &self,
        conn: &r2d2::PooledConnection<SqliteConnectionManager>,
        waypoints: &mut Vec<Waypoint>,
        table_name: &str,
        identifier: &str,
    ) -> Result<()> {
        let query = format!(
            "SELECT ndb_identifier, icao_code, ndb_name, ndb_latitude, ndb_longitude, id
             FROM {table}
             WHERE ndb_identifier = ?1",
            table = table_name
        );

        let mut stmt = match conn.prepare(&query) {
            Ok(stmt) => stmt,
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("no such table") {
                    return Ok(());
                }
                return Err(e.into());
            }
        };

        let found = stmt
            .query_map(params![identifier], |row| {
                let lat: f64 = row.get(3)?;
                let lon: f64 = row.get(4)?;
                Ok(Waypoint {
                    identifier: row.get::<_, String>(0)?,
                    icao_code: row.get::<_, String>(1).unwrap_or_default(),
                    name: row.get::<_, Option<String>>(2)?,
                    coordinate: Coordinate::new(lat, lon)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                    waypoint_type: WaypointType::NDB,
                    usage: None,
                    id: row.get::<_, Option<String>>(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        waypoints.extend(found);
        Ok(())
    }

    /// 查找航路段
    pub fn find_airway_segments(&self, route_identifier: &str) -> Result<Vec<AirwaySegment>> {
        let graph = self.network()?;
        Ok(graph
            .airway_rows
            .get(&route_identifier.trim().to_uppercase())
            .into_iter()
            .flatten()
            .map(|&r| graph.segment(r))
            .collect())
    }

    /// 查找机场的SID
    pub fn find_sids(&self, airport_icao: &str) -> Result<Vec<Sid>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT procedure_identifier, transition_identifier
             FROM tbl_sids 
             WHERE airport_identifier = ?1
             ORDER BY procedure_identifier, transition_identifier",
        )?;

        let mut sids = Vec::new();
        let rows = stmt
            .query_map(params![airport_icao], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);

        for row in rows {
            let (procedure_id, transition_id) = row;
            let waypoints =
                self.load_sid_waypoints(airport_icao, &procedure_id, transition_id.as_deref())?;
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
            stmt.query_map(
                params![airport_icao, procedure_id, trans],
                Self::parse_sid_star_waypoint,
            )?
        } else {
            stmt.query_map(
                params![airport_icao, procedure_id],
                Self::parse_sid_star_waypoint,
            )?
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
             ORDER BY procedure_identifier, transition_identifier",
        )?;

        let mut stars = Vec::new();
        let rows = stmt
            .query_map(params![airport_icao], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);

        for row in rows {
            let (procedure_id, transition_id) = row;
            let waypoints =
                self.load_star_waypoints(airport_icao, &procedure_id, transition_id.as_deref())?;
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
            stmt.query_map(
                params![airport_icao, procedure_id, trans],
                Self::parse_sid_star_waypoint,
            )?
        } else {
            stmt.query_map(
                params![airport_icao, procedure_id],
                Self::parse_sid_star_waypoint,
            )?
        }
        .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(waypoints)
    }

    /// 解析SID/STAR航点
    fn parse_sid_star_waypoint(row: &Row) -> rusqlite::Result<SidStarWaypoint> {
        Ok(SidStarWaypoint {
            sequence: row.get(0)?,
            waypoint_identifier: row.get(1)?,
            coordinate: Coordinate::new(row.get::<_, f64>(2)?, row.get::<_, f64>(3)?)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
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
        let graph = self.network()?;
        let from = self.find_waypoints(from_waypoint)?;
        let to = self.find_waypoints(to_waypoint)?;
        let mut names = Vec::new();
        for name in graph.airways.iter() {
            if from
                .iter()
                .any(|a| to.iter().any(|b| !graph.expand(name, a, b).is_empty()))
            {
                names.push(name.clone());
            }
        }
        Ok(names)
    }

    pub fn find_airway_waypoints_near(
        &self,
        latitude: f64,
        longitude: f64,
        radius_degrees: f64,
    ) -> Result<Vec<Waypoint>> {
        let origin = Coordinate::new(latitude, longitude)?;
        let graph = self.network()?;
        let mut points: Vec<_> = graph
            .nodes
            .iter()
            .filter_map(|p| {
                let d = crate::geo::haversine_distance_nm(&origin, &p.coordinate);
                (d <= radius_degrees * 60.0).then_some((d, p))
            })
            .collect();
        points.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.identifier.cmp(&b.1.identifier))
        });
        Ok(points.into_iter().map(|(_, p)| p.clone()).collect())
    }

    pub fn find_outbound_airway_segments(
        &self,
        waypoint_identifier: &str,
    ) -> Result<Vec<AirwaySegment>> {
        let graph = self.network()?;
        Ok(graph
            .by_name
            .get(waypoint_identifier)
            .into_iter()
            .flatten()
            .flat_map(|&n| graph.outgoing[n].iter())
            .map(|&e| {
                let edge = &graph.edges[e];
                graph.segment(edge.record + usize::from(!edge.reverse))
            })
            .collect())
    }

    pub fn find_sid_exit_waypoints(&self, airport_icao: &str) -> Result<Vec<Waypoint>> {
        self.airport_gateways(airport_icao, true)
    }

    pub fn find_star_entry_waypoints(&self, airport_icao: &str) -> Result<Vec<Waypoint>> {
        self.airport_gateways(airport_icao, false)
    }

    #[allow(clippy::type_complexity)] // Preserve the existing public signature.
    pub fn load_all_airway_edges(
        &self,
    ) -> Result<Vec<(String, String, Coordinate, String, Coordinate)>> {
        let graph = self.network()?;
        Ok(graph
            .edges
            .iter()
            .map(|e| {
                let a = &graph.nodes[e.from];
                let b = &graph.nodes[e.to];
                (
                    graph.airways[graph.records[e.record].airway].clone(),
                    a.identifier.clone(),
                    a.coordinate,
                    b.identifier.clone(),
                    b.coordinate,
                )
            })
            .collect())
    }

    pub fn load_all_airway_waypoints(&self) -> Result<Vec<(String, Coordinate)>> {
        Ok(self
            .network()?
            .nodes
            .iter()
            .map(|p| (p.identifier.clone(), p.coordinate))
            .collect())
    }
}
