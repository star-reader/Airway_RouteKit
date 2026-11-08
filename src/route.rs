use crate::config::Config;
use crate::database::DatabasePool;
use crate::error::{Result, RouteKitError};
use crate::geo::{bearing_turn_angle, calculate_bearing, haversine_distance_nm};
use crate::geo::{distance_regression_from_ranges, distance_regression_nm};
use crate::models::*;
use crate::network::{HeightBand, Network};
use crate::oceanic::{
    OceanicSource, PathRole, RouteDiagnostics, RouteSearchResult, SearchOptions, UsedOceanicPath,
};
use crate::planning::{
    class_for_edge, ConnectionClass, PlanningContext, PlanningResult, RouteQuality, RoutingPolicy,
    SourceDistance,
};
use crate::spatial::SpatialIndex;
use parking_lot::RwLock;
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
struct Solution {
    edges: Vec<usize>,
    cost: f64,
    exposure: f64,
    changes: usize,
}
struct Cached {
    key: String,
    graph: Weak<Network>,
    solutions: Vec<Solution>,
    reference: f64,
    preference_limit: f64,
}

pub struct RouteSearcher {
    db_pool: Arc<DatabasePool>,
    config: Config,
    cache: RwLock<VecDeque<Cached>>,
}

#[derive(Clone, Copy, Default)]
struct QueueItem {
    cost: f64,
    index: usize,
    changes: usize,
    bound: f64,
}
impl QueueItem {
    fn score_cmp(&self, other: &Self) -> Ordering {
        self.cost
            .total_cmp(&other.cost)
            .then(self.changes.cmp(&other.changes))
            .then(self.bound.total_cmp(&other.bound))
    }
}
impl PartialEq for QueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
            && self.changes == other.changes
            && self.bound == other.bound
            && self.index == other.index
    }
}
impl Eq for QueueItem {}
impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.changes.cmp(&self.changes))
            .then_with(|| other.bound.total_cmp(&self.bound))
            .then_with(|| other.index.cmp(&self.index))
    }
}

struct Label {
    node: usize,
    incoming: Option<usize>,
    parent: Option<usize>,
    cost: f64,
    base_cost: f64,
    changes: usize,
    distance: f64,
    distance_limit: f64,
    height: HeightBand,
    steps: usize,
    active: bool,
}

struct Query<'a> {
    graph: &'a Network,
    request: &'a RouteRequest,
    config: &'a Config,
    departure: &'a Airport,
    destination: &'a Airport,
    starts: Vec<usize>,
    ends: HashMap<usize, f64>,
    heuristic: Vec<f64>,
    initial_height: HeightBand,
    deadline: Instant,
    options: &'a SearchOptions,
    distance_limit: f64,
    policy: &'a RoutingPolicy,
    objective: Objective,
    base_limit: f64,
    remaining: Vec<f64>,
    away_cache: Vec<Cell<f64>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Objective {
    Efficiency,
    Preference,
}

fn angular_penalty(a: f64, b: f64) -> f64 {
    (1.0 - bearing_turn_angle(a, b).to_radians().cos()) / 2.0
}

fn timeout() -> RouteKitError {
    RouteKitError::General("Search budget exhausted before optimality was established".into())
}

impl RouteSearcher {
    pub fn new(
        db_pool: Arc<DatabasePool>,
        _spatial_index: Arc<RwLock<SpatialIndex>>,
        config: Config,
    ) -> Self {
        Self {
            db_pool,
            config,
            cache: RwLock::new(VecDeque::new()),
        }
    }

    pub(crate) fn clear_cache(&self) {
        self.cache.write().clear();
    }

    pub fn search_routes_simple(&self, request: &RouteRequest) -> Result<Vec<RouteResult>> {
        Ok(self
            .search_routes(request)?
            .into_iter()
            .map(|r| RouteResult {
                route_string: r.to_route_string(),
                total_distance_nm: r.total_distance_nm,
            })
            .collect())
    }

    pub fn search_routes(&self, request: &RouteRequest) -> Result<Vec<Route>> {
        Ok(self
            .search_routes_with_options(request, &SearchOptions::default())?
            .routes)
    }

    pub fn search_routes_with_options(
        &self,
        request: &RouteRequest,
        options: &SearchOptions,
    ) -> Result<RouteSearchResult> {
        Ok(self
            .search_routes_with_context(
                request,
                &PlanningContext {
                    options: options.clone(),
                    ..Default::default()
                },
            )?
            .result)
    }

