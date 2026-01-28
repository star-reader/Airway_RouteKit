/// 集成测试

use routekit::*;

const TEST_DB_PATH: &str = "raw_data/e_dfd_PMDG.s3db";

#[test]
fn test_routekit_initialization() {
    // 如果数据库文件不存在，跳过测试
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        println!("跳过测试：数据库文件不存在");
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH);
    assert!(kit.is_ok(), "RouteKit初始化失败");
}

#[test]
fn test_load_airport() {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        println!("跳过测试：数据库文件不存在");
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).unwrap();
    
    // 测试加载北京首都国际机场
    let result = kit.load_airport("ZBAA");
    if let Ok(airport) = result {
        assert_eq!(airport.identifier, "ZBAA");
        println!("成功加载机场: {} ({:?})", airport.identifier, airport.name);
    }
}

#[test]
fn test_coordinate_validation() {
    // 有效坐标
    assert!(Coordinate::new(40.0, 116.0).is_ok());
    assert!(Coordinate::new(-90.0, -180.0).is_ok());
    assert!(Coordinate::new(90.0, 180.0).is_ok());
    
    // 无效坐标
    assert!(Coordinate::new(91.0, 0.0).is_err());
    assert!(Coordinate::new(-91.0, 0.0).is_err());
    assert!(Coordinate::new(0.0, 181.0).is_err());
    assert!(Coordinate::new(0.0, -181.0).is_err());
}

#[test]
fn test_distance_calculation() {
    use routekit::geo::haversine_distance_nm;
    
    // 北京到上海的距离
    let beijing = Coordinate::new(39.9042, 116.4074).unwrap();
    let shanghai = Coordinate::new(31.2304, 121.4737).unwrap();
    
    let distance = haversine_distance_nm(&beijing, &shanghai);
    
    // 实际距离约为534海里
    assert!(distance > 500.0 && distance < 600.0, "距离计算异常: {}", distance);
    println!("北京到上海距离: {:.2} 海里", distance);
}

#[test]
fn test_config_builder() {
    let config = Config::builder()
        .db_pool_size(15)
        .max_search_depth(1500)
        .spatial_search_radius_nm(100.0)
        .enable_cache(true)
        .build();
    
    assert!(config.is_ok());
    let config = config.unwrap();
    assert_eq!(config.db_pool_size, 15);
    assert_eq!(config.max_search_depth, 1500);
}

#[test]
fn test_config_validation() {
    // 无效配置：连接池大小为0
    let config = Config::builder()
        .db_pool_size(0)
        .build();
    
    assert!(config.is_err());
}

#[test]
fn test_utils_functions() {
    use routekit::utils::*;
    
    // 测试ICAO验证
    assert!(validate_icao("ZBAA"));
    assert!(validate_icao("ZSPD"));
    assert!(!validate_icao("ZBA"));
    assert!(!validate_icao("12345"));
    
    // 测试单位转换
    let nm = 100.0;
    let km = nm_to_km(nm);
    assert!((km - 185.2).abs() < 0.1);
    assert!((km_to_nm(km) - nm).abs() < 0.1);
    
    // 测试航路字符串分割
    let route = "ZBAA SID TEPID G212 VYK STAR ZSPD";
    let parts = split_route_string(route);
    assert!(parts.contains(&"ZBAA".to_string()));
    assert!(parts.contains(&"G212".to_string()));
    assert!(parts.contains(&"ZSPD".to_string()));
}

#[test]
fn test_route_request_creation() {
    use routekit::route::RouteRequestBuilder;
    
    let request = RouteRequestBuilder::new("ZBAA", "ZSPD")
        .flight_level(FlightLevel::High)
        .route_preference(RoutePreference::AirwayPreferred)
        .max_routes(5)
        .build();
    
    assert_eq!(request.departure_icao, "ZBAA");
    assert_eq!(request.destination_icao, "ZSPD");
    assert_eq!(request.max_routes, 5);
}

#[test]
fn test_parse_simple_route() {
    if !std::path::Path::new(TEST_DB_PATH).exists() {
        println!("跳过测试：数据库文件不存在");
        return;
    }

    let kit = RouteKit::new(TEST_DB_PATH).unwrap();
    
    // 测试简单的航路解析
    let route_string = "ZBAA ZSPD";
    let parsed = kit.parse_route(route_string);
    
    if let Ok(result) = parsed {
        println!("解析结果: 有效={}, 元素数={}", result.is_valid, result.elements.len());
        println!("警告: {:?}", result.warnings);
        println!("错误: {:?}", result.errors);
    }
}

#[test]
fn test_bearing_calculation() {
    use routekit::geo::calculate_bearing;
    
    let coord1 = Coordinate::new(40.0, 116.0).unwrap();
    let coord2 = Coordinate::new(31.0, 121.0).unwrap();
    
    let bearing = calculate_bearing(&coord1, &coord2);
    
    // 航向应该在0-360度之间
    assert!(bearing >= 0.0 && bearing < 360.0, "航向超出范围: {}", bearing);
    println!("航向: {:.2}度", bearing);
}

#[test]
fn test_flight_time_calculation() {
    use routekit::utils::calculate_flight_time_minutes;
    
    let distance_nm = 500.0;
    let speed_knots = 450.0;
    
    let time = calculate_flight_time_minutes(distance_nm, speed_knots);
    
    // 500海里 / 450节 * 60分钟 ≈ 66.7分钟
    assert!((time - 66.67).abs() < 1.0, "飞行时间计算异常: {}", time);
    println!("预计飞行时间: {:.2} 分钟", time);
}
