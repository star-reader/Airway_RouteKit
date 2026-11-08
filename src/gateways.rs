//! Extract only enroute-facing procedure boundaries, without simulating procedures.
use crate::error::Result;
use crate::geo::haversine_distance_nm;
use crate::models::*;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

struct Leg {
    fix: Option<Waypoint>,
    termination: String,
}
struct Branch {
    role: u8,
    family: u8,
    transition: String,
    legs: Vec<Leg>,
}

fn same_fix(a: &Waypoint, b: &Waypoint) -> bool {
    a.identifier == b.identifier
        && a.icao_code == b.icao_code
        && haversine_distance_nm(&a.coordinate, &b.coordinate) < 0.05
}

fn runway_compatible(a: &str, b: &str) -> bool {
    // Named enroute transitions do not impose runway identity.
    if !a.starts_with("RW") || !b.starts_with("RW") {
        return true;
    }
    a == b || (a.get(..4) == b.get(..4) && (a.ends_with('B') || b.ends_with('B')))
}

pub(crate) fn load(conn: &Connection, airport: &str, departure: bool) -> Result<Vec<Waypoint>> {
    let table = if departure { "tbl_sids" } else { "tbl_stars" };
    let sql = format!("SELECT procedure_identifier,route_type,transition_identifier,seqno,
        waypoint_identifier,waypoint_icao_code,waypoint_latitude,waypoint_longitude,
        path_termination,id,waypoint_description_code FROM {table}
        WHERE airport_identifier=?1 ORDER BY procedure_identifier,route_type,transition_identifier,seqno");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params![airport])?;
    let mut procedures: BTreeMap<String, BTreeMap<(String, String), Branch>> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let proc_id: String = row.get(0)?;
        let kind: String = row.get(1)?;
        // Roles follow flight direction: runway/common/enroute for SID; reversed for STAR.
        let role = match (departure, kind.as_str()) {
            (true, "1" | "4" | "F") | (false, "1" | "4" | "7" | "F") => 0,
            (_, "2" | "5" | "M") | (false, "8") => 1,
            (true, "3" | "6" | "S") | (false, "3" | "6" | "9" | "S") => 2,
            _ => continue, // Engine-out/vector/unknown boundary types are not guessed.
        };
        let transition = row.get::<_, Option<String>>(2)?.unwrap_or_default();
        let family = match kind.as_str() {
            "1" | "2" | "3" => 0,
            "4" | "5" | "6" => 1,
            "7" | "8" | "9" => 2,
            _ => 3,
        };
        let identifier: Option<String> = row.get(4)?;
        let lat: Option<f64> = row.get(6)?;
        let lon: Option<f64> = row.get(7)?;
        let desc = row.get::<_, Option<String>>(10)?.unwrap_or_default();
        let fix = match (identifier, lat, lon) {
            (Some(id), Some(lat), Some(lon)) if !id.is_empty() => Some(Waypoint {
                identifier: id,
                icao_code: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                coordinate: Coordinate::new(lat, lon)?,
                name: None,
                usage: None,
                id: row.get::<_, Option<String>>(9)?.filter(|s| !s.is_empty()),
                waypoint_type: match desc.as_bytes().first() {
                    Some(b'V') => WaypointType::VOR,
                    Some(b'N') => WaypointType::NDB,
                    _ => WaypointType::Enroute,
                },
            }),
            _ => None,
        };
        procedures
            .entry(proc_id)
            .or_default()
            .entry((kind, transition.clone()))
            .or_insert_with(|| Branch {
                role,
                family,
                transition,
                legs: Vec::new(),
            })
            .legs
            .push(Leg {
                fix,
                termination: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
            });
    }
    let mut gateways = Vec::new();
    for groups in procedures.values() {
        for branch in groups.values() {
            let boundary = if departure {
                branch.legs.last()
            } else {
                branch.legs.first()
            };
            let Some(leg) = boundary else {
                continue;
            };
            // Only fixed-position boundary terminations. Never substitute the last named point
            // before a heading/vector/manual termination.
            if !matches!(
                leg.termination.as_str(),
                "IF" | "TF" | "CF" | "DF" | "RF" | "AF"
            ) {
                continue;
            }
            let Some(fix) = &leg.fix else {
                continue;
            };
            let extended = groups.values().any(|other| {
                let further = if departure {
                    other.role > branch.role
                } else {
                    other.role < branch.role
                };
                // If an outward section exists, even a mismatched interface must not
                // turn an internal section boundary into an enroute gateway.
                further
                    && branch.family == other.family
                    && runway_compatible(&branch.transition, &other.transition)
            });
            if !extended {
                gateways.push(fix.clone());
            }
        }
    }
    gateways.sort_by(|a, b| {
        a.identifier
            .cmp(&b.identifier)
            .then(a.icao_code.cmp(&b.icao_code))
            .then(a.coordinate.latitude.total_cmp(&b.coordinate.latitude))
            .then(a.coordinate.longitude.total_cmp(&b.coordinate.longitude))
    });
    gateways.dedup_by(|a, b| same_fix(a, b));
    Ok(gateways)
}