    pub fn search_routes_with_context(
        &self,
        request: &RouteRequest,
        context: &PlanningContext,
    ) -> Result<PlanningResult> {
        let options = &context.options;
        self.config.validate()?;
        options.validate()?;
        context.policy.validate()?;
        let mut request = request.clone();
        request.departure_icao = request.departure_icao.trim().to_uppercase();
        request.destination_icao = request.destination_icao.trim().to_uppercase();
        if request.max_routes == 0 {
            return Ok(PlanningResult {
                result: RouteSearchResult {
                    routes: Vec::new(),
                    diagnostics: Vec::new(),
                },
                quality: Vec::new(),
                skipped_connections: Vec::new(),
            });
        }
        for airport in [&request.departure_icao, &request.destination_icao] {
            if !crate::utils::validate_icao(airport) {
                return Err(RouteKitError::InvalidIcao(airport.clone()));
            }
        }
        if request.departure_icao == request.destination_icao {
            return Err(RouteKitError::General(
                "Departure and destination must differ".into(),
            ));
        }
        let initial_height = match request.flight_level {
            Some(FlightLevel::Custom(fl)) if (1..=1000).contains(&fl) => HeightBand {
                low: fl * 100,
                high: fl * 100,
            },
            Some(FlightLevel::Custom(_)) => {
                return Err(RouteKitError::ConfigError(
                    "Custom flight level must be 1..=1000 (350 means FL350)".into(),
                ))
            }
            _ => HeightBand::ALL,
        };
        let base = self.db_pool.network()?;
        let (regional, skipped_connections) =
            context
                .regions
                .resolve(&self.db_pool, options, &context.capabilities)?;
        let mut supplemental = crate::oceanic::prepare(&self.db_pool, options)?;
        supplemental.extend(regional);
        let graph = if supplemental.is_empty() {
            base
        } else {
            Arc::new(base.with_supplemental(supplemental)?)
        };
        let departure = self.db_pool.load_airport(&request.departure_icao)?;
        let destination = self.db_pool.load_airport(&request.destination_icao)?;
        let key = serde_json::to_string(&(&request, context))?;
        // Supplemental graphs are query-local; keep them and time-sensitive snapshots out of the cache.
        let cacheable = self.config.enable_cache
            && options.oceanic_paths.is_empty()
            && context.regions.regions.is_empty();
        if cacheable {
            let cache = self.cache.read();
            if let Some(entry) = cache.iter().find(|c| {
                c.key == key && c.graph.upgrade().is_some_and(|g| Arc::ptr_eq(&g, &graph))
            }) {
                return build_planning_result(
                    &graph,
                    &entry.solutions,
                    &departure,
                    &destination,
                    context,
                    (entry.reference, entry.preference_limit),
                    skipped_connections,
                );
            }
        }
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(self.config.search_timeout_ms))
            .ok_or_else(|| {
                RouteKitError::ConfigError(
                    "Search timeout exceeds the supported clock range".into(),
                )
            })?;
        let mut starts: Vec<_> = self
            .db_pool
            .find_sid_exit_waypoints(&departure.identifier)?
            .iter()
            .flat_map(|p| graph.matches(p))
            .collect();
        let mut arrivals: Vec<_> = self
            .db_pool
            .find_star_entry_waypoints(&destination.identifier)?
            .iter()
            .flat_map(|p| graph.matches(p))
            .collect();
        starts.sort_unstable();
        starts.dedup();
        arrivals.sort_unstable();
        arrivals.dedup();
        let mut query = Query {
            graph: &graph,
            request: &request,
            config: &self.config,
            departure: &departure,
            destination: &destination,
            starts,
            ends: HashMap::new(),
            heuristic: Vec::new(),
            initial_height,
            deadline,
            options,
            distance_limit: options
                .limit(&departure.coordinate, &destination.coordinate)?
                .unwrap_or(f64::INFINITY),
            policy: &context.policy,
            objective: Objective::Efficiency,
            base_limit: f64::INFINITY,
            remaining: graph
                .nodes
                .iter()
                .map(|p| haversine_distance_nm(&p.coordinate, &destination.coordinate))
                .collect(),
            away_cache: (0..graph.edges.len())
                .map(|_| Cell::new(f64::NAN))
                .collect(),
        };
        for n in arrivals {
            if graph.incoming[n].iter().any(|&e| query.allowed(e)) {
                query.ends.insert(n, query.endpoint_cost(n, false));
            }
        }
        query.starts = query
            .starts
            .iter()
            .copied()
            .filter(|&n| graph.outgoing[n].iter().any(|&e| query.allowed(e)))
            .collect();
        if query.starts.is_empty() || query.ends.is_empty() {
            return Err(RouteKitError::General("No verified SID exit / STAR entry with usable enroute connections; no proximity DCT fallback was inserted".into()));
        }
        query.heuristic = query.reverse_lower_bound()?;
        query.starts.retain(|&n| query.heuristic[n].is_finite());
        let Some(reference_solution) = query.solve(&HashSet::new(), None, None)? else {
            if query.distance_limit.is_finite() {
                return Err(RouteKitError::General(format!(
                    "No feasible route within detour budget ({:.1} NM including terminal distance proxies) using the available graph and supplied context",
                    query.distance_limit)));
            }
            return Err(RouteKitError::RouteNotFound {
                from: request.departure_icao.clone(),
                to: request.destination_icao.clone(),
            });
        };
        let reference = reference_solution.cost;
        let reference_distance = reference_solution
            .edges
            .iter()
            .map(|&id| graph.edges[id].distance)
            .sum::<f64>()
            + haversine_distance_nm(
                &departure.coordinate,
                &graph.nodes[graph.edges[reference_solution.edges[0]].from].coordinate,
            )
            + haversine_distance_nm(
                &destination.coordinate,
                &graph.nodes[graph.edges[*reference_solution.edges.last().unwrap()].to].coordinate,
            );
        let preference_limit = reference
            + context.policy.tolerance(
                reference_distance,
                self.config.search_weights.distance_weight,
            );
        let best = query.prefer(
            reference_solution,
            &HashSet::new(),
            None,
            None,
            preference_limit,
        )?;
        let mut solutions = vec![best.clone()];
        // Small bounded deviation pool: alternatives, not a claim of exact K-shortest paths.
        // The best result is always solved with ALL gateways, independently of max_routes.
        if request.max_routes > 1 {
            let first = graph.edges[best.edges[0]].from;
            let last = graph.edges[*best.edges.last().unwrap()].to;
            let mut variations = vec![(None, Some(first), None), (None, None, Some(last))];
            let mut positions = vec![
                0,
                best.edges.len() / 3,
                best.edges.len() / 2,
                best.edges.len() * 2 / 3,
                best.edges.len() - 1,
            ];
            for i in 1..best.edges.len() {
                if graph.records[graph.edges[best.edges[i]].record].airway
                    != graph.records[graph.edges[best.edges[i - 1]].record].airway
                {
                    positions.push(i);
                }
            }
            positions.sort_unstable();
            positions.dedup();
            for i in positions.into_iter().take(10) {
                variations.push((Some(best.edges[i]), None, None));
            }
            let mut signatures = HashSet::new();
            signatures.insert(signature(&graph, &best));
            for (edge, start, end) in variations {
                if Instant::now() >= deadline {
                    break;
                }
                let banned: HashSet<_> = if let Some(id) = edge {
                    let e = &graph.edges[id];
                    graph.outgoing[e.from]
                        .iter()
                        .copied()
                        .filter(|&other| {
                            graph.edges[other].to == e.to
                                && signature(
                                    &graph,
                                    &Solution {
                                        edges: vec![other],
                                        cost: 0.0,
                                        ..Default::default()
                                    },
                                ) == signature(
                                    &graph,
                                    &Solution {
                                        edges: vec![id],
                                        cost: 0.0,
                                        ..Default::default()
                                    },
                                )
                        })
                        .collect()
                } else {
                    HashSet::new()
                };
                query.objective = Objective::Efficiency;
                query.base_limit = f64::INFINITY;
                let alternative = query.solve(&banned, start, end).and_then(|s| {
                    s.map(|s| query.prefer(s, &banned, start, end, preference_limit))
                        .transpose()
                });
                match alternative {
                    Ok(Some(s)) if signatures.insert(signature(&graph, &s)) => solutions.push(s),
                    Ok(_) => {}
                    Err(RouteKitError::General(_)) => break,
                    Err(e) => return Err(e),
                }
            }
        }
        solutions.sort_by(|a, b| query.rank(a, b, preference_limit));
        solutions.truncate(request.max_routes);
        let result = build_planning_result(
            &graph,
            &solutions,
            &departure,
            &destination,
            context,
            (reference, preference_limit),
            skipped_connections,
        )?;
        if cacheable && self.config.cache_size > 0 {
            let mut cache = self.cache.write();
            while cache.len() >= self.config.cache_size {
                cache.pop_front();
            }
            cache.push_back(Cached {
                key,
                graph: Arc::downgrade(&graph),
                solutions,
                reference,
                preference_limit,
            });
        }
        Ok(result)
    }
}

