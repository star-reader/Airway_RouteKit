/// 基本使用示例

use routekit::*;

fn main() -> Result<()> {
    // 初始化日志
    env_logger::init();

    println!("=== RouteKit 基本使用示例 ===\n");

    // 1. 创建RouteKit实例
    println!("1. 初始化RouteKit...");
    let kit = RouteKit::new("raw_data/e_dfd_PMDG.s3db")?;
    println!("   ✓ RouteKit初始化成功\n");

    // 2. 加载机场信息
    println!("2. 加载机场信息...");
    match kit.load_airport("ZBAA") {
        Ok(airport) => {
            println!("   机场代码: {}", airport.identifier);
            println!("   机场名称: {:?}", airport.name);
            println!("   坐标: {:.4}°N, {:.4}°E", 
                airport.coordinate.latitude, 
                airport.coordinate.longitude);
            println!("   海拔: {:?} 英尺", airport.elevation);
        }
        Err(e) => println!("   加载机场失败: {}", e),
    }
    println!();

    // 3. 查找航点
    println!("3. 查找航点...");
    match kit.find_waypoint("TEPID") {
        Ok(Some(waypoint)) => {
            println!("   航点标识: {}", waypoint.identifier);
            println!("   坐标: {:.4}°N, {:.4}°E", 
                waypoint.coordinate.latitude, 
                waypoint.coordinate.longitude);
        }
        Ok(None) => println!("   航点未找到"),
        Err(e) => println!("   查询失败: {}", e),
    }
    println!();

    // 4. 空间搜索
    println!("4. 空间搜索 - 查找指定位置附近的航点...");
    let center = Coordinate::new(40.0, 116.0)?;
    let nearby = kit.find_waypoints_within_radius(&center, 50.0);
    println!("   在50海里半径内找到 {} 个航点", nearby.len());
    if !nearby.is_empty() {
        println!("   最近的5个航点:");
        for wp in nearby.iter().take(5) {
            println!("     - {}", wp.identifier);
        }
    }
    println!();

    // 5. 解析航路字符串
    println!("5. 解析航路字符串...");
    let route_strings = vec![
        "ZBAA ZSPD",
        "ZBAA SID TEPID G212 VYK STAR ZSPD",
        "ZBAA -> ZSPD via A593",
    ];

    for route_str in route_strings {
        println!("   输入: {}", route_str);
        match kit.parse_route(route_str) {
            Ok(parsed) => {
                println!("     有效: {}", parsed.is_valid);
                println!("     元素数: {}", parsed.elements.len());
                if !parsed.warnings.is_empty() {
                    println!("     警告: {:?}", parsed.warnings);
                }
                if !parsed.errors.is_empty() {
                    println!("     错误: {:?}", parsed.errors);
                }
            }
            Err(e) => println!("     解析失败: {}", e),
        }
        println!();
    }

    // 6. 地理计算演示
    println!("6. 地理计算演示...");
    let beijing = Coordinate::new(39.9042, 116.4074)?;
    let shanghai = Coordinate::new(31.2304, 121.4737)?;
    
    let distance = routekit::geo::haversine_distance_nm(&beijing, &shanghai);
    let bearing = routekit::geo::calculate_bearing(&beijing, &shanghai);
    
    println!("   北京到上海:");
    println!("     距离: {:.2} 海里 ({:.2} 千米)", 
        distance, 
        routekit::utils::nm_to_km(distance));
    println!("     初始航向: {:.2}°", bearing);
    
    let flight_time = routekit::utils::calculate_flight_time_minutes(distance, 450.0);
    println!("     预计飞行时间: {:.0} 分钟 (假设巡航速度450节)", flight_time);
    println!();

    // 7. 航路查询（如果数据充足）
    println!("7. 航路查询...");
    let request = RouteRequest {
        departure_icao: "ZBAA".to_string(),
        destination_icao: "ZSPD".to_string(),
        flight_level: Some(FlightLevel::High),
        route_preference: RoutePreference::Balanced,
        max_routes: 3,
    };

    match kit.find_routes(&request) {
        Ok(routes) => {
            println!("   找到 {} 条航路:", routes.len());
            for (i, route) in routes.iter().enumerate() {
                println!("\n   航路 {}:", i + 1);
                println!("     总距离: {:.2} 海里", route.total_distance_nm);
                println!("     航段数: {}", route.segments.len());
                if let Some(time) = route.estimated_time_minutes {
                    println!("     预计时间: {:.0} 分钟", time);
                }
            }
        }
        Err(e) => {
            println!("   航路查询失败: {}", e);
            println!("   (这可能是因为数据库中没有足够的航路数据)");
        }
    }

    println!("\n=== 示例完成 ===");

    Ok(())
}
