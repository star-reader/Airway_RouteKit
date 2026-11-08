//! 无需外部数据库的公共 API 和航路解析回归测试。

#[path = "support/database.rs"]
mod database;
#[path = "support/route.rs"]
mod route;

use database::setup_parser_db;
use route::find_resolved_waypoint;
use routekit::{RouteElement, RouteKit};

#[test]
fn test_routekit_initialization() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("initialize RouteKit with fixture database");
    let coordinate = routekit::Coordinate::new(30.90, 121.10).unwrap();
    let nearest = kit
        .find_nearest_waypoint(&coordinate)
        .expect("spatial index should be populated");
    assert_eq!(nearest.identifier, "TEPID");
}

#[test]
fn test_load_airport() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");
    let airport = kit
        .load_airport("ZSPD")
        .expect("fixture airport should load");
    assert_eq!(airport.identifier, "ZSPD");
    assert!((airport.coordinate.latitude - 31.1434).abs() < 1e-6);
    assert!((airport.coordinate.longitude - 121.8052).abs() < 1e-6);
    assert!(matches!(
        kit.load_airport("ZZZZ"),
        Err(routekit::RouteKitError::AirportNotFound(_))
    ));
}

#[test]
fn test_parse_simple_route() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");
    let parsed = kit
        .parse_route("ZGGG ZSPD")
        .expect("parse airport-only route");
    assert!(parsed.is_valid, "invalid route: {:?}", parsed.errors);
    assert_eq!(parsed.departure.as_ref().unwrap().identifier, "ZGGG");
    assert_eq!(parsed.destination.as_ref().unwrap().identifier, "ZSPD");
    assert!(
        parsed.warnings.is_empty(),
        "unexpected warnings: {:?}",
        parsed.warnings
    );
}

fn get_first_airway_segments<'a>(
    parsed: &'a routekit::ParsedRoute,
    airway_id: &str,
) -> &'a [routekit::AirwaySegment] {
    parsed
        .elements
        .iter()
        .find_map(|e| match e {
            RouteElement::Airway {
                identifier,
                segments,
            } if identifier == airway_id => Some(segments.as_slice()),
            _ => None,
        })
        .expect("expected airway should be present")
}

#[test]
fn test_consecutive_waypoints_emit_implicit_dct_segments() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR AND MULOV").expect("parse");

    let direct_count = parsed
        .elements
        .iter()
        .filter(|e| matches!(e, RouteElement::Direct { .. }))
        .count();
    assert_eq!(
        direct_count, 2,
        "two consecutive hops should emit two DCT segments"
    );

    let first_is_waypoint = matches!(parsed.elements.first(), Some(RouteElement::Waypoint(wp)) if wp.identifier == "SUPAR");
    assert!(
        first_is_waypoint,
        "first fix should remain a Waypoint element"
    );
}

#[test]
fn test_dct_duplicate_waypoint_prefers_nearest_continuity() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR DCT AND").expect("parse");
    let direct_to = parsed.elements.iter().find_map(|e| match e {
        RouteElement::Direct { to, .. } => Some(to),
        _ => None,
    });

    let to = direct_to.expect("should produce DCT segment");
    assert_eq!(to.icao_code, "ZS");
    assert!((to.coordinate.latitude - 30.25666667).abs() < 0.001);
}

#[test]
fn test_airway_duplicate_waypoint_prefers_contextual_match() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR B221 AND").expect("parse");
    let segments = get_first_airway_segments(&parsed, "B221");
    assert!(!segments.is_empty());

    let last = segments.last().unwrap();
    assert_eq!(last.waypoint.identifier, "AND");
    assert_eq!(last.waypoint.icao_code, "ZS");
    assert!((last.waypoint.coordinate.latitude - 30.25666667).abs() < 0.001);
}