impl Query<'_> {
    fn prefer(
        &mut self,
        fallback: Solution,
        banned: &HashSet<usize>,
        start: Option<usize>,
        end: Option<usize>,
        limit: f64,
    ) -> Result<Solution> {
        if self.request.route_preference == RoutePreference::DirectPreferred
            || fallback.cost > limit + 1e-8
        {
            return Ok(fallback);
        }
        self.objective = Objective::Preference;
        self.base_limit = limit;
        self.solve(banned, start, end)?.ok_or_else(|| {
            RouteKitError::General("Preference pass lost an efficiency-feasible path".into())
        })
    }

    fn rank(&self, a: &Solution, b: &Solution, limit: f64) -> Ordering {
        let cost = || a.cost.total_cmp(&b.cost).then(a.edges.cmp(&b.edges));
        if self.request.route_preference == RoutePreference::DirectPreferred {
            return cost();
        }
        let a_inside = a.cost <= limit + 1e-8;
        let b_inside = b.cost <= limit + 1e-8;
        b_inside.cmp(&a_inside).then_with(|| {
            if a_inside && b_inside {
                a.exposure
                    .total_cmp(&b.exposure)
                    .then(a.changes.cmp(&b.changes))
                    .then_with(cost)
            } else {
                cost()
            }
        })
    }

    fn regression(&self, id: usize) -> f64 {
        let cache = &self.away_cache[id];
        if !cache.get().is_nan() {
            return cache.get();
        }
        let e = &self.graph.edges[id];
        let value = if let Some(path) = self.graph.supplemental.get(&id) {
            path.points
                .windows(2)
                .map(|p| {
                    distance_regression_nm(
                        &p[0].coordinate,
                        &p[1].coordinate,
                        &self.destination.coordinate,
                    )
                })
                .sum()
        } else {
            distance_regression_from_ranges(
                self.remaining[e.from],
                self.remaining[e.to],
                e.distance,
            )
        };
        cache.set(value);
        value
    }

    fn exposure(&self, id: usize) -> f64 {
        if class_for_edge(self.graph, id).preferred() {
            0.0
        } else {
            self.graph.edges[id].distance
        }
    }

    fn route_change(&self, id: usize, previous: Option<usize>) -> usize {
        usize::from(previous.is_some_and(|p| {
            class_for_edge(self.graph, p).preferred()
                && class_for_edge(self.graph, id).preferred()
                && self.graph.records[self.graph.edges[p].record].airway
                    != self.graph.records[self.graph.edges[id].record].airway
        }))
    }

    fn priority(
        &self,
        cost: f64,
        changes: usize,
        base: f64,
        node: usize,
        index: usize,
    ) -> QueueItem {
        match self.objective {
            Objective::Efficiency => QueueItem {
                cost: cost + self.heuristic[node],
                index,
                ..Default::default()
            },
            Objective::Preference => QueueItem {
                cost,
                changes,
                bound: base + self.heuristic[node],
                index,
            },
        }
    }

    fn allowed(&self, id: usize) -> bool {
        let e = &self.graph.edges[id];
        self.graph.level_allowed(e, self.request.flight_level)
            && self.initial_height.intersect(e.height).is_some()
            && e.magnetic.is_some()
    }

    fn endpoint_cost(&self, n: usize, departure: bool) -> f64 {
        let (airport, other) = if departure {
            (self.departure, self.destination)
        } else {
            (self.destination, self.departure)
        };
        let c = &self.graph.nodes[n].coordinate;
        let d = haversine_distance_nm(&airport.coordinate, c);
        let scale =
            (0.1 * haversine_distance_nm(&airport.coordinate, &other.coordinate)).clamp(20.0, 80.0);
        self.config.search_weights.distance_weight * d
            + if d < 0.1 {
                0.0
            } else {
                scale
                    * angular_penalty(
                        calculate_bearing(&airport.coordinate, c),
                        calculate_bearing(&airport.coordinate, &other.coordinate),
                    )
            }
    }

    fn join_cost(&self, id: usize, departure: bool) -> f64 {
        let e = &self.graph.edges[id];
        let (airport, node) = if departure {
            (self.departure, e.from)
        } else {
            (self.destination, e.to)
        };
        let c = &self.graph.nodes[node].coordinate;
        if haversine_distance_nm(c, &airport.coordinate) < 0.1 {
            return 0.0;
        }
        if departure {
            10.0 * angular_penalty(
                (calculate_bearing(c, &airport.coordinate) + 180.0) % 360.0,
                e.initial,
            )
        } else {
            10.0 * angular_penalty(e.final_course, calculate_bearing(c, &airport.coordinate))
        }
    }

    fn edge_cost(&self, id: usize, previous: Option<usize>) -> f64 {
        let e = &self.graph.edges[id];
        let (direct_factor, entry) = match self.request.route_preference {
            RoutePreference::AirwayPreferred => (1.0, 30.0),
            RoutePreference::Balanced => (0.35, 12.0),
            RoutePreference::DirectPreferred => (0.0, 0.0),
        };
        let weights = &self.config.search_weights;
        let mut cost = weights.distance_weight
            * (e.distance + self.policy.regression_weight * self.regression(id));
        if let Some(path) = self.graph.supplemental.get(&id) {
            cost += path.internal_turn_cost;
        }
        let prev = previous.map(|id| &self.graph.edges[id]);
        let oceanic = self
            .graph
            .supplemental
            .get(&id)
            .is_some_and(|p| p.spec.role == PathRole::Oceanic);
        if e.direct
            && !oceanic
            && class_for_edge(self.graph, id) != ConnectionClass::PublishedTransition
        {
            cost += (direct_factor * e.distance
                + if previous.is_some_and(|id| {
                    self.graph.edges[id].direct
                        && !self
                            .graph
                            .supplemental
                            .get(&id)
                            .is_some_and(|p| p.spec.role == PathRole::Oceanic)
                }) {
                    0.0
                } else {
                    entry
                })
                * weights.airway_preference_weight
                / 0.8;
        }
        if e.minimum.is_none() {
            cost += weights.altitude_match_weight * 0.005 * e.distance;
        }
        if let Some(p) = prev {
            let angle = bearing_turn_angle(p.final_course, e.initial);
            cost += 10.0 * ((angle - 30.0).max(0.0) / 150.0).powi(2);
        } else {
            cost += self.join_cost(id, true);
        }
        cost
    }

    // Distances in this reverse graph omit all non-negative history-dependent costs.
    fn reverse_lower_bound(&self) -> Result<Vec<f64>> {
        let mut d = vec![f64::INFINITY; self.graph.nodes.len()];
        let mut queue = BinaryHeap::new();
        for (&n, &cost) in &self.ends {
            d[n] = cost;
            queue.push(QueueItem {
                cost,
                index: n,
                ..Default::default()
            });
        }
        let mut iterations = 0;
        while let Some(item) = queue.pop() {
            iterations += 1;
            if iterations % 512 == 0 && Instant::now() >= self.deadline {
                return Err(timeout());
            }
            if item.cost > d[item.index] {
                continue;
            }
            for &id in &self.graph.incoming[item.index] {
                if !self.allowed(id) {
                    continue;
                }
                let e = &self.graph.edges[id];
                let cost = item.cost + self.config.search_weights.distance_weight * e.distance;
                if cost < d[e.from] {
                    d[e.from] = cost;
                    queue.push(QueueItem {
                        cost,
                        index: e.from,
                        ..Default::default()
                    });
                }
            }
        }
        Ok(d)
    }

    fn solve(
        &self,
        banned: &HashSet<usize>,
        excluded_start: Option<usize>,
        excluded_end: Option<usize>,
    ) -> Result<Option<Solution>> {
        let answer = self.astar(banned, excluded_start, excluded_end, false)?;
        if answer.as_ref().is_some_and(|s| has_cycle(self.graph, s)) {
            self.astar(banned, excluded_start, excluded_end, true)
        } else {
            Ok(answer)
        }
    }

    fn astar(
        &self,
        banned: &HashSet<usize>,
        excluded_start: Option<usize>,
        excluded_end: Option<usize>,
        elementary: bool,
    ) -> Result<Option<Solution>> {
        if Instant::now() >= self.deadline {
            return Err(timeout());
        }
        let mut labels: Vec<Label> = Vec::new();
        let mut buckets: HashMap<(usize, Option<usize>), Vec<usize>> = HashMap::new();
        let mut queue = BinaryHeap::new();
        for &node in &self.starts {
            if Some(node) == excluded_start {
                continue;
            }
            let base_cost = self.endpoint_cost(node, true);
            let cost = if self.objective == Objective::Efficiency {
                base_cost
            } else {
                0.0
            };
            let distance = haversine_distance_nm(
                &self.departure.coordinate,
                &self.graph.nodes[node].coordinate,
            );
            if distance + self.remaining[node] > self.distance_limit + 1e-8
                || base_cost + self.heuristic[node] > self.base_limit + 1e-8
            {
                continue;
            }
            let index = labels.len();
            labels.push(Label {
                node,
                incoming: None,
                parent: None,
                cost,
                base_cost,
                changes: 0,
                distance,
                distance_limit: self.distance_limit,
                height: self.initial_height,
                steps: 0,
                active: true,
            });
            buckets.entry((node, None)).or_default().push(index);
            queue.push(self.priority(cost, 0, base_cost, node, index));
        }
        let mut best: Option<(QueueItem, f64)> = None;
        let mut depth_limited = false;
        let mut iterations = 0;
        while let Some(item) = queue.pop() {
            iterations += 1;
            if iterations % 128 == 0 && Instant::now() >= self.deadline {
                return Err(timeout());
            }
            if !labels[item.index].active {
                continue;
            }
            if best
                .as_ref()
                .is_some_and(|(goal, _)| item.score_cmp(goal) == Ordering::Greater)
            {
                break;
            }
            let current = &labels[item.index];
            if current.steps > 0 && Some(current.node) != excluded_end {
                if let Some(&terminal) = self.ends.get(&current.node) {
                    let terminal = terminal + self.join_cost(current.incoming.unwrap(), false);
                    let base = current.base_cost + terminal;
                    if base <= self.base_limit + 1e-8 {
                        let goal = match self.objective {
                            Objective::Efficiency => QueueItem {
                                cost: base,
                                index: item.index,
                                ..Default::default()
                            },
                            Objective::Preference => QueueItem {
                                cost: current.cost,
                                changes: current.changes,
                                bound: base,
                                index: item.index,
                            },
                        };
                        if best
                            .as_ref()
                            .is_none_or(|(old, _)| goal.score_cmp(old) == Ordering::Less)
                        {
                            best = Some((goal, base));
                        }
                    }
                }
            }
            if current.steps >= self.config.max_search_depth {
                depth_limited = true;
                continue;
            }
            let (node, previous, height, g, base_g, changes, steps, distance, distance_limit) = (
                current.node,
                current.incoming,
                current.height,
                current.cost,
                current.base_cost,
                current.changes,
                current.steps,
                current.distance,
                current.distance_limit,
            );
            for &id in &self.graph.outgoing[node] {
                if banned.contains(&id) || !self.allowed(id) {
                    continue;
                }
                let e = &self.graph.edges[id];
                if previous.is_some_and(|p| self.graph.edges[p].from == e.to) {
                    continue;
                }
                if elementary
                    && edge_keys(self.graph, id)
                        .any(|point| contains_point(self.graph, &labels, item.index, point))
                {
                    continue;
                }
                let Some(height) = height.intersect(e.height) else {
                    continue;
                };
                let distance = distance + e.distance;
                let distance_limit = self
                    .graph
                    .supplemental
                    .get(&id)
                    .and_then(|p| {
                        let snapshot = match p.spec.source {
                            OceanicSource::Snapshot {
                                valid_until_unix, ..
                            } => Some(valid_until_unix),
                            _ => None,
                        };
                        snapshot
                            .into_iter()
                            .chain(p.region_valid_until)
                            .min()
                            .map(|until| {
                                ((until as i128 - self.options.planning_time_unix.unwrap() as i128)
                                    as f64
                                    - 1.0)
                                    / 3600.0
                                    * self.options.cruise_speed_knots
                            })
                    })
                    .map_or(distance_limit, |limit| distance_limit.min(limit));
                if distance + self.remaining[e.to] > distance_limit + 1e-8 {
                    continue;
                }
                let increment = self.edge_cost(id, previous);
                let base_cost = base_g + increment;
                if !self.heuristic[e.to].is_finite()
                    || base_cost + self.heuristic[e.to] > self.base_limit + 1e-8
                {
                    continue;
                }
                let (cost, changes) = match self.objective {
                    Objective::Efficiency => (g + increment, 0),
                    Objective::Preference => (
                        g + self.exposure(id),
                        changes + self.route_change(id, previous),
                    ),
                };
                let priority = self.priority(cost, changes, base_cost, e.to, 0);
                if best
                    .as_ref()
                    .is_some_and(|(goal, _)| priority.score_cmp(goal) == Ordering::Greater)
                {
                    continue;
                }
                let candidate = Label {
                    node: e.to,
                    incoming: Some(id),
                    parent: Some(item.index),
                    cost,
                    base_cost,
                    changes,
                    distance,
                    distance_limit,
                    height,
                    steps: steps + 1,
                    active: true,
                };
                let bucket = buckets.entry((e.to, Some(id))).or_default();
                let dominates = |a: &Label, b: &Label| {
                    a.cost <= b.cost
                        && a.base_cost <= b.base_cost
                        && a.changes <= b.changes
                        && a.distance <= b.distance
                        && a.distance_limit >= b.distance_limit
                        && a.height.contains(b.height)
                        && a.steps <= b.steps
                };
                if bucket.iter().any(|&old| {
                    labels[old].active
                        && dominates(&labels[old], &candidate)
                        && (!elementary
                            || visited_subset(self.graph, &labels, old, item.index, Some(id)))
                }) {
                    continue;
                }
                for &old in bucket.iter() {
                    if labels[old].active
                        && dominates(&candidate, &labels[old])
                        && (!elementary
                            || visited_subset(self.graph, &labels, item.index, old, None))
                    {
                        labels[old].active = false;
                    }
                }
                if labels.len() >= 500_000 {
                    return Err(timeout());
                }
                let index = labels.len();
                labels.push(candidate);
                bucket.push(index);
                queue.push(self.priority(cost, changes, base_cost, e.to, index));
            }
        }
        if let Some((goal, cost)) = best {
            let mut at = goal.index;
            let mut edges = Vec::new();
            while let Some(parent) = labels[at].parent {
                edges.push(labels[at].incoming.unwrap());
                at = parent;
            }
            edges.reverse();
            let exposure = edges.iter().map(|&id| self.exposure(id)).sum();
            let changes = edges
                .iter()
                .enumerate()
                .map(|(i, &id)| self.route_change(id, i.checked_sub(1).map(|p| edges[p])))
                .sum();
            return Ok(Some(Solution {
                edges,
                cost,
                exposure,
                changes,
            }));
        }
        if depth_limited {
            return Err(RouteKitError::General(
                "Maximum route depth reached before a route was found".into(),
            ));
        }
        Ok(None)
    }
}

