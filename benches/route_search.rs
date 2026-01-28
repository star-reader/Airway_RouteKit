/// 性能基准测试

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use routekit::*;

const TEST_DB_PATH: &str = "raw_data/e_dfd_PMDG.s3db";

fn benchmark_distance_calculation(c: &mut Criterion) {
    use routekit::geo::haversine_distance_nm;
    
    let coord1 = Coordinate::new(40.0, 116.0).unwrap();
    let coord2 = Coordinate::new(31.0, 121.0).unwrap();
    
    c.bench_function("haversine_distance", |b| {
        b.iter(|| {
            haversine_distance_nm(black_box(&coord1), black_box(&coord2))
        })
    });
}

fn benchmark_bearing_calculation(c: &mut Criterion) {
    use routekit::geo::calculate_bearing;
    
    let coord1 = Coordinate::new(40.0, 116.0).unwrap();
    let coord2 = Coordinate::new(31.0, 121.0).unwrap();
    
    c.bench_function("calculate_bearing", |b| {
        b.iter(|| {
            calculate_bearing(black_box(&coord1), black_box(&coord2))
        })
    });
}

fn benchmark_route_parsing(c: &mut Criterion) {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).unwrap();
    let route_string = "ZBAA SID TEPID G212 VYK STAR ZSPD";
    
    c.bench_function("parse_route", |b| {
        b.iter(|| {
            kit.parse_route(black_box(route_string))
        })
    });
}

fn benchmark_spatial_search(c: &mut Criterion) {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).unwrap();
    let coord = Coordinate::new(40.0, 116.0).unwrap();
    
    c.bench_function("find_nearest_waypoint", |b| {
        b.iter(|| {
            kit.find_nearest_waypoint(black_box(&coord))
        })
    });
    
    c.bench_function("find_waypoints_within_radius", |b| {
        b.iter(|| {
            kit.find_waypoints_within_radius(black_box(&coord), black_box(50.0))
        })
    });
}

criterion_group!(
    benches,
    benchmark_distance_calculation,
    benchmark_bearing_calculation,
    benchmark_route_parsing,
    benchmark_spatial_search
);

criterion_main!(benches);
