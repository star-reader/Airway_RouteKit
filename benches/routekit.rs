//! 使用固定测试数据衡量几何计算、航路解析和空间查询，不依赖本地导航数据库。

#[path = "../tests/support/database.rs"]
mod database;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use routekit::geo::{calculate_bearing, haversine_distance_nm};
use routekit::{Coordinate, RouteElement, RouteKit};

fn benchmark_geometry(c: &mut Criterion) {
    let from = Coordinate::new(40.0, 116.0).unwrap();
    let to = Coordinate::new(31.0, 121.0).unwrap();

    c.bench_function("haversine_distance", |b| {
        b.iter(|| haversine_distance_nm(black_box(&from), black_box(&to)))
    });
    c.bench_function("calculate_bearing", |b| {
        b.iter(|| calculate_bearing(black_box(&from), black_box(&to)))
    });
}

fn benchmark_routekit(c: &mut Criterion) {
    // 建库和索引初始化放在计时循环之外。
    let db = database::setup_parser_db();
    let kit = RouteKit::new(db.path()).expect("initialize fixture database");
    let route = "ZSPD TEPID W90 SUPAR ZGGG";
    let parsed = kit.parse_route(route).expect("parse fixture route");
    assert!(
        parsed.is_valid && parsed.warnings.is_empty(),
        "invalid benchmark route: {parsed:?}"
    );
    assert!(parsed.elements.iter().any(|element| matches!(
        element,
        RouteElement::Airway { identifier, segments } if identifier == "W90" && segments.len() == 3
    )));

    c.bench_function("parse_route", |b| {
        b.iter(|| {
            kit.parse_route(black_box(route))
                .expect("parse fixture route")
        })
    });

    let coordinate = Coordinate::new(30.90, 121.10).unwrap();
    assert_eq!(
        kit.find_nearest_waypoint(&coordinate).unwrap().identifier,
        "TEPID"
    );
    assert!(!kit
        .find_waypoints_within_radius(&coordinate, 50.0)
        .is_empty());

    c.bench_function("find_nearest_waypoint", |b| {
        b.iter(|| kit.find_nearest_waypoint(black_box(&coordinate)))
    });
    c.bench_function("find_waypoints_within_radius", |b| {
        b.iter(|| kit.find_waypoints_within_radius(black_box(&coordinate), black_box(50.0)))
    });
}

criterion_group!(benches, benchmark_geometry, benchmark_routekit);
criterion_main!(benches);