type PhysicalKey = (i64, i64);
fn physical_key(p: &Coordinate) -> PhysicalKey {
    (
        (p.latitude * 1e7).round() as i64,
        (((p.longitude + 180.0).rem_euclid(360.0) - 180.0) * 1e7).round() as i64,
    )
}
// Include hidden macro-edge coordinates when checking elementary paths. They
// are not switching nodes, but revisiting them would still be a physical loop.
fn edge_keys(graph: &Network, id: usize) -> impl Iterator<Item = PhysicalKey> + '_ {
    std::iter::once(physical_key(&graph.nodes[graph.edges[id].to].coordinate)).chain(
        graph.supplemental.get(&id).into_iter().flat_map(|p| {
            p.points[1..p.points.len() - 1]
                .iter()
                .map(|p| physical_key(&p.coordinate))
        }),
    )
}
fn contains_point(graph: &Network, labels: &[Label], mut at: usize, point: PhysicalKey) -> bool {
    loop {
        if physical_key(&graph.nodes[labels[at].node].coordinate) == point
            || labels[at]
                .incoming
                .is_some_and(|id| edge_keys(graph, id).any(|p| p == point))
        {
            return true;
        }
        match labels[at].parent {
            Some(p) => at = p,
            None => return false,
        }
    }
}
fn visited_subset(
    graph: &Network,
    labels: &[Label],
    mut a: usize,
    b: usize,
    extra: Option<usize>,
) -> bool {
    loop {
        let contained = |point| {
            contains_point(graph, labels, b, point)
                || extra.is_some_and(|id| edge_keys(graph, id).any(|p| p == point))
        };
        if !contained(physical_key(&graph.nodes[labels[a].node].coordinate))
            || labels[a]
                .incoming
                .is_some_and(|id| edge_keys(graph, id).any(|p| !contained(p)))
        {
            return false;
        }
        match labels[a].parent {
            Some(p) => a = p,
            None => return true,
        }
    }
}
fn signature(graph: &Network, s: &Solution) -> Vec<PhysicalKey> {
    let key = physical_key;
    let mut points = vec![key(&graph.nodes[graph.edges[s.edges[0]].from].coordinate)];
    for &id in &s.edges {
        if let Some(path) = graph.supplemental.get(&id) {
            points.extend(path.points.iter().skip(1).map(|p| key(&p.coordinate)));
        } else {
            points.push(key(&graph.nodes[graph.edges[id].to].coordinate));
        }
    }
    points
}
fn has_cycle(graph: &Network, s: &Solution) -> bool {
    let mut seen = HashSet::new();
    signature(graph, s)
        .into_iter()
        .any(|point| !seen.insert(point))
}

