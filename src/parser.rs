use crate::database::DatabasePool;
use crate::error::{Result, RouteKitError};
use crate::geo::haversine_distance_nm;
use crate::models::*;
use crate::utils::{is_runway_pattern, split_route_string, validate_icao};
use std::collections::HashMap;
use std::sync::Arc;

pub struct RouteParser {
    db_pool: Arc<DatabasePool>,
}

struct State {
    last: Option<Waypoint>,
    score: f64,
    errors: usize,
    parent: Option<usize>,
    element: Option<RouteElement>,
    message: Option<String>,
}

fn key(point: &Option<Waypoint>) -> String {
    point
        .as_ref()
        .map(|p| {
            format!(
                "{}|{}|{:?}|{:?}|{:.7}|{:.7}",
                p.identifier,
                p.icao_code,
                p.waypoint_type,
                p.id,
                p.coordinate.latitude,
                p.coordinate.longitude
            )
        })
        .unwrap_or_default()
}

fn add_state(
    arena: &mut Vec<State>,
    positions: &mut [HashMap<String, usize>],
    pos: usize,
    state: State,
) {
    let identity = key(&state.last);
    if let Some(&old) = positions[pos].get(&identity) {
        if arena[old].errors < state.errors
            || (arena[old].errors == state.errors && arena[old].score <= state.score)
        {
            return;
        }
    }
    let id = arena.len();
    arena.push(state);
    positions[pos].insert(identity, id);
}

impl RouteParser {
    pub fn new(db_pool: Arc<DatabasePool>) -> Self {
        Self { db_pool }
    }

