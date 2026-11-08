use routekit::{FlightLevel, RouteKit, RoutePreference, RouteRequest};

fn main() -> routekit::Result<()> {
    let mut args = std::env::args().skip(1);
    let database = args
        .next()
        .unwrap_or_else(|| "raw_data/e_dfd_PMDG.s3db".into());
    let departure = args.next().unwrap_or_else(|| "ZBAA".into());
    let destination = args.next().unwrap_or_else(|| "ZSPD".into());
    let kit = RouteKit::new(database)?;
    let request = RouteRequest {
        departure_icao: departure,
        destination_icao: destination,
        flight_level: Some(FlightLevel::High),
        route_preference: RoutePreference::Balanced,
        max_routes: 3,
    };

    // 完整结果包含逐段航点、航路、距离和高度范围。
    for route in kit.find_routes(&request)? {
        println!(
            "{} ({:.1} NM)",
            route.to_route_string(),
            route.total_distance_nm
        );
    }
    // 简化接口适合只需要航路文本与距离的调用方。
    println!(
        "{}",
        serde_json::to_string_pretty(&kit.find_routes_simple(&request)?)?
    );
    Ok(())
}