fn build_route(
    graph: &Network,
    solution: &Solution,
    departure: &Airport,
    destination: &Airport,
) -> Result<Route> {
    if solution.edges.is_empty() || has_cycle(graph, solution) {
        return Err(RouteKitError::InvalidSegment(
            "Empty or cyclic enroute path".into(),
        ));
    }
    let mut segments = Vec::with_capacity(solution.edges.len());
    let mut height = HeightBand::ALL;
    for (i, &id) in solution.edges.iter().enumerate() {
        let e = &graph.edges[id];
        if i > 0 && graph.edges[solution.edges[i - 1]].to != e.from {
            return Err(RouteKitError::InvalidSegment(
                "Disconnected enroute edges".into(),
            ));
        }
        height = height.intersect(e.height).ok_or_else(|| {
            RouteKitError::AltitudeRestrictionViolation("No common enroute altitude".into())
        })?;
        let r = &graph.records[e.record];
        if let Some(path) = graph.supplemental.get(&id) {
            for points in path.points.windows(2) {
                segments.push(RouteSegment {
                    from: points[0].clone(),
                    to: points[1].clone(),
                    airway: None,
                    distance_nm: haversine_distance_nm(
                        &points[0].coordinate,
                        &points[1].coordinate,
                    ),
                    magnetic_course: (calculate_bearing(
                        &points[0].coordinate,
                        &points[1].coordinate,
                    ) - path.spec.magnetic_variation_deg)
                        .rem_euclid(360.0),
                    minimum_altitude: e.minimum,
                    maximum_altitude: r.max,
                });
            }
            continue;
        }
        segments.push(RouteSegment {
            from: graph.nodes[e.from].clone(),
            to: graph.nodes[e.to].clone(),
            airway: (!e.direct).then(|| graph.airways[r.airway].clone()),
            distance_nm: e.distance,
            magnetic_course: e.magnetic.ok_or_else(|| {
                RouteKitError::InvalidSegment("Missing published magnetic course".into())
            })?,
            minimum_altitude: e.minimum,
            maximum_altitude: r.max,
        });
    }
    let distance: f64 = segments.iter().map(|s| s.distance_nm).sum();
    Ok(Route {
        departure: departure.clone(),
        destination: destination.clone(),
        sid: None,
        star: None,
        segments,
        total_distance_nm: distance,
        estimated_time_minutes: Some(distance / 450.0 * 60.0),
    })
}

