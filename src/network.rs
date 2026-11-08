//! Shared directed navigation graph. No proximity-generated links are added.
use crate::error::{Result, RouteKitError};
use crate::geo::{calculate_bearing, haversine_distance_nm};
use crate::models::*;
use rusqlite::Connection;
use std::collections::{BTreeMap, HashMap};
use std::ops::Index;
use std::sync::Arc;

/// Immutable database payload plus a small query-local suffix. Cloning an
/// oceanic overlay shares the large payload instead of duplicating all strings.
#[derive(Clone)]
pub(crate) struct SharedVec<T> {
    base: Arc<Vec<T>>,
    extra: Vec<T>,
}
impl<T> Default for SharedVec<T> {
    fn default() -> Self {
        Self {
            base: Arc::new(Vec::new()),
            extra: Vec::new(),
        }
    }
}
impl<T> From<Vec<T>> for SharedVec<T> {
    fn from(base: Vec<T>) -> Self {
        Self {
            base: Arc::new(base),
            extra: Vec::new(),
        }
    }
}
impl<T> SharedVec<T> {
    pub fn len(&self) -> usize {
        self.base.len() + self.extra.len()
    }
    pub fn push(&mut self, value: T) {
        self.extra.push(value);
    }
    pub fn last(&self) -> Option<&T> {
        self.extra.last().or_else(|| self.base.last())
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.base.iter().chain(self.extra.iter())
    }
    fn freeze(&mut self) {
        debug_assert!(self.base.is_empty());
        self.base = Arc::new(std::mem::take(&mut self.extra));
    }
}
impl<T> Index<usize> for SharedVec<T> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        if index < self.base.len() {
            &self.base[index]
        } else {
            &self.extra[index - self.base.len()]
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HeightBand {
    pub low: i32,
    pub high: i32,
}

impl HeightBand {
    pub const ALL: Self = Self {
        low: 0,
        high: 100_000,
    };
    pub fn intersect(self, other: Self) -> Option<Self> {
        let band = Self {
            low: self.low.max(other.low),
            high: self.high.min(other.high),
        };
        (band.low <= band.high).then_some(band)
    }
    pub fn contains(self, other: Self) -> bool {
        self.low <= other.low && self.high >= other.high
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub node: usize,
    pub airway: usize,
    pub section: usize,
    pub seq: i32,
    pub level: String,
    pub direction: String,
    pub kind: String,
    pub min1: Option<i32>,
    pub min2: Option<i32>,
    pub max: Option<i32>,
    pub outbound: Option<f64>,
    pub inbound: Option<f64>,
    pub published_distance: Option<f64>,
    pub end: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Edge {
    pub from: usize,
    pub to: usize,
    pub record: usize,
    pub reverse: bool,
    pub distance: f64,
    pub initial: f64,
    pub final_course: f64,
    pub magnetic: Option<f64>,
    pub height: HeightBand,
    pub minimum: Option<i32>,
    pub direct: bool,
}

#[derive(Clone)]
pub(crate) struct Network {
    pub nodes: SharedVec<Waypoint>,
    pub edges: SharedVec<Edge>,
    pub outgoing: Vec<Vec<usize>>,
    pub incoming: Vec<Vec<usize>>,
    pub records: SharedVec<Record>,
    pub airways: SharedVec<String>,
    pub airway_rows: HashMap<String, Vec<usize>>,
    pub by_name: HashMap<String, Vec<usize>>,
    pub supplemental: HashMap<usize, crate::oceanic::ResolvedPath>,
}

fn invalid(message: impl Into<String>) -> RouteKitError {
    RouteKitError::InvalidSegment(message.into())
}

impl Network {
    pub fn load(conn: &Connection) -> Result<Self> {
        let mut graph = Self {
            nodes: SharedVec::default(),
            edges: SharedVec::default(),
            outgoing: Vec::new(),
            incoming: Vec::new(),
            records: SharedVec::default(),
            airways: SharedVec::default(),
            airway_rows: HashMap::new(),
            by_name: HashMap::new(),
            supplemental: HashMap::new(),
        };
        let mut identities: HashMap<String, usize> = HashMap::new();
        let mut names = BTreeMap::<String, usize>::new();
        let mut stmt = conn.prepare(
            "SELECT route_identifier,seqno,waypoint_identifier,icao_code,waypoint_latitude,
             waypoint_longitude,waypoint_description_code,route_type,flightlevel,
             direction_restriction,minimum_altitude1,minimum_altitude2,maximum_altitude,
             outbound_course,inbound_course,inbound_distance,id
             FROM tbl_enroute_airways ORDER BY route_identifier,seqno",
        )?;
        let mut rows = stmt.query([])?;
        let mut section = 0;
        while let Some(row) = rows.next()? {
            let airway: String = row.get(0)?;
            let seq: i32 = row.get(1)?;
            let ident: String = row.get(2)?;
            let region: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
            let coord = Coordinate::new(row.get(4)?, row.get(5)?)?;
            let description = row.get::<_, Option<String>>(6)?.unwrap_or_default();
            let reference = row.get::<_, Option<String>>(16)?.filter(|s| !s.is_empty());
            let facility = description.as_bytes().first().copied().unwrap_or(b'E');
            let key = reference.clone().unwrap_or_else(|| {
                format!(
                    "{}|{}|{}|{:.7}|{:.7}",
                    region, ident, facility, coord.latitude, coord.longitude
                )
            });
            let node = if let Some(&n) = identities.get(&key) {
                if graph.nodes[n].identifier != ident
                    || graph.nodes[n].icao_code != region
                    || haversine_distance_nm(&graph.nodes[n].coordinate, &coord) > 0.05
                {
                    return Err(invalid(format!("Conflicting navigation identity: {key}")));
                }
                n
            } else {
                let n = graph.nodes.len();
                graph.nodes.push(Waypoint {
                    identifier: ident.clone(),
                    icao_code: region,
                    coordinate: coord,
                    name: None,
                    usage: None,
                    id: reference,
                    waypoint_type: match facility {
                        b'V' => WaypointType::VOR,
                        b'N' => WaypointType::NDB,
                        _ => WaypointType::Enroute,
                    },
                });
                graph.outgoing.push(Vec::new());
                graph.incoming.push(Vec::new());
                graph.by_name.entry(ident).or_default().push(n);
                identities.insert(key, n);
                n
            };
            let airway_id = *names.entry(airway.clone()).or_insert_with(|| {
                let n = graph.airways.len();
                graph.airways.push(airway.clone());
                n
            });
            if let Some(previous) = graph.records.last() {
                if previous.airway == airway_id && previous.seq == seq {
                    return Err(invalid(format!("Duplicate sequence {airway}/{seq}")));
                }
                if previous.airway != airway_id || previous.end {
                    section += 1;
                }
            }
            let record = Record {
                node,
                airway: airway_id,
                section,
                seq,
                kind: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                level: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
                direction: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                min1: row.get(10)?,
                min2: row.get(11)?,
                max: row.get(12)?,
                outbound: row.get(13)?,
                inbound: row.get(14)?,
                published_distance: row.get(15)?,
                end: description.as_bytes().get(1) == Some(&b'E'),
            };
            let i = graph.records.len();
            graph.airway_rows.entry(airway).or_default().push(i);
            graph.records.push(record);
        }
        for i in 0..graph.records.len().saturating_sub(1) {
            let a = &graph.records[i];
            let b = &graph.records[i + 1];
            if a.section != b.section || a.airway != b.airway || a.node == b.node {
                continue;
            }
            // Fixed-wing enroute scope. Unknown route/direction/level codes are not inferred.
            if !matches!(a.kind.as_str(), "O" | "R" | "C" | "D" | "S")
                || !matches!(a.level.as_str(), "H" | "L" | "B")
            {
                continue;
            }
            match a.direction.trim() {
                "F" => graph.add_edge(i, false)?,
                "B" => graph.add_edge(i, true)?,
                "" => {
                    graph.add_edge(i, false)?;
                    graph.add_edge(i, true)?;
                }
                _ => return Err(invalid("Unknown airway direction restriction")),
            }
        }
        graph.nodes.freeze();
        graph.edges.freeze();
        graph.records.freeze();
        graph.airways.freeze();
        Ok(graph)
    }

    fn add_edge(&mut self, record: usize, reverse: bool) -> Result<()> {
        let a = &self.records[record];
        let b = &self.records[record + 1];
        let (from, to) = if reverse {
            (b.node, a.node)
        } else {
            (a.node, b.node)
        };
        let origin = &self.nodes[from].coordinate;
        let destination = &self.nodes[to].coordinate;
        // ARINC 424 5.30: min2 is the reverse-direction MEA, min1 otherwise.
        let minimum = if reverse { a.min2.or(a.min1) } else { a.min1 };
        let height = HeightBand {
            low: minimum.unwrap_or(0),
            high: a.max.unwrap_or(100_000),
        };
        if height.low > height.high {
            log::warn!(
                "Excluded contradictory altitude interval on {}/{} (reverse={reverse})",
                self.airways[a.airway],
                a.seq
            );
            return Ok(());
        }
        let published = if reverse { b.inbound } else { a.outbound };
        let magnetic = published
            .filter(|v| v.is_finite() && (0.0..=360.0).contains(v))
            .map(|v| (v + if reverse { 180.0 } else { 0.0 }).rem_euclid(360.0));
        let distance = haversine_distance_nm(origin, destination);
        let edge = Edge {
            from,
            to,
            record,
            reverse,
            distance,
            magnetic,
            minimum,
            height,
            initial: calculate_bearing(origin, destination),
            final_course: (calculate_bearing(destination, origin) + 180.0).rem_euclid(360.0),
            direct: a.kind == "D",
        };
        let id = self.edges.len();
        self.outgoing[from].push(id);
        self.incoming[to].push(id);
        self.edges.push(edge);
        Ok(())
    }

    pub fn matches(&self, waypoint: &Waypoint) -> Vec<usize> {
        self.by_name
            .get(&waypoint.identifier)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&n| {
                let other = &self.nodes[n];
                (waypoint.icao_code.is_empty() || other.icao_code == waypoint.icao_code)
                    && haversine_distance_nm(&waypoint.coordinate, &other.coordinate) <= 0.05
                    && match (&waypoint.id, &other.id) {
                        (Some(a), Some(b)) if a.contains('|') && b.contains('|') => a == b,
                        _ => true,
                    }
            })
            .collect()
    }

    pub fn level_allowed(&self, edge: &Edge, level: Option<FlightLevel>) -> bool {
        let category = self.records[edge.record].level.as_str();
        match level {
            Some(FlightLevel::High) => matches!(category, "H" | "B"),
            Some(FlightLevel::Low) => matches!(category, "L" | "B"),
            _ => true,
        }
    }

    pub fn segment(&self, row: usize) -> AirwaySegment {
        let r = &self.records[row];
        AirwaySegment {
            route_identifier: self.airways[r.airway].clone(),
            sequence: r.seq,
            waypoint: self.nodes[r.node].clone(),
            route_type: if r.kind == "R" {
                RouteType::RNAV
            } else {
                RouteType::Other
            },
            flight_level: Some(r.level.clone()),
            direction_restriction: (!r.direction.is_empty()).then(|| r.direction.clone()),
            minimum_altitude1: r.min1,
            minimum_altitude2: r.min2,
            maximum_altitude: r.max,
            outbound_course: r.outbound,
            inbound_course: r.inbound,
            inbound_distance: r.published_distance,
        }
    }

    /// Expand only a verified continuous named-airway path; never return an arbitrary suffix.
    pub fn expand(&self, airway: &str, from: &Waypoint, to: &Waypoint) -> Vec<Vec<AirwaySegment>> {
        let Some(rows) = self.airway_rows.get(airway) else {
            return Vec::new();
        };
        let starts = self.matches(from);
        let ends = self.matches(to);
        let mut paths = Vec::new();
        for (i, &a) in rows
            .iter()
            .enumerate()
            .filter(|(_, a)| starts.contains(&self.records[**a].node))
        {
            for (j, &b) in rows
                .iter()
                .enumerate()
                .filter(|(_, b)| ends.contains(&self.records[**b].node))
            {
                if i == j || self.records[a].section != self.records[b].section {
                    continue;
                }
                let reverse = j < i;
                let lo = i.min(j);
                let hi = i.max(j);
                if (lo..hi).any(|p| {
                    !self.outgoing[self.records[rows[if reverse { p + 1 } else { p }]].node]
                        .iter()
                        .any(|&e| {
                            self.edges[e].record == rows[p] && self.edges[e].reverse == reverse
                        })
                }) {
                    continue;
                }
                let mut indices = rows[lo..=hi].to_vec();
                if reverse {
                    indices.reverse();
                }
                paths.push(indices.into_iter().map(|r| self.segment(r)).collect());
            }
        }
        paths
    }
}
