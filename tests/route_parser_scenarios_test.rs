use routekit::{RouteElement, RouteKit};
use rusqlite::{params, Connection};
use tempfile::NamedTempFile;

fn setup_parser_db() -> NamedTempFile {
    let tmp = NamedTempFile::new().expect("create temp db");
    let conn = Connection::open(tmp.path()).expect("open temp db");

    conn.execute_batch(
        r#"
        CREATE TABLE tbl_airports (
            icao_code TEXT,
            airport_identifier TEXT,
            airport_identifier_3letter TEXT,
            airport_name TEXT,
            airport_ref_latitude REAL,
            airport_ref_longitude REAL,
            ifr_capability TEXT,
            elevation INTEGER,
            transition_altitude INTEGER,
            transition_level INTEGER
        );

        CREATE TABLE tbl_enroute_waypoints (
            waypoint_identifier TEXT,
            icao_code TEXT,
            waypoint_name TEXT,
            waypoint_latitude REAL,
            waypoint_longitude REAL,
            waypoint_type TEXT,
            waypoint_usage TEXT,
            id TEXT
        );

        CREATE TABLE tbl_terminal_waypoints (
            waypoint_identifier TEXT,
            icao_code TEXT,
            waypoint_name TEXT,
            waypoint_latitude REAL,
            waypoint_longitude REAL,
            id TEXT
        );

        CREATE TABLE tbl_enroute_airways (
            route_identifier TEXT,
            seqno INTEGER,
            waypoint_identifier TEXT,
            waypoint_latitude REAL,
            waypoint_longitude REAL,
            icao_code TEXT,
            route_type TEXT,
            flightlevel TEXT,
            direction_restriction TEXT,
            minimum_altitude1 INTEGER,
            minimum_altitude2 INTEGER,
            maximum_altitude INTEGER,
            outbound_course REAL,
            inbound_course REAL,
            inbound_distance REAL
        );
        "#,
    )
    .expect("create tables");

    conn.execute(
        "INSERT INTO tbl_airports VALUES (?1, ?2, NULL, NULL, ?3, ?4, NULL, NULL, NULL, NULL)",
        params!["ZGGG", "ZGGG", 23.3924_f64, 113.2988_f64],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tbl_airports VALUES (?1, ?2, NULL, NULL, ?3, ?4, NULL, NULL, NULL, NULL)",
        params!["ZSPD", "ZSPD", 31.1434_f64, 121.8052_f64],
    )
    .unwrap();

    // 唯一航点
    insert_wp(&conn, "SUPAR", "ZS", 30.80, 121.00, "Enroute");
    insert_wp(&conn, "TEPID", "ZS", 30.90, 121.10, "Enroute");
    insert_wp(&conn, "MULOV", "ZS", 30.95, 121.15, "Enroute");

    // 重名航点 AND：一个中国、一个挪威
    insert_wp(&conn, "AND", "ZS", 30.25666667, 121.22166667, "Enroute");
    insert_wp(&conn, "AND", "EN", 69.28782778, 16.14137778, "VOR");

    // B221：故意放入两个 AND（不同坐标）验证航路内部歧义选择
    insert_airway_seg(&conn, "B221", 10, "SUPAR", 30.80, 121.00, "ZS");
    insert_airway_seg(&conn, "B221", 20, "AND", 69.28782778, 16.14137778, "EN");
    insert_airway_seg(&conn, "B221", 30, "MULOV", 30.95, 121.15, "ZS");
    insert_airway_seg(&conn, "B221", 40, "AND", 30.25666667, 121.22166667, "ZS");

    // W90：用于反向截取测试
    insert_airway_seg(&conn, "W90", 10, "TEPID", 30.90, 121.10, "ZS");
    insert_airway_seg(&conn, "W90", 20, "MULOV", 30.95, 121.15, "ZS");
    insert_airway_seg(&conn, "W90", 30, "SUPAR", 30.80, 121.00, "ZS");

    tmp
}

fn insert_wp(conn: &Connection, id: &str, icao: &str, lat: f64, lon: f64, wp_type: &str) {
    conn.execute(
        "INSERT INTO tbl_enroute_waypoints
         (waypoint_identifier, icao_code, waypoint_name, waypoint_latitude, waypoint_longitude, waypoint_type, waypoint_usage, id)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, NULL, NULL)",
        params![id, icao, lat, lon, wp_type],
    )
    .unwrap();
}

fn insert_airway_seg(conn: &Connection, route: &str, seqno: i32, id: &str, lat: f64, lon: f64, icao: &str) {
    conn.execute(
        "INSERT INTO tbl_enroute_airways
         (route_identifier, seqno, waypoint_identifier, waypoint_latitude, waypoint_longitude, icao_code, route_type,
          flightlevel, direction_restriction, minimum_altitude1, minimum_altitude2, maximum_altitude, outbound_course, inbound_course, inbound_distance)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'R', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
        params![route, seqno, id, lat, lon, icao],
    )
    .unwrap();
}

fn get_first_airway_segments(parsed: &routekit::ParsedRoute, airway_id: &str) -> Vec<routekit::AirwaySegment> {
    parsed
        .elements
        .iter()
        .find_map(|e| match e {
            RouteElement::Airway { identifier, segments } if identifier == airway_id => Some(segments.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn test_dct_duplicate_waypoint_prefers_nearest_continuity() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR DCT AND").expect("parse");
    let direct_to = parsed.elements.iter().find_map(|e| match e {
        RouteElement::Direct { to, .. } => Some(to.clone()),
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