fn build_planning_result(
    graph: &Network,
    solutions: &[Solution],
    departure: &Airport,
    destination: &Airport,
    context: &PlanningContext,
    selection: (f64, f64),
    skipped_connections: Vec<crate::planning::SkippedConnection>,
) -> Result<PlanningResult> {
    let mut result = build_result(graph, solutions, departure, destination, &context.options)?;
    let gc = haversine_distance_nm(&departure.coordinate, &destination.coordinate);
    let mut quality = Vec::new();
    for (i, solution) in solutions.iter().enumerate() {
        let route = &result.routes[i];
        let regression = route
            .segments
            .iter()
            .map(|s| {
                distance_regression_nm(
                    &s.from.coordinate,
                    &s.to.coordinate,
                    &destination.coordinate,
                )
            })
            .sum::<f64>();
        let ratio = result.diagnostics[i].proxy_total_distance_nm / gc;
        let mut flags = Vec::new();
        if ratio > context.policy.detour_warning_ratio {
            flags.push("detour_requires_review".into());
            result.diagnostics[i].warnings.push(format!("Proxy distance is {:.1}% above airport great-circle distance; review available network coverage and applicable restrictions",(ratio-1.0)*100.0));
        }
        if regression > 1.0 {
            flags.push("local_regression".into());
            result.diagnostics[i].warnings.push(format!(
                "Physical enroute path accumulates {regression:.1} NM of distance regression"
            ));
        }
        if solution.cost > selection.1 + 1e-8 {
            flags.push("outside_preference_window".into());
        }
        let mut sources: Vec<SourceDistance> = Vec::new();
        let mut regions = Vec::new();
        for &id in &solution.edges {
            let class = class_for_edge(graph, id);
            if let Some(value) = sources.iter_mut().find(|s| s.class == class) {
                value.distance_nm += graph.edges[id].distance;
            } else {
                sources.push(SourceDistance {
                    class,
                    distance_nm: graph.edges[id].distance,
                });
            }
            if let Some(region) = graph.supplemental.get(&id).and_then(|p| p.region.as_ref()) {
                if !regions.contains(region) {
                    regions.push(region.clone());
                }
            }
        }
        if sources
            .iter()
            .any(|s| s.class == ConnectionClass::Simulation)
        {
            flags.push("offline_simulation".into());
        }
        quality.push(RouteQuality {
            efficiency_cost: solution.cost,
            reference_efficiency_cost: selection.0,
            preference_cost_limit: selection.1,
            regression_nm: regression,
            proxy_detour_ratio: ratio,
            sources,
            flags,
            regions,
        });
    }
    Ok(PlanningResult {
        result,
        quality,
        skipped_connections,
    })
}

fn build_result(
    graph: &Network,
    solutions: &[Solution],
    departure: &Airport,
    destination: &Airport,
    options: &SearchOptions,
) -> Result<RouteSearchResult> {
    let limit = options.limit(&departure.coordinate, &destination.coordinate)?;
    let mut routes = Vec::new();
    let mut diagnostics = Vec::new();
    for solution in solutions {
        let mut route = build_route(graph, solution, departure, destination)?;
        route.estimated_time_minutes =
            Some(route.total_distance_nm / options.cruise_speed_knots * 60.0);
        let first = &route.segments[0].from.coordinate;
        let last = &route.segments.last().unwrap().to.coordinate;
        let proxy = haversine_distance_nm(&departure.coordinate, first)
            + route.total_distance_nm
            + haversine_distance_nm(last, &destination.coordinate);
        if limit.is_some_and(|l| proxy > l + 1e-8) {
            return Err(RouteKitError::InvalidSegment(
                "Result exceeds detour budget".into(),
            ));
        }
        let used: Vec<_> = solution
            .edges
            .iter()
            .filter_map(|id| graph.supplemental.get(id))
            .map(|p| UsedOceanicPath {
                identifier: p.spec.identifier.clone(),
                source: p.spec.source.clone(),
                role: p.spec.role,
            })
            .collect();
        let mut warnings = Vec::new();
        if !used.is_empty() {
            warnings.push("Supplemental courses use caller-supplied constant magnetic variation; snapshot validity covers the whole estimated journey at constant speed. Airspace, equipment and operational permissions are caller responsibilities.".into());
        }
        if used
            .iter()
            .any(|p| matches!(p.source, OceanicSource::Simulation { .. }))
        {
            warnings.push("Offline simulation includes unvalidated direct legs; this is not a published or operationally authorized oceanic route.".into());
        }
        diagnostics.push(RouteDiagnostics {
            proxy_total_distance_nm: proxy,
            detour_limit_nm: limit,
            used_oceanic_paths: used,
            warnings,
        });
        routes.push(route);
    }
    Ok(RouteSearchResult {
        routes,
        diagnostics,
    })
}

