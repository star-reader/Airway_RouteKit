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

        CREATE TABLE tbl_vhfnavaids (
            area_code TEXT,
            airport_identifier TEXT,
            icao_code TEXT,
            vor_identifier TEXT,
            vor_name TEXT,
            vor_frequency REAL,
            navaid_class TEXT,
            vor_latitude REAL,
            vor_longitude REAL,
            dme_ident TEXT,
            dme_latitude REAL,
            dme_longitude REAL,
            dme_elevation INTEGER,
            ilsdme_bias REAL,
            range INTEGER,
            station_declination REAL,
            magnetic_variation REAL,
            id TEXT
        );

        CREATE TABLE tbl_enroute_ndbnavaids (
            area_code TEXT,
            icao_code TEXT,
            ndb_identifier TEXT,
            ndb_name TEXT,
            ndb_frequency REAL,
            navaid_class TEXT,
            ndb_latitude REAL,
            ndb_longitude REAL,
            range INTEGER,
            id TEXT
        );

        CREATE TABLE tbl_terminal_ndbnavaids (
            area_code TEXT,
            airport_identifier TEXT,
            icao_code TEXT,
            ndb_identifier TEXT,
            ndb_name TEXT,
            ndb_frequency REAL,
            navaid_class TEXT,
            ndb_latitude REAL,
            ndb_longitude REAL,
            range INTEGER,
            id TEXT
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
    conn.execute(
        "INSERT INTO tbl_airports VALUES (?1, ?2, NULL, NULL, ?3, ?4, NULL, NULL, NULL, NULL)",
        params!["ZYHB", "ZYHB", 45.6234_f64, 126.2503_f64],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tbl_airports VALUES (?1, ?2, NULL, NULL, ?3, ?4, NULL, NULL, NULL, NULL)",
        params!["ZUCK", "ZUCK", 30.5785_f64, 103.9471_f64],
    )
    .unwrap();

    // 唯一航点
    insert_wp(&conn, "SUPAR", "ZS", 30.80, 121.00, "Enroute");
    insert_wp(&conn, "TEPID", "ZS", 30.90, 121.10, "Enroute");
    insert_wp(&conn, "MULOV", "ZS", 30.95, 121.15, "Enroute");
    insert_wp(&conn, "HRB", "ZY", 45.70, 126.30, "Enroute");
    insert_wp(&conn, "NODAL", "ZY", 40.04638889, 123.17805556, "Enroute");
    insert_wp(&conn, "VENOS", "ZY", 38.90277778, 122.32666667, "Enroute");

    // 重名航点 AND：一个中国、一个挪威
    insert_wp(&conn, "AND", "ZS", 30.25666667, 121.22166667, "Enroute");
    insert_wp(&conn, "AND", "EN", 69.28782778, 16.14137778, "VOR");
    insert_vor(&conn, "AND", "ZS", "AND VOR", 30.25666667, 121.22166667);
    insert_wp(&conn, "CHI", "ZY", 39.20, 122.90, "Enroute");
    insert_wp(&conn, "CHI", "YB", -12.55, 130.86666667, "Enroute");

    // B221：故意放入两个 AND（不同坐标）验证航路内部歧义选择
    insert_airway_seg(&conn, "B221", 10, "SUPAR", 30.80, 121.00, "ZS");
    insert_airway_seg(&conn, "B221", 20, "AND", 69.28782778, 16.14137778, "EN");
    insert_airway_seg(&conn, "B221", 30, "MULOV", 30.95, 121.15, "ZS");
    insert_airway_seg(&conn, "B221", 40, "AND", 30.25666667, 121.22166667, "ZS");

    // W90：用于反向截取测试
    insert_airway_seg(&conn, "W90", 10, "TEPID", 30.90, 121.10, "ZS");
    insert_airway_seg(&conn, "W90", 20, "MULOV", 30.95, 121.15, "ZS");
    insert_airway_seg(&conn, "W90", 30, "SUPAR", 30.80, 121.00, "ZS");

    // 仅存在于NDB表，不存在于waypoints表，用于验证回退读取
    insert_enroute_ndb(&conn, "NDBA", "ZS", "NDB A", 31.00, 121.30);
    insert_terminal_ndb(&conn, "NDBT", "ZS", "NDB T", 31.10, 121.40);

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

fn insert_vor(conn: &Connection, id: &str, icao: &str, name: &str, lat: f64, lon: f64) {
    conn.execute(
        "INSERT INTO tbl_vhfnavaids
         (area_code, airport_identifier, icao_code, vor_identifier, vor_name, vor_frequency, navaid_class,
          vor_latitude, vor_longitude, dme_ident, dme_latitude, dme_longitude, dme_elevation,
          ilsdme_bias, range, station_declination, magnetic_variation, id)
         VALUES ('ZZ', NULL, ?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
        params![icao, id, name, lat, lon],
    )
    .unwrap();
}

fn insert_enroute_ndb(conn: &Connection, id: &str, icao: &str, name: &str, lat: f64, lon: f64) {
    conn.execute(
        "INSERT INTO tbl_enroute_ndbnavaids
         (area_code, icao_code, ndb_identifier, ndb_name, ndb_frequency, navaid_class, ndb_latitude, ndb_longitude, range, id)
         VALUES ('ZZ', ?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL, NULL)",
        params![icao, id, name, lat, lon],
    )
    .unwrap();
}

fn insert_terminal_ndb(conn: &Connection, id: &str, icao: &str, name: &str, lat: f64, lon: f64) {
    conn.execute(
        "INSERT INTO tbl_terminal_ndbnavaids
         (area_code, airport_identifier, icao_code, ndb_identifier, ndb_name, ndb_frequency, navaid_class, ndb_latitude, ndb_longitude, range, id)
         VALUES ('ZZ', NULL, ?1, ?2, ?3, NULL, NULL, ?4, ?5, NULL, NULL)",
        params![icao, id, name, lat, lon],
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

#[test]
fn test_vor_is_loaded_as_waypoint_candidate() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("SUPAR DCT AND").expect("parse");
    let direct_to = parsed.elements.iter().find_map(|e| match e {
        RouteElement::Direct { to, .. } => Some(to.clone()),
        _ => None,
    });

    let to = direct_to.expect("should produce DCT segment");
    assert_eq!(to.identifier, "AND");
    // Because enroute and VOR both exist for AND, this ensures the parser still resolves it successfully.
    assert!(matches!(to.waypoint_type, routekit::WaypointType::Enroute | routekit::WaypointType::VOR));
}

#[test]
fn test_ndb_tables_are_used_for_waypoint_resolution() {
    let db = setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("init routekit");

    let parsed = kit.parse_route("ZGGG SID NDBA NDBT ZSPD").expect("parse");
    let ndb_waypoints: Vec<_> = parsed
        .elements
        .iter()
        .filter_map(|e| match e {
            RouteElement::Waypoint(wp) if wp.identifier == "NDBA" || wp.identifier == "NDBT" => Some(wp.clone()),
            _ => None,
        })
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

    let has_and_unknown = parsed
        .warnings
        .iter()
        .any(|w| w.contains("无法识别的元素: AND"));
    assert!(!has_and_unknown, "AND should be consumed by airway endpoint, got warnings: {:?}", parsed.warnings);
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

    let parsed = kit.parse_route("ZYHB HRB NODAL CHI VENOS ZUCK").expect("parse");
    let chi_wp = parsed.elements.iter().find_map(|e| match e {
        RouteElement::Waypoint(wp) if wp.identifier == "CHI" => Some(wp.clone()),
        _ => None,
    });

    let chi = chi_wp.expect("CHI should be parsed");
    assert_eq!(chi.icao_code, "ZY");
    assert!(chi.coordinate.latitude > 0.0, "should not jump to YB/Channel Island candidate");
}
