use crate::database::DatabasePool;
use crate::error::{Result, RouteKitError};
use crate::geo::{
    bearing_turn_angle, calculate_bearing, calculate_destination, haversine_distance_nm,
};
use crate::models::*;
use crate::network::{Edge, HeightBand, Network, Record};
use crate::planning::ConnectionClass;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetourBudget {
    pub ratio: f64,
    pub allowance_nm: f64,
}
impl Default for DetourBudget {
    fn default() -> Self {
        Self {
            ratio: 1.5,
            allowance_nm: 200.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PathRole {
    Oceanic,
    Transition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum OceanicSource {
    Simulation {
        provenance: String,
    },
    Snapshot {
        provenance: String,
        valid_from_unix: i64,
        valid_until_unix: i64,
    },
}

/// One directed, indivisible path. Shared endpoints allow explicit connections;
/// intermediate coordinates never become junctions or authorize partial track use.
/// Supplemental paths belong to the high enroute network; Custom FL requests
/// are checked against their explicit minimum/maximum flight levels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OceanicPath {
    pub identifier: String,
    pub source: OceanicSource,
    pub role: PathRole,
    pub from: Waypoint,
    pub to: Waypoint,
    pub via: Vec<Coordinate>,
    pub minimum_fl: i32,
    pub maximum_fl: i32,
    /// East-positive declination supplied by the caller. Constant along this path;
    /// used only to estimate the legacy magnetic_course output for supplemental legs.
    pub magnetic_variation_deg: f64,
}

impl OceanicPath {
    /// Offline baseline only. Endpoints must be selected explicitly by the caller.
    /// Rounding to whole minutes makes the generated coordinate string lossless.
    pub fn simulation(from: Waypoint, to: Waypoint, spacing_nm: f64) -> Result<Self> {
        Coordinate::new(from.coordinate.latitude, from.coordinate.longitude)?;
        Coordinate::new(to.coordinate.latitude, to.coordinate.longitude)?;
        let distance = haversine_distance_nm(&from.coordinate, &to.coordinate);
        if !spacing_nm.is_finite()
            || !(10.0..=600.0).contains(&spacing_nm)
            || !(0.1..10_800.0).contains(&distance)
        {
            return Err(invalid(
                "Invalid simulation spacing or ambiguous/degenerate endpoints",
            ));
        }
        let count = (distance / spacing_nm).ceil() as usize;
        if count > 127 {
            return Err(invalid("Simulation exceeds 128 point budget"));
        }
        let bearing = calculate_bearing(&from.coordinate, &to.coordinate);
        let via = (1..count)
            .map(|i| {
                let c = calculate_destination(
                    &from.coordinate,
                    distance * i as f64 / count as f64,
                    bearing,
                )?;
                Coordinate::new(
                    (c.latitude * 60.0).round() / 60.0,
                    (c.longitude * 60.0).round() / 60.0,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            identifier: format!("SIM-{}-{}", from.identifier, to.identifier),
            source: OceanicSource::Simulation {
                provenance: "Offline great-circle baseline; caller-selected endpoints".into(),
            },
            role: PathRole::Oceanic,
            from,
            to,
            via,
            minimum_fl: 290,
            maximum_fl: 410,
            magnetic_variation_deg: 0.0,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchOptions {
    /// Product quality bound, not an operational aviation rule. None opts out.
    pub detour_budget: Option<DetourBudget>,
    pub oceanic_paths: Vec<OceanicPath>,
    pub allow_simulation: bool,
    /// Caller-defined reference time at which snapshot applicability is evaluated.
    /// All used snapshots must remain valid for the entire estimated journey.
    pub planning_time_unix: Option<i64>,
    pub cruise_speed_knots: f64,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            detour_budget: Some(DetourBudget::default()),
            oceanic_paths: Vec::new(),
            allow_simulation: false,
            planning_time_unix: None,
            cruise_speed_knots: 450.0,
        }
    }
}
impl SearchOptions {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.oceanic_paths.len() > 64
            || !self.cruise_speed_knots.is_finite()
            || !(50.0..=1000.0).contains(&self.cruise_speed_knots)
        {
            return Err(invalid(
                "At most 64 supplemental paths and a speed of 50..=1000 kt are supported",
            ));
        }
        if self.detour_budget.as_ref().is_some_and(|b| {
            !b.ratio.is_finite()
                || b.ratio < 1.0
                || !b.allowance_nm.is_finite()
                || b.allowance_nm < 0.0
        }) {
            return Err(invalid(
                "Detour ratio must be >= 1 and allowance must be non-negative and finite",
            ));
        }
        Ok(())
    }
    pub(crate) fn limit(&self, from: &Coordinate, to: &Coordinate) -> Result<Option<f64>> {
        self.detour_budget
            .as_ref()
            .map(|b| {
                let limit = b.ratio * haversine_distance_nm(from, to) + b.allowance_nm;
                if !limit.is_finite() {
                    return Err(invalid("Detour limit overflow"));
                }
                Ok(limit)
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsedOceanicPath {
    pub identifier: String,
    pub source: OceanicSource,
    pub role: PathRole,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteDiagnostics {
    pub proxy_total_distance_nm: f64,
    pub detour_limit_nm: Option<f64>,
    pub used_oceanic_paths: Vec<UsedOceanicPath>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteSearchResult {
    pub routes: Vec<Route>,
    /// Same order as routes; legacy Route/RouteResult serialization is unchanged.
    pub diagnostics: Vec<RouteDiagnostics>,
}

#[derive(Clone)]
pub(crate) struct ResolvedPath {
    pub spec: OceanicPath,
    pub points: Vec<Waypoint>,
    pub internal_turn_cost: f64,
    pub class: ConnectionClass,
    pub region: Option<String>,
    pub region_valid_until: Option<i64>,
}
fn invalid(message: &str) -> RouteKitError {
    RouteKitError::ConfigError(message.into())
}

/// ICAO degree-only or degree/minute coordinate tokens; no ambiguous five-letter
/// ARINC oceanic shorthand, slash forms, or silent minute overflow.
pub(crate) fn coordinate_waypoint(token: &str) -> Option<Waypoint> {
    let (lat_end, lon_start, lon_end, minute) = match token.len() {
        7 => (2, 3, 6, false),
        11 => (4, 5, 10, true),
        _ => return None,
    };
    if !token.is_ascii() {
        return None;
    }
    let b = token.as_bytes();
    if !matches!(b[lat_end], b'N' | b'S') || !matches!(b[lon_end], b'E' | b'W') {
        return None;
    }
    let lat = &token[..lat_end];
    let lon = &token[lon_start..lon_end];
    if !lat.bytes().chain(lon.bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let number = |s: &str, degrees: usize| -> Option<f64> {
        let d: f64 = s[..degrees].parse().ok()?;
        let m: f64 = if minute {
            s[degrees..].parse().ok()?
        } else {
            0.0
        };
        (m < 60.0).then_some(d + m / 60.0)
    };
    let latitude = number(lat, 2)? * if b[lat_end] == b'S' { -1.0 } else { 1.0 };
    let longitude = number(lon, 3)? * if b[lon_end] == b'W' { -1.0 } else { 1.0 };
    let mut p = Waypoint::simple(token, latitude, longitude).ok()?;
    p.waypoint_type = WaypointType::Other("OceanicCoordinate".into());
    Some(p)
}
fn minute_waypoint(c: &Coordinate) -> Result<Waypoint> {
    Coordinate::new(c.latitude, c.longitude)?;
    let lat = (c.latitude.abs() * 60.0).round() as u32;
    let lon = (c.longitude.abs() * 60.0).round() as u32;
    let token = format!(
        "{:02}{:02}{}{:03}{:02}{}",
        lat / 60,
        lat % 60,
        if c.latitude < 0.0 { 'S' } else { 'N' },
        lon / 60,
        lon % 60,
        if c.longitude < 0.0 { 'W' } else { 'E' }
    );
    let p = coordinate_waypoint(&token).ok_or_else(|| invalid("Invalid oceanic coordinate"))?;
    if haversine_distance_nm(c, &p.coordinate) > 1e-6 {
        return Err(invalid("Supplemental coordinates must use whole minutes; precision is never silently discarded"));
    }
    Ok(p)
}

pub(crate) fn prepare(db: &DatabasePool, options: &SearchOptions) -> Result<Vec<ResolvedPath>> {
    let mut paths = Vec::new();
    let mut names = HashSet::new();
    for p in &options.oceanic_paths {
        let provenance = match &p.source {
            OceanicSource::Simulation { provenance } => {
                if !options.allow_simulation {
                    return Err(invalid("Simulation requires allow_simulation=true"));
                }
                provenance
            }
            OceanicSource::Snapshot {
                provenance,
                valid_from_unix,
                valid_until_unix,
            } => {
                if valid_from_unix >= valid_until_unix {
                    return Err(invalid("Invalid snapshot validity interval"));
                }
                let time = options
                    .planning_time_unix
                    .ok_or_else(|| invalid("Snapshot paths require planning_time_unix"))?;
                if !(*valid_from_unix..*valid_until_unix).contains(&time) {
                    return Err(invalid(
                        "Snapshot is not valid at the requested planning time",
                    ));
                }
                provenance
            }
        };
        if p.identifier.trim().is_empty()
            || !names.insert(p.identifier.clone())
            || provenance.trim().is_empty()
            || p.via.len() > 126
            || p.minimum_fl < 1
            || p.maximum_fl > 1000
            || p.minimum_fl > p.maximum_fl
            || !p.magnetic_variation_deg.is_finite()
            || p.magnetic_variation_deg.abs() > 180.0
        {
            return Err(invalid(
                "Invalid path identifier, provenance, geometry, altitude or magnetic variation",
            ));
        }
        let resolve = |point: &Waypoint| -> Result<Waypoint> {
            Coordinate::new(point.coordinate.latitude, point.coordinate.longitude)?;
            let points = db.find_waypoints(&point.identifier)?;
            points
                .into_iter()
                .find(|q| {
                    q.icao_code == point.icao_code
                        && haversine_distance_nm(&q.coordinate, &point.coordinate) < 1e-6
                        && match (&q.id, &point.id) {
                            (Some(a), Some(b)) if a.contains('|') && b.contains('|') => a == b,
                            (Some(a), Some(b)) => a.rsplit('|').next() == b.rsplit('|').next(),
                            _ => true,
                        }
                })
                .ok_or_else(|| {
                    invalid("Supplemental endpoints must reference exact database fixes")
                })
        };
        let from = resolve(&p.from)?;
        let to = resolve(&p.to)?;
        let mut points = vec![from];
        for c in &p.via {
            points.push(minute_waypoint(c)?);
        }
        points.push(to);
        if points
            .windows(2)
            .any(|w| haversine_distance_nm(&w[0].coordinate, &w[1].coordinate) < 0.01)
        {
            return Err(invalid("Supplemental path contains a degenerate leg"));
        }
        let mut identities = HashSet::new();
        for point in &points {
            if !identities.insert((
                (point.coordinate.latitude * 1e7).round() as i64,
                (point.coordinate.longitude * 1e7).round() as i64,
            )) {
                return Err(invalid("Supplemental path repeats a physical point"));
            }
        }
        let internal_turn_cost = points
            .windows(3)
            .map(|w| {
                let arriving = (calculate_bearing(&w[1].coordinate, &w[0].coordinate) + 180.0)
                    .rem_euclid(360.0);
                let departing = calculate_bearing(&w[1].coordinate, &w[2].coordinate);
                10.0 * ((bearing_turn_angle(arriving, departing) - 30.0).max(0.0) / 150.0).powi(2)
            })
            .sum();
        paths.push(ResolvedPath {
            spec: p.clone(),
            points,
            internal_turn_cost,
            class: match p.source {
                OceanicSource::Simulation { .. } => ConnectionClass::Simulation,
                OceanicSource::Snapshot { .. } => match p.role {
                    PathRole::Oceanic => ConnectionClass::PublishedTrack,
                    PathRole::Transition => ConnectionClass::PublishedTransition,
                },
            },
            region: None,
            region_valid_until: None,
        });
    }
    Ok(paths)
}

impl Network {
    pub(crate) fn with_supplemental(&self, paths: Vec<ResolvedPath>) -> Result<Self> {
        let mut g = self.clone();
        for mut path in paths {
            let mut endpoint = |point: &Waypoint| -> Result<usize> {
                let matches = g.matches(point);
                if matches.len() > 1 {
                    return Err(invalid("Ambiguous supplemental endpoint"));
                }
                if let Some(&n) = matches.first() {
                    return Ok(n);
                }
                let n = g.nodes.len();
                g.nodes.push(point.clone());
                g.by_name
                    .entry(point.identifier.clone())
                    .or_default()
                    .push(n);
                g.outgoing.push(Vec::new());
                g.incoming.push(Vec::new());
                Ok(n)
            };
            let from = endpoint(path.points.first().unwrap())?;
            let to = endpoint(path.points.last().unwrap())?;
            if from == to {
                return Err(invalid("Supplemental endpoints must differ"));
            }
            path.points[0] = g.nodes[from].clone();
            let last_index = path.points.len() - 1;
            path.points[last_index] = g.nodes[to].clone();
            let distances: Vec<_> = path
                .points
                .windows(2)
                .map(|w| haversine_distance_nm(&w[0].coordinate, &w[1].coordinate))
                .collect();
            let first = &path.points[0].coordinate;
            let second = &path.points[1].coordinate;
            let last = &path.points[path.points.len() - 1].coordinate;
            let penultimate = &path.points[path.points.len() - 2].coordinate;
            let initial = calculate_bearing(first, second);
            let record = g.records.len();
            let airway = g.airways.len();
            g.airways.push(path.spec.identifier.clone());
            g.records.push(Record {
                node: from,
                airway,
                section: record,
                seq: 0,
                level: "H".into(),
                direction: "F".into(),
                kind: "X".into(),
                min1: Some(path.spec.minimum_fl * 100),
                min2: None,
                max: Some(path.spec.maximum_fl * 100),
                outbound: None,
                inbound: None,
                published_distance: None,
                end: true,
            });
            let id = g.edges.len();
            g.outgoing[from].push(id);
            g.incoming[to].push(id);
            g.edges.push(Edge {
                from,
                to,
                record,
                reverse: false,
                distance: distances.iter().sum(),
                initial,
                final_course: (calculate_bearing(last, penultimate) + 180.0).rem_euclid(360.0),
                magnetic: Some((initial - path.spec.magnetic_variation_deg).rem_euclid(360.0)),
                height: HeightBand {
                    low: path.spec.minimum_fl * 100,
                    high: path.spec.maximum_fl * 100,
                },
                minimum: Some(path.spec.minimum_fl * 100),
                direct: true,
            });
            g.supplemental.insert(id, path);
        }
        Ok(g)
    }
}
