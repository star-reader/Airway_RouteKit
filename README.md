# RouteKit - 航空航路计算与解析库

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

一个用Rust开发的高性能航空航路计算和解析库，为飞行计划系统、模拟飞行软件和其他航空应用提供核心路由功能。

## 安装

添加到 `Cargo.toml`:

```toml
[dependencies]
airway_routekit = "0.1"
```

或使用cargo命令：

```bash
cargo add airway_routekit
```

## 快速开始

### 基本使用

```rust
use routekit::*;

fn main() -> Result<()> {
    // 创建RouteKit实例
    let kit = RouteKit::new("path/to/database.s3db")?;

    // 查询航路
    let request = RouteRequest {
        departure_icao: "ZBAA".to_string(),
        destination_icao: "ZSPD".to_string(),
        flight_level: Some(FlightLevel::High),
        route_preference: RoutePreference::Balanced,
        max_routes: 3,
    };

    let routes = kit.find_routes(&request)?;
    for route in routes {
        println!("航路总距离: {} 海里", route.total_distance_nm);
    }

    Ok(())
}
```

### 航路解析

```rust
use routekit::RouteKit;

let kit = RouteKit::new("database.s3db")?;

// 支持多种格式
let parsed = kit.parse_route("ZBAA SID TEPID G212 VYK STAR ZSPD")?;

if parsed.is_valid {
    println!("起飞: {:?}", parsed.departure);
    println!("目的: {:?}", parsed.destination);
    println!("航路元素: {}", parsed.elements.len());
}
```

### 地理计算

```rust
use routekit::{Coordinate, geo};

let beijing = Coordinate::new(39.9042, 116.4074)?;
let shanghai = Coordinate::new(31.2304, 121.4737)?;

// 计算大圆距离
let distance = geo::haversine_distance_nm(&beijing, &shanghai);
println!("距离: {:.2} 海里", distance);

// 计算航向
let bearing = geo::calculate_bearing(&beijing, &shanghai);
println!("航向: {:.2}°", bearing);
```

### 空间搜索

```rust
let coord = Coordinate::new(40.0, 116.0)?;

// 查找最近的航点
if let Some(waypoint) = kit.find_nearest_waypoint(&coord) {
    println!("最近航点: {}", waypoint.identifier);
}

// 查找半径内的所有航点
let nearby = kit.find_waypoints_within_radius(&coord, 50.0);
println!("找到 {} 个航点", nearby.len());
```

---

**注意**: 本库需要配合航空导航数据库使用，请确保你有合法的数据库访问权限。