pub struct RouteRequestBuilder {
    request: RouteRequest,
}
impl RouteRequestBuilder {
    pub fn new(departure: &str, destination: &str) -> Self {
        Self {
            request: RouteRequest {
                departure_icao: departure.into(),
                destination_icao: destination.into(),
                ..Default::default()
            },
        }
    }
    pub fn flight_level(mut self, level: FlightLevel) -> Self {
        self.request.flight_level = Some(level);
        self
    }
    pub fn route_preference(mut self, pref: RoutePreference) -> Self {
        self.request.route_preference = pref;
        self
    }
    pub fn max_routes(mut self, max: usize) -> Self {
        self.request.max_routes = max;
        self
    }
    pub fn build(self) -> RouteRequest {
        self.request
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{Edge, Record};

    fn airport(id: &str, lat: f64, lon: f64) -> Airport {
        Airport {
            icao_code: "ZZ".into(),
            identifier: id.into(),
            identifier_3letter: None,
            name: None,
            coordinate: Coordinate::new(lat, lon).unwrap(),
            ifr_capability: None,
            elevation: None,
            transition_altitude: None,
            transition_level: None,
        }
    }

    fn tiny_network(seed: u64) -> Network {
        let mut random = seed + 1;
        let mut next = || {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            random >> 32
        };
        let nodes: Vec<_> = (0..7)
            .map(|i| {
                Waypoint::simple(
                    &format!("P{i}"),
                    1.0 - i as f64 * 0.01,
                    (i % 3) as f64 * 0.008,
                )
                .unwrap()
            })
            .collect();
        let mut g = Network {
            nodes: nodes.into(),
            edges: Vec::new().into(),
            records: Vec::new().into(),
            outgoing: vec![Vec::new(); 7],
            incoming: vec![Vec::new(); 7],
            airways: vec!["A1".into(), "B2".into()].into(),
            airway_rows: HashMap::new(),
            by_name: HashMap::new(),
            supplemental: HashMap::new(),
        };
        for from in 0..7 {
            for to in 0..7 {
                if from == to || (to != from + 1 && next() % 4 != 0) {
                    continue;
                }
                let low = if next() % 3 == 0 { 20000 } else { 10000 };
                let high = if next() % 3 == 0 { 25000 } else { 40000 };
                let airway = (next() % 2) as usize;
                let direct = next() % 5 == 0;
                let record = g.records.len();
                g.records.push(Record {
                    node: from,
                    airway,
                    section: record,
                    seq: 10,
                    level: "B".into(),
                    direction: "F".into(),
                    kind: if direct { "D".into() } else { "O".into() },
                    min1: Some(low),
                    min2: None,
                    max: Some(high),
                    outbound: Some(90.0),
                    inbound: Some(90.0),
                    published_distance: None,
                    end: false,
                });
                let a = &g.nodes[from].coordinate;
                let b = &g.nodes[to].coordinate;
                let id = g.edges.len();
                g.outgoing[from].push(id);
                g.incoming[to].push(id);
                g.edges.push(Edge {
                    from,
                    to,
                    record,
                    reverse: false,
                    distance: haversine_distance_nm(a, b),
                    initial: calculate_bearing(a, b),
                    final_course: (calculate_bearing(b, a) + 180.0) % 360.0,
                    magnetic: Some(90.0),
                    height: HeightBand { low, high },
                    minimum: Some(low),
                    direct,
                });
            }
        }
        g
    }

    /// Exhaustively enumerate physical-node-simple paths, without A* heuristics,
    /// state merging, or label dominance. This is the independent search oracle.
    fn exhaustive(
        q: &Query<'_>,
        path: &mut Vec<usize>,
        visited: &mut HashSet<usize>,
        node: usize,
        height: HeightBand,
        cost: f64,
        best: &mut f64,
    ) {
        if let Some(&id) = path.last() {
            if let Some(&terminal) = q.ends.get(&node) {
                let first = q.graph.edges[path[0]].from;
                let distance = haversine_distance_nm(
                    &q.departure.coordinate,
                    &q.graph.nodes[first].coordinate,
                ) + path.iter().map(|&e| q.graph.edges[e].distance).sum::<f64>()
                    + haversine_distance_nm(
                        &q.graph.nodes[node].coordinate,
                        &q.destination.coordinate,
                    );
                if distance <= q.distance_limit + 1e-8 {
                    *best = best.min(cost + terminal + q.join_cost(id, false));
                }
            }
        }
        if path.len() >= q.config.max_search_depth {
            return;
        }
        for &id in &q.graph.outgoing[node] {
            let e = &q.graph.edges[id];
            if !q.allowed(id) || visited.contains(&e.to) {
                continue;
            }
            let Some(height) = height.intersect(e.height) else {
                continue;
            };
            let next = cost + q.edge_cost(id, path.last().copied());
            path.push(id);
            visited.insert(e.to);
            exhaustive(q, path, visited, e.to, height, next, best);
            path.pop();
            visited.remove(&e.to);
        }
    }

    #[test]
    fn astar_and_zero_heuristic_match_exhaustive_simple_paths() {
        let departure = airport("AAAA", 1.01, 0.0);
        let destination = airport("BBBB", 0.92, 0.0);
        let config = Config::default();
        let policy = RoutingPolicy::default();
        let options = SearchOptions {
            detour_budget: None,
            ..Default::default()
        };
        for seed in 0..48 {
            let graph = tiny_network(seed);
            for pref in [
                RoutePreference::AirwayPreferred,
                RoutePreference::Balanced,
                RoutePreference::DirectPreferred,
            ] {
                let request = RouteRequest {
                    route_preference: pref,
                    ..Default::default()
                };
                let mut q = Query {
                    graph: &graph,
                    request: &request,
                    config: &config,
                    departure: &departure,
                    destination: &destination,
                    starts: vec![0, 1],
                    ends: HashMap::new(),
                    heuristic: Vec::new(),
                    initial_height: HeightBand::ALL,
                    deadline: Instant::now() + Duration::from_secs(5),
                    options: &options,
                    distance_limit: f64::INFINITY,
                    policy: &policy,
                    objective: Objective::Efficiency,
                    base_limit: f64::INFINITY,
                    remaining: graph
                        .nodes
                        .iter()
                        .map(|p| haversine_distance_nm(&p.coordinate, &destination.coordinate))
                        .collect(),
                    away_cache: (0..graph.edges.len())
                        .map(|_| Cell::new(f64::NAN))
                        .collect(),
                };
                for n in [5, 6] {
                    q.ends.insert(n, q.endpoint_cost(n, false));
                }
                let mut expected = f64::INFINITY;
                for &start in &q.starts {
                    exhaustive(
                        &q,
                        &mut Vec::new(),
                        &mut HashSet::from([start]),
                        start,
                        q.initial_height,
                        q.endpoint_cost(start, true),
                        &mut expected,
                    );
                }
                q.heuristic = q.reverse_lower_bound().unwrap();
                for use_bound in [true, false] {
                    if !use_bound {
                        q.heuristic.fill(0.0);
                    }
                    let result = q.solve(&HashSet::new(), None, None).unwrap().unwrap();
                    assert!((result.cost-expected).abs()<1e-8,"seed={seed} preference={pref:?} bound={use_bound} actual={} expected={expected}",result.cost);
                    assert!(!has_cycle(&graph, &result));
                }
                q.heuristic = q.reverse_lower_bound().unwrap();
                for ratio in [1.0, 1.1, 1.4] {
                    q.distance_limit = ratio
                        * haversine_distance_nm(&departure.coordinate, &destination.coordinate);
                    let mut expected = f64::INFINITY;
                    for &start in &q.starts {
                        exhaustive(
                            &q,
                            &mut Vec::new(),
                            &mut HashSet::from([start]),
                            start,
                            q.initial_height,
                            q.endpoint_cost(start, true),
                            &mut expected,
                        );
                    }
                    let actual = q.solve(&HashSet::new(), None, None).unwrap();
                    if expected.is_finite() {
                        assert!(
                            (actual.unwrap().cost - expected).abs() < 1e-8,
                            "resource oracle seed={seed}, ratio={ratio}"
                        );
                    } else {
                        assert!(
                            actual.is_none(),
                            "resource oracle seed={seed}, ratio={ratio}"
                        );
                    }
                }
                // Independently enumerate complete simple paths. No queue, heuristic,
                // resource pruning or dominance from the preference solver is reused.
                q.distance_limit = f64::INFINITY;
                let mut candidates = Vec::new();
                for &start in &q.starts {
                    enumerate_solutions(
                        &q,
                        &mut Vec::new(),
                        &mut HashSet::from([start]),
                        start,
                        q.initial_height,
                        &mut candidates,
                    );
                }
                for ratio in [1.0, 1.1, 1.4, f64::INFINITY] {
                    q.distance_limit = ratio
                        * haversine_distance_nm(&departure.coordinate, &destination.coordinate);
                    let feasible: Vec<_> = candidates
                        .iter()
                        .filter(|(_, d)| *d <= q.distance_limit + 1e-8)
                        .collect();
                    let reference = feasible.iter().map(|(s, _)| s.cost).min_by(f64::total_cmp);
                    for tolerance in [0.0, 0.5, 3.0, 20.0] {
                        q.base_limit = reference.unwrap_or(0.0) + tolerance;
                        q.objective = Objective::Preference;
                        let expected = feasible
                            .iter()
                            .filter(|(s, _)| s.cost <= q.base_limit + 1e-8)
                            .map(|(s, _)| s)
                            .min_by(|a, b| {
                                a.exposure
                                    .total_cmp(&b.exposure)
                                    .then(a.changes.cmp(&b.changes))
                                    .then(a.cost.total_cmp(&b.cost))
                            });
                        for bound in [true, false] {
                            q.heuristic = if bound {
                                q.reverse_lower_bound().unwrap()
                            } else {
                                vec![0.0; graph.nodes.len()]
                            };
                            let actual = q.solve(&HashSet::new(), None, None).unwrap();
                            match (expected, actual) {
                                (Some(e), Some(a)) => {
                                    assert!((a.exposure-e.exposure).abs()<1e-8 && a.changes==e.changes && (a.cost-e.cost).abs()<1e-8,
                                        "preference oracle seed={seed} pref={pref:?} ratio={ratio} tolerance={tolerance} bound={bound}: ({},{},{}) vs ({},{},{})",a.exposure,a.changes,a.cost,e.exposure,e.changes,e.cost);
                                    assert!(!has_cycle(&graph, &a));
                                }
                                (None, None) => {}
                                _ => panic!("Preference oracle feasibility mismatch seed={seed}"),
                            }
                        }
                    }
                }
            }
        }
    }

    fn enumerate_solutions(
        q: &Query<'_>,
        path: &mut Vec<usize>,
        visited: &mut HashSet<usize>,
        node: usize,
        height: HeightBand,
        out: &mut Vec<(Solution, f64)>,
    ) {
        if let Some(&last) = path.last() {
            if let Some(&terminal) = q.ends.get(&node) {
                let first = q.graph.edges[path[0]].from;
                let mut cost = q.endpoint_cost(first, true) + terminal + q.join_cost(last, false);
                let mut exposure = 0.0;
                let mut changes = 0;
                for (i, &id) in path.iter().enumerate() {
                    let previous = i.checked_sub(1).map(|j| path[j]);
                    cost += q.edge_cost(id, previous);
                    exposure += q.exposure(id);
                    changes += q.route_change(id, previous);
                }
                let distance = haversine_distance_nm(
                    &q.departure.coordinate,
                    &q.graph.nodes[first].coordinate,
                ) + path
                    .iter()
                    .map(|&id| q.graph.edges[id].distance)
                    .sum::<f64>()
                    + haversine_distance_nm(
                        &q.graph.nodes[node].coordinate,
                        &q.destination.coordinate,
                    );
                out.push((
                    Solution {
                        edges: path.clone(),
                        cost,
                        exposure,
                        changes,
                    },
                    distance,
                ));
            }
        }
        if path.len() >= q.config.max_search_depth {
            return;
        }
        for &id in &q.graph.outgoing[node] {
            let e = &q.graph.edges[id];
            if !q.allowed(id) || visited.contains(&e.to) {
                continue;
            }
            let Some(h) = height.intersect(e.height) else {
                continue;
            };
            path.push(id);
            visited.insert(e.to);
            enumerate_solutions(q, path, visited, e.to, h, out);
            path.pop();
            visited.remove(&e.to);
        }
    }

    #[test]
    fn expired_deadline_is_an_error_not_route_not_found() {
        let graph = tiny_network(1);
        let request = RouteRequest::default();
        let config = Config::default();
        let policy = RoutingPolicy::default();
        let options = SearchOptions::default();
        let a = airport("AAAA", 1.0, 0.0);
        let b = airport("BBBB", 0.9, 0.0);
        let q = Query {
            graph: &graph,
            request: &request,
            config: &config,
            departure: &a,
            destination: &b,
            starts: vec![0],
            ends: HashMap::from([(6, 0.0)]),
            heuristic: vec![0.0; 7],
            initial_height: HeightBand::ALL,
            deadline: Instant::now() - Duration::from_millis(1),
            options: &options,
            distance_limit: f64::INFINITY,
            policy: &policy,
            objective: Objective::Efficiency,
            base_limit: f64::INFINITY,
            remaining: graph
                .nodes
                .iter()
                .map(|p| haversine_distance_nm(&p.coordinate, &b.coordinate))
                .collect(),
            away_cache: (0..graph.edges.len())
                .map(|_| Cell::new(f64::NAN))
                .collect(),
        };
        assert!(
            matches!(q.solve(&HashSet::new(),None,None),Err(RouteKitError::General(message)) if message.contains("budget"))
        );
    }

    #[test]
    fn oceanic_cost_does_not_depend_on_coordinate_subdivision() {
        use crate::oceanic::{OceanicPath, ResolvedPath};
        let base = tiny_network(1);
        let make = |name: &str, latitudes: &[f64]| {
            let spec = OceanicPath {
                identifier: name.into(),
                source: OceanicSource::Simulation {
                    provenance: "test".into(),
                },
                role: PathRole::Oceanic,
                from: base.nodes[0].clone(),
                to: base.nodes[6].clone(),
                via: Vec::new(),
                minimum_fl: 290,
                maximum_fl: 410,
                magnetic_variation_deg: 0.0,
            };
            let mut points = vec![spec.from.clone()];
            points.extend(
                latitudes
                    .iter()
                    .map(|&lat| Waypoint::simple("COORD", lat, 0.0).unwrap()),
            );
            points.push(spec.to.clone());
            ResolvedPath {
                spec,
                points,
                internal_turn_cost: 0.0,
                class: ConnectionClass::Simulation,
                region: None,
                region_valid_until: None,
            }
        };
        let paths = vec![
            make("COARSE", &[]),
            make("FINE", &[0.99, 0.98, 0.97, 0.96, 0.95]),
        ];
        let graph = base.with_supplemental(paths).unwrap();
        let ids: Vec<_> = graph.supplemental.keys().copied().collect();
        let departure = airport("AAAA", 1.01, 0.0);
        let destination = airport("BBBB", 0.92, 0.0);
        let config = Config::default();
        let policy = RoutingPolicy::default();
        let options = SearchOptions::default();
        for pref in [
            RoutePreference::Balanced,
            RoutePreference::AirwayPreferred,
            RoutePreference::DirectPreferred,
        ] {
            let request = RouteRequest {
                route_preference: pref,
                ..Default::default()
            };
            let q = Query {
                graph: &graph,
                request: &request,
                config: &config,
                departure: &departure,
                destination: &destination,
                starts: Vec::new(),
                ends: HashMap::new(),
                heuristic: Vec::new(),
                initial_height: HeightBand::ALL,
                deadline: Instant::now() + Duration::from_secs(5),
                options: &options,
                distance_limit: f64::INFINITY,
                policy: &policy,
                objective: Objective::Efficiency,
                base_limit: f64::INFINITY,
                remaining: graph
                    .nodes
                    .iter()
                    .map(|p| haversine_distance_nm(&p.coordinate, &destination.coordinate))
                    .collect(),
                away_cache: (0..graph.edges.len())
                    .map(|_| Cell::new(f64::NAN))
                    .collect(),
            };
            assert!((q.edge_cost(ids[0], None) - q.edge_cost(ids[1], None)).abs() < 1e-8);
        }
    }
}