    pub fn parse(&self, route_string: &str) -> Result<ParsedRoute> {
        let mut parsed = ParsedRoute {
            raw_input: route_string.into(),
            departure: None,
            destination: None,
            sid: None,
            star: None,
            elements: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            is_valid: false,
        };
        let normalized = route_string
            .to_uppercase()
            .replace(['(', ')', '"', '\'', '[', ']', '{', '}', '，', '。'], " ")
            .replace("..", " ");
        let mut tokens = split_route_string(&normalized);
        tokens.retain(|t| !matches!(t.as_str(), "VIA" | "TO" | "FROM" | "AT" | "THE"));
        if tokens.is_empty() {
            parsed.errors.push("输入为空".into());
            return Ok(parsed);
        }
        if tokens.len() > 512 {
            return Err(RouteKitError::ParseError("Route exceeds 512 tokens".into()));
        }
        if validate_icao(&tokens[0]) {
            parsed.departure = self.db_pool.load_airport(&tokens[0]).ok();
        }
        if parsed.departure.is_some() {
            tokens.remove(0);
        }
        if let Some(last) = tokens.last().filter(|s| validate_icao(s)) {
            parsed.destination = self.db_pool.load_airport(last).ok();
        }
        if parsed.destination.is_some() {
            tokens.pop();
        }
        tokens.retain(|t| {
            if is_runway_pattern(t) {
                parsed.warnings.push(format!("Runway not formatted: {t}"));
                false
            } else {
                true
            }
        });
        let graph = self.db_pool.network()?;
        let mut candidates = Vec::with_capacity(tokens.len());
        let mut annotations = Vec::with_capacity(tokens.len());
        for token in &tokens {
            let points = if token == "DCT" || token == "SID" || token == "STAR" {
                Vec::new()
            } else if let Some(point) = crate::oceanic::coordinate_waypoint(token) {
                vec![point]
            } else {
                self.db_pool.find_waypoints(token)?
            };
            let annotation = if token == "SID" {
                Some(RouteElement::SID(token.clone()))
            } else if token == "STAR" {
                Some(RouteElement::STAR(token.clone()))
            } else if points.is_empty() && !graph.airway_rows.contains_key(token) {
                if parsed.departure.as_ref().is_some_and(|a| {
                    self.db_pool
                        .procedure_exists(&a.identifier, token, true)
                        .unwrap_or(false)
                }) {
                    Some(RouteElement::SID(token.clone()))
                } else if parsed.destination.as_ref().is_some_and(|a| {
                    self.db_pool
                        .procedure_exists(&a.identifier, token, false)
                        .unwrap_or(false)
                }) {
                    Some(RouteElement::STAR(token.clone()))
                } else {
                    None
                }
            } else {
                None
            };
            candidates.push(points);
            annotations.push(annotation);
        }
        let mut arena = vec![State {
            last: None,
            score: 0.0,
            errors: 0,
            parent: None,
            element: None,
            message: None,
        }];
        let mut positions: Vec<HashMap<String, usize>> =
            (0..=tokens.len()).map(|_| HashMap::new()).collect();
        positions[0].insert(String::new(), 0);
        for i in 0..tokens.len() {
            let mut current: Vec<_> = positions[i].values().copied().collect();
            current.sort_unstable();
            for at in current {
                let last = arena[at].last.clone();
                let score = arena[at].score;
                let errors = arena[at].errors;
                let token = &tokens[i];
                if let Some(element) = annotations[i].clone() {
                    add_state(
                        &mut arena,
                        &mut positions,
                        i + 1,
                        State {
                            last,
                            score,
                            errors,
                            parent: Some(at),
                            element: Some(element),
                            message: None,
                        },
                    );
                    continue;
                }
                for point in &candidates[i] {
                    let distance =
                        last.as_ref()
                            .map(|p| haversine_distance_nm(&p.coordinate, &point.coordinate))
                            .or_else(|| {
                                parsed.departure.as_ref().map(|a| {
                                    haversine_distance_nm(&a.coordinate, &point.coordinate)
                                })
                            })
                            .unwrap_or(0.0);
                    // Sanity limits for unqualified hops, not authorization limits on explicit DCT.
                    if distance > 3000.0
                        || (last.is_none() && parsed.departure.is_some() && distance > 1000.0)
                    {
                        continue;
                    }
                    let element = match &last {
                        Some(from) => RouteElement::Direct {
                            from: from.clone(),
                            to: point.clone(),
                        },
                        None => RouteElement::Waypoint(point.clone()),
                    };
                    add_state(
                        &mut arena,
                        &mut positions,
                        i + 1,
                        State {
                            last: Some(point.clone()),
                            score: score + distance,
                            errors,
                            parent: Some(at),
                            element: Some(element),
                            message: None,
                        },
                    );
                }
                if let Some(from) = &last {
                    if i + 1 < tokens.len() {
                        for to in &candidates[i + 1] {
                            if token == "DCT" {
                                let distance =
                                    haversine_distance_nm(&from.coordinate, &to.coordinate);
                                add_state(
                                    &mut arena,
                                    &mut positions,
                                    i + 2,
                                    State {
                                        last: Some(to.clone()),
                                        score: score + distance,
                                        errors,
                                        parent: Some(at),
                                        element: Some(RouteElement::Direct {
                                            from: from.clone(),
                                            to: to.clone(),
                                        }),
                                        message: None,
                                    },
                                );
                            } else if graph.airway_rows.contains_key(token) {
                                for path in graph.expand(token, from, to) {
                                    let distance: f64 = path
                                        .windows(2)
                                        .map(|p| {
                                            haversine_distance_nm(
                                                &p[0].waypoint.coordinate,
                                                &p[1].waypoint.coordinate,
                                            )
                                        })
                                        .sum();
                                    let last = path.last().unwrap().waypoint.clone();
                                    add_state(
                                        &mut arena,
                                        &mut positions,
                                        i + 2,
                                        State {
                                            last: Some(last),
                                            score: score + distance,
                                            errors,
                                            parent: Some(at),
                                            element: Some(RouteElement::Airway {
                                                identifier: token.clone(),
                                                segments: path,
                                            }),
                                            message: None,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                let message = if token == "DCT" {
                    "DCT 缺少明确的起点或终点".to_owned()
                } else if graph.airway_rows.contains_key(token) {
                    format!("航路 {token} 的端点、连续性或方向不成立")
                } else {
                    format!("无法识别或无法连续解析的元素: {token}")
                };
                // An unresolved token explicitly breaks continuity. No Direct is bridged over it.
                add_state(
                    &mut arena,
                    &mut positions,
                    i + 1,
                    State {
                        last: None,
                        score,
                        errors: errors + 1,
                        parent: Some(at),
                        element: Some(RouteElement::Unknown(token.clone())),
                        message: Some(message),
                    },
                );
                if arena.len() > 100_000 {
                    return Err(RouteKitError::ParseError(
                        "Ambiguous route exceeds parser state budget".into(),
                    ));
                }
            }
        }
        let final_score = |s: &State| {
            s.score
                + s.last
                    .as_ref()
                    .zip(parsed.destination.as_ref())
                    .map(|(p, a)| haversine_distance_nm(&p.coordinate, &a.coordinate))
                    .unwrap_or(0.0)
        };
        let best = positions[tokens.len()]
            .values()
            .copied()
            .min_by(|&a, &b| {
                arena[a]
                    .errors
                    .cmp(&arena[b].errors)
                    .then_with(|| final_score(&arena[a]).total_cmp(&final_score(&arena[b])))
                    .then(a.cmp(&b))
            })
            .unwrap_or(0);
        let mut at = best;
        loop {
            if let Some(element) = arena[at].element.clone() {
                parsed.elements.push(element);
            }
            if let Some(message) = &arena[at].message {
                parsed.errors.push(message.clone());
            }
            match arena[at].parent {
                Some(parent) => at = parent,
                None => break,
            }
        }
        parsed.elements.reverse();
        parsed.errors.reverse();
        for element in &parsed.elements {
            match element {
                RouteElement::SID(id) => parsed.sid = Some(id.clone()),
                RouteElement::STAR(id) => parsed.star = Some(id.clone()),
                _ => {}
            }
        }
        if parsed
            .elements
            .iter()
            .any(|e| matches!(e, RouteElement::Direct { .. }))
        {
            parsed
                .warnings
                .push("DCT 已按输入解析；未验证区域、时间和运行许可".into());
        }
        if parsed.sid.is_some() || parsed.star.is_some() {
            parsed.warnings.push("SID/STAR not formatted".into());
        }
        parsed.is_valid =
            parsed.errors.is_empty() && parsed.departure.is_some() && parsed.destination.is_some();
        Ok(parsed)
    }

    pub fn parse_flexible(&self, route_string: &str) -> Result<ParsedRoute> {
        // Partial elements are always returned; unresolved input never becomes valid by omission.
        self.parse(route_string)
    }
}
