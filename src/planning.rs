use crate::models::*;
use crate::oceanic::{RouteSearchResult, SearchOptions};
use crate::regions::RegionCatalog;
use crate::{Result, RouteKitError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionClass {
    PublishedAirway,
    PublishedDirect,
    PublishedTrack,
    PermittedRandom,
    PublishedTransition,
    Simulation,
}
impl ConnectionClass {
    pub(crate) fn preferred(self) -> bool {
        matches!(
            self,
            Self::PublishedAirway | Self::PublishedTrack | Self::PublishedTransition
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RoutingPolicy {
    pub regression_weight: f64,
    pub equivalent_ratio: f64,
    pub equivalent_min_nm: f64,
    pub equivalent_max_nm: f64,
    /// Diagnostic threshold only. Necessary detours are not rejected by this ratio.
    pub detour_warning_ratio: f64,
}
impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            regression_weight: 2.0,
            equivalent_ratio: 0.002,
            equivalent_min_nm: 3.0,
            equivalent_max_nm: 20.0,
            detour_warning_ratio: 1.15,
        }
    }
}
impl RoutingPolicy {
    pub(crate) fn validate(&self) -> Result<()> {
        if [
            self.regression_weight,
            self.equivalent_ratio,
            self.equivalent_min_nm,
            self.equivalent_max_nm,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
            || self.equivalent_min_nm > self.equivalent_max_nm
            || !self.detour_warning_ratio.is_finite()
            || self.detour_warning_ratio < 1.0
        {
            return Err(RouteKitError::ConfigError("Invalid routing policy".into()));
        }
        Ok(())
    }
    pub(crate) fn tolerance(&self, distance_nm: f64, distance_weight: f64) -> f64 {
        (distance_nm * self.equivalent_ratio).clamp(self.equivalent_min_nm, self.equivalent_max_nm)
            * distance_weight
    }
}

/// Additive API: legacy Config, RouteRequest, SearchOptions and route schemas remain unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlanningContext {
    pub options: SearchOptions,
    pub policy: RoutingPolicy,
    pub regions: RegionCatalog,
    /// Caller-declared capabilities. The planner does not infer aircraft equipment.
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDistance {
    pub class: ConnectionClass,
    pub distance_nm: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteQuality {
    pub efficiency_cost: f64,
    pub reference_efficiency_cost: f64,
    pub preference_cost_limit: f64,
    pub regression_nm: f64,
    pub proxy_detour_ratio: f64,
    pub sources: Vec<SourceDistance>,
    pub flags: Vec<String>,
    pub regions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedConnection {
    pub region: String,
    pub identifier: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningResult {
    pub result: RouteSearchResult,
    pub quality: Vec<RouteQuality>,
    pub skipped_connections: Vec<SkippedConnection>,
}

pub(crate) fn class_for_edge(graph: &crate::network::Network, id: usize) -> ConnectionClass {
    graph
        .supplemental
        .get(&id)
        .map(|p| p.class)
        .unwrap_or_else(|| {
            if graph.edges[id].direct {
                ConnectionClass::PublishedDirect
            } else {
                ConnectionClass::PublishedAirway
            }
        })
}

pub(crate) fn waypoint_equal(a: &Waypoint, b: &Waypoint) -> bool {
    a.identifier == b.identifier
        && a.icao_code == b.icao_code
        && crate::geo::haversine_distance_nm(&a.coordinate, &b.coordinate) < 1e-6
}