#[test]
fn test_airway_reverse_direction_segment_extraction() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR W90 TEPID").expect("parse");
    let segments = get_first_airway_segments(&parsed, "W90");

    assert_eq!(segments.len(), 3);
    assert_eq!(segments.first().unwrap().waypoint.identifier, "SUPAR");
    assert_eq!(segments.last().unwrap().waypoint.identifier, "TEPID");
}

#[test]
fn test_unknown_waypoint_kept_as_unknown_token() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR NOTEXIST").expect("parse");
    let has_unknown = parsed
        .elements
        .iter()
        .any(|e| matches!(e, RouteElement::Unknown(x) if x == "NOTEXIST"));
    assert!(has_unknown);
}

#[test]
fn test_vor_table_is_used_for_waypoint_resolution() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR DCT VORPT").expect("parse");
    let vor = find_resolved_waypoint(&parsed, "VORPT").expect("VOR-only waypoint should resolve");
    assert_eq!(vor.waypoint_type, routekit::WaypointType::VOR);
    assert_eq!(vor.icao_code, "ZS");
}

#[test]
fn test_ndb_tables_are_used_for_waypoint_resolution() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("ZGGG SID NDBA NDBT ZSPD").expect("parse");

    let ndb_waypoints: Vec<_> = ["NDBA", "NDBT"]
        .iter()
        .filter_map(|id| find_resolved_waypoint(&parsed, id))
        .collect();

    assert_eq!(ndb_waypoints.len(), 2);
    assert!(ndb_waypoints
        .iter()
        .all(|wp| matches!(wp.waypoint_type, routekit::WaypointType::NDB)));
}

#[test]
fn test_airway_endpoint_token_is_not_reparsed_as_unknown() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit
        .parse_route("ZGGG TAN W24 TEPID W90 NOLON A599 ELNEX G204 MULOV V73 SUPAR B221 AND ZSPD")
        .expect("parse");

    let segments = get_first_airway_segments(&parsed, "B221");
    assert_eq!(segments.last().unwrap().waypoint.identifier, "AND");
    assert!(!parsed
        .elements
        .iter()
        .any(|element| matches!(element, RouteElement::Unknown(id) if id == "AND")));

    let has_and_unknown = parsed
        .warnings
        .iter()
        .any(|w| w.contains("无法识别的元素: AND"));
    assert!(
        !has_and_unknown,
        "AND should be consumed by airway endpoint, got warnings: {:?}",
        parsed.warnings
    );
}

#[test]
fn test_first_token_after_departure_is_not_forced_sid() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("ZYHB HRB NODAL VENOS ZUCK").expect("parse");

    let has_sid = parsed
        .elements
        .iter()
        .any(|e| matches!(e, RouteElement::SID(_)));
    assert!(!has_sid, "HRB should be parsed as waypoint, not SID");

    let first_wp = parsed.elements.iter().find_map(|e| match e {
        RouteElement::Waypoint(wp) => Some(wp.identifier.clone()),
        _ => None,
    });
    assert_eq!(first_wp.as_deref(), Some("HRB"));
}

#[test]
fn test_duplicate_identifier_prefers_local_continuity_over_far_region() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit
        .parse_route("ZYHB HRB NODAL CHI VENOS ZUCK")
        .expect("parse");
    let chi = find_resolved_waypoint(&parsed, "CHI").expect("CHI should be parsed");
    assert_eq!(chi.icao_code, "ZY");
    assert!(
        chi.coordinate.latitude > 0.0,
        "should not jump to YB/Channel Island candidate"
    );
}

#[test]
fn test_single_far_candidate_is_skipped_instead_of_forced_match() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("ZYHB NEAR1 FARPT ZSPD").expect("parse");

    assert!(
        find_resolved_waypoint(&parsed, "FARPT").is_none(),
        "FARPT should not resolve as a waypoint or a direct segment endpoint"
    );

    let has_unknown_farpt = parsed.elements.iter().any(|e| match e {
        RouteElement::Unknown(id) => id == "FARPT",
        _ => false,
    });
    assert!(has_unknown_farpt, "FARPT should remain as unknown token");
}
