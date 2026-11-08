//! 默认集成测试和基准共用的临时导航数据库，无需本地航行数据。

use rusqlite::{params, Connection};
use tempfile::NamedTempFile;

pub fn setup_parser_db() -> NamedTempFile {
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
    // 仅存在于 VOR 表，避免航路航点掩盖 VOR 回退读取失效。
    insert_vor(&conn, "VORPT", "ZS", "VOR ONLY", 30.85, 121.05);
    insert_wp(&conn, "CHI", "ZY", 39.20, 122.90, "Enroute");
    insert_wp(&conn, "CHI", "YB", -12.55, 130.86666667, "Enroute");
    insert_wp(&conn, "NEAR1", "ZS", 35.15, 120.69, "Enroute");
    insert_wp(&conn, "FARPT", "ES", 54.92, 15.66, "Enroute");

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
    insert_enroute_ndb(&conn, "NDBA", "ZG", "NDB A", 23.50, 113.50);
    insert_terminal_ndb(&conn, "NDBT", "ZG", "NDB T", 23.60, 113.60);

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

fn insert_airway_seg(
    conn: &Connection,
    route: &str,
    seqno: i32,
    id: &str,
    lat: f64,
    lon: f64,
    icao: &str,
) {
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
