use crate::database::DatabasePool;
use crate::models::*;
use crate::oceanic::{self, OceanicPath, OceanicSource, PathRole, ResolvedPath, SearchOptions};
use crate::planning::{waypoint_equal, ConnectionClass, SkippedConnection};
use crate::{Result, RouteKitError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayRole {
    Entry,
    Exit,
    Land,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionalGateway {
    pub identifier: String,
    pub fix: Waypoint,
    pub roles: Vec<GatewayRole>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionalConnectionKind {
    PublishedTrack,
    PermittedRandom,
    PublishedTransition,
    Simulation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionalConnection {
    pub path: OceanicPath,
    pub kind: RegionalConnectionKind,
    pub required_capabilities: Vec<String>,
}
/// Caller-supplied, versioned rules. Boundaries are convex minor-great-circle
/// polygons in one hemisphere; non-convex regions must be split offline.
/// The engine verifies supplied constraints, not authenticity/completeness of an AIP.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OceanRegion {
    pub identifier: String,
    pub version: String,
    pub rules_reference: String,
    pub valid_from_unix: i64,
    pub valid_until_unix: i64,
    pub boundary: Vec<Coordinate>,
    pub excluded_areas: Vec<Vec<Coordinate>>,
    pub gateways: Vec<RegionalGateway>,
    pub connections: Vec<RegionalConnection>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RegionCatalog {
    pub regions: Vec<OceanRegion>,
}

type V3 = [f64; 3];
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: V3) -> Option<V3> {
    let n = dot(a, a).sqrt();
    (n > 1e-12).then(|| [a[0] / n, a[1] / n, a[2] / n])
}
fn xyz(c: &Coordinate) -> V3 {
    let (s, t) = c.latitude.to_radians().sin_cos();
    let (u, v) = c.longitude.to_radians().sin_cos();
    [t * v, t * u, s]
}
fn angle(a: V3, b: V3) -> f64 {
    dot(cross(a, b), cross(a, b)).sqrt().atan2(dot(a, b))
}
fn on_arc(p: V3, a: V3, b: V3) -> bool {
    (angle(a, p) + angle(p, b) - angle(a, b)).abs() < 1e-9
}
fn invalid(s: &str) -> RouteKitError {
    RouteKitError::ConfigError(s.into())
}

struct Polygon {
    vertices: Vec<V3>,
    normals: Vec<V3>,
}
impl Polygon {
    fn new(coordinates: &[Coordinate]) -> Result<Self> {
        let mut c = coordinates.to_vec();
        if c.len() > 1 && c.first() == c.last() {
            c.pop();
        }
        if !(3..=128).contains(&c.len()) {
            return Err(invalid("Region polygons need 3..=128 vertices"));
        }
        for p in &c {
            Coordinate::new(p.latitude, p.longitude)?;
        }
        let vertices: Vec<_> = c.iter().map(xyz).collect();
        let center = unit(
            vertices
                .iter()
                .fold([0.; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]),
        )
        .ok_or_else(|| invalid("Ambiguous region hemisphere"))?;
        if vertices.iter().any(|&p| dot(center, p) <= 1e-8) {
            return Err(invalid("Region must fit in an open hemisphere"));
        }
        let mut normals = Vec::new();
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            let mut n = unit(cross(a, b)).ok_or_else(|| invalid("Degenerate region boundary"))?;
            if dot(n, center).abs() < 1e-10 {
                return Err(invalid("Degenerate region area"));
            }
            if dot(n, center) < 0. {
                n = [-n[0], -n[1], -n[2]];
            }
            if vertices.iter().any(|&p| dot(n, p) < -1e-9) {
                return Err(invalid(
                    "Non-convex region; split into convex regions offline",
                ));
            }
            normals.push(n);
        }
        Ok(Self { vertices, normals })
    }
    fn contains(&self, p: V3) -> bool {
        self.normals.iter().all(|&n| dot(n, p) >= -1e-9)
    }
    fn intersects_arc(&self, a: V3, b: V3) -> bool {
        if self.contains(a) || self.contains(b) {
            return true;
        }
        let normal = cross(a, b);
        for i in 0..self.vertices.len() {
            let c = self.vertices[i];
            let d = self.vertices[(i + 1) % self.vertices.len()];
            if let Some(p) = unit(cross(normal, cross(c, d))) {
                if on_arc(p, a, b) && on_arc(p, c, d) {
                    return true;
                }
                let q = [-p[0], -p[1], -p[2]];
                if on_arc(q, a, b) && on_arc(q, c, d) {
                    return true;
                }
            } else if on_arc(c, a, b) || on_arc(d, a, b) {
                return true;
            }
        }
        false
    }
}

impl RegionCatalog {
    pub(crate) fn resolve(
        &self,
        db: &DatabasePool,
        options: &SearchOptions,
        capabilities: &[String],
    ) -> Result<(Vec<ResolvedPath>, Vec<SkippedConnection>)> {
        if self.regions.len() > 32
            || self
                .regions
                .iter()
                .map(|r| r.connections.len())
                .sum::<usize>()
                + options.oceanic_paths.len()
                > 64
        {
            return Err(invalid(
                "At most 32 regions and 64 total supplemental connections are supported",
            ));
        }
        let mut resolved = Vec::new();
        let mut skipped = Vec::new();
        let mut region_ids = HashSet::new();
        for r in &self.regions {
            if r.identifier.trim().is_empty()
                || !region_ids.insert(&r.identifier)
                || r.version.trim().is_empty()
                || r.rules_reference.trim().is_empty()
                || r.valid_from_unix >= r.valid_until_unix
                || r.gateways.len() > 128
                || r.excluded_areas.len() > 32
            {
                return Err(invalid(
                    "Invalid region identity, provenance, validity or size",
                ));
            }
            let boundary = Polygon::new(&r.boundary)?;
            let excluded = r
                .excluded_areas
                .iter()
                .map(|p| Polygon::new(p))
                .collect::<Result<Vec<_>>>()?;
            let mut ids = HashSet::new();
            for (i, g) in r.gateways.iter().enumerate() {
                if g.identifier.trim().is_empty()
                    || !ids.insert(&g.identifier)
                    || g.roles.is_empty()
                {
                    return Err(invalid("Invalid regional gateway identity or roles"));
                }
                if r.gateways[..i]
                    .iter()
                    .any(|other| waypoint_equal(&g.fix, &other.fix))
                {
                    return Err(invalid(
                        "Duplicate physical gateway; combine roles on one entry",
                    ));
                }
                Coordinate::new(g.fix.coordinate.latitude, g.fix.coordinate.longitude)?;
                if !db
                    .find_waypoints(&g.fix.identifier)?
                    .iter()
                    .any(|p| waypoint_equal(p, &g.fix))
                {
                    return Err(invalid("Regional gateway does not match a database fix"));
                }
            }
            let time = options
                .planning_time_unix
                .ok_or_else(|| invalid("Region catalogs require planning_time_unix"))?;
            let mut names = HashSet::new();
            for c in &r.connections {
                let p = &c.path;
                if !names.insert(&p.identifier)
                    || c.required_capabilities.iter().any(|v| v.trim().is_empty())
                {
                    return Err(invalid(
                        "Invalid regional connection identity or capability",
                    ));
                }
                let from = r
                    .gateways
                    .iter()
                    .find(|g| waypoint_equal(&g.fix, &p.from))
                    .ok_or_else(|| invalid("Connection origin is not a declared gateway"))?;
                let to = r
                    .gateways
                    .iter()
                    .find(|g| waypoint_equal(&g.fix, &p.to))
                    .ok_or_else(|| invalid("Connection destination is not a declared gateway"))?;
                let roles = match p.role {
                    PathRole::Oceanic => {
                        from.roles.contains(&GatewayRole::Entry)
                            && to.roles.contains(&GatewayRole::Exit)
                    }
                    PathRole::Transition => {
                        (from.roles.contains(&GatewayRole::Land)
                            && to.roles.contains(&GatewayRole::Entry))
                            || (from.roles.contains(&GatewayRole::Exit)
                                && to.roles.contains(&GatewayRole::Land))
                    }
                };
                if !roles {
                    return Err(invalid(
                        "Regional connection violates entry/exit/land roles",
                    ));
                }
                let class = match (&c.kind, &p.source, p.role) {
                    (RegionalConnectionKind::Simulation, OceanicSource::Simulation { .. }, _) => {
                        ConnectionClass::Simulation
                    }
                    (
                        RegionalConnectionKind::PublishedTrack,
                        OceanicSource::Snapshot { .. },
                        PathRole::Oceanic,
                    ) => ConnectionClass::PublishedTrack,
                    (
                        RegionalConnectionKind::PermittedRandom,
                        OceanicSource::Snapshot { .. },
                        PathRole::Oceanic,
                    ) => ConnectionClass::PermittedRandom,
                    (
                        RegionalConnectionKind::PublishedTransition,
                        OceanicSource::Snapshot { .. },
                        PathRole::Transition,
                    ) => ConnectionClass::PublishedTransition,
                    _ => {
                        return Err(invalid(
                            "A simulation cannot be promoted to a published/permitted connection",
                        ))
                    }
                };
                let mut geometry = vec![p.from.coordinate];
                geometry.extend(&p.via);
                geometry.push(p.to.coordinate);
                for point in &geometry {
                    Coordinate::new(point.latitude, point.longitude)?;
                }
                if p.role == PathRole::Oceanic
                    && geometry.iter().any(|p| !boundary.contains(xyz(p)))
                {
                    return Err(invalid(
                        "Oceanic connection leaves its declared convex region",
                    ));
                }
                if geometry.windows(2).any(|w| {
                    excluded
                        .iter()
                        .any(|p| p.intersects_arc(xyz(&w[0]), xyz(&w[1])))
                }) {
                    return Err(invalid("Regional connection intersects an excluded area"));
                }
                if let OceanicSource::Snapshot {
                    valid_from_unix,
                    valid_until_unix,
                    ..
                } = &p.source
                {
                    if *valid_from_unix < r.valid_from_unix
                        || *valid_until_unix > r.valid_until_unix
                        || valid_from_unix >= valid_until_unix
                    {
                        return Err(invalid(
                            "Connection validity must lie inside region rule validity",
                        ));
                    }
                }
                let inactive = if !(r.valid_from_unix..r.valid_until_unix).contains(&time) {
                    Some("Region rule snapshot is inactive")
                } else if c
                    .required_capabilities
                    .iter()
                    .any(|v| !capabilities.contains(v))
                {
                    Some("Required capabilities are absent")
                } else if matches!(&p.source,OceanicSource::Snapshot{valid_from_unix,valid_until_unix,..} if !(*valid_from_unix..*valid_until_unix).contains(&time))
                {
                    Some("Connection snapshot is inactive")
                } else {
                    None
                };
                if let Some(reason) = inactive {
                    skipped.push(SkippedConnection {
                        region: r.identifier.clone(),
                        identifier: p.identifier.clone(),
                        reason: reason.into(),
                    });
                    continue;
                }
                let local = SearchOptions {
                    oceanic_paths: vec![p.clone()],
                    ..options.clone()
                };
                let mut path = oceanic::prepare(db, &local)?.remove(0);
                path.class = class;
                path.region = Some(r.identifier.clone());
                path.region_valid_until = Some(r.valid_until_unix);
                resolved.push(path);
            }
        }
        let mut names: HashSet<&str> = options
            .oceanic_paths
            .iter()
            .map(|p| p.identifier.as_str())
            .collect();
        for p in &resolved {
            if !names.insert(&p.spec.identifier) {
                return Err(invalid("Duplicate supplemental identifier across catalogs"));
            }
        }
        Ok((resolved, skipped))
    }
}
