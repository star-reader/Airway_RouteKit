use routekit::{Config, FlightLevel, PlanningContext, RouteKit, RoutePreference, RouteRequest};

fn main() -> routekit::Result<()> {
    let mut args = std::env::args().skip(1);
    let database = args
        .next()
        .unwrap_or_else(|| "raw_data/e_navdata.s3db".into());
    let departure = args.next().unwrap_or_else(|| "ZBAA".into());
    let destination = args.next().unwrap_or_else(|| "ZSPD".into());
    let config = Config::builder().search_timeout_ms(10_000).build()?;
    let kit = RouteKit::with_config(database, config)?;
    let request = RouteRequest {
        departure_icao: departure,
        destination_icao: destination,
        flight_level: Some(FlightLevel::Custom(350)),
        route_preference: RoutePreference::Balanced,
        max_routes: 1,
    };
    // 调用方可以通过 context.options 提供补充路径，通过 context.regions 提供区域规则。
    let context = PlanningContext::default();
    let planned = kit.find_routes_with_context(&request, &context)?;
    println!("{}", serde_json::to_string_pretty(&planned)?);
    if let Some(route) = planned.result.routes.first() {
        let preview = routekit::geojson::route_geojson(route, 25.0)?;
        println!("{}", serde_json::to_string_pretty(&preview)?);
    }
    Ok(())
}
