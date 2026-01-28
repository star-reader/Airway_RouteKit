/// 高级航路解析示例

use routekit::*;

fn main() -> Result<()> {
    env_logger::init();

    println!("=== 航路解析高级示例 ===\n");

    let kit = RouteKit::new("raw_data/e_dfd_PMDG.s3db")?;

    // 测试各种格式的航路字符串
    let test_cases = vec![
        // 标准格式
        "ZBAA SID TEPID G212 VYK STAR ZSPD",
        
        // 带跑道信息
        "ZBAA/36R TEPID6D TEPID A593 BTO",
        
        // 包含DCT（直飞）
        "ZBAA TEPID DCT VYK ZSPD",
        
        // 使用箭头和via
        "ZBAA -> TEPID via G212 -> ZSPD",
        
        // 中文标点
        "ZBAA，TEPID，G212，VYK，ZSPD",
        
        // 混合格式
        "ZBAA..TEPID6D.TEPID..A593..BTO",
        
        // 带括号和引号
        "\"ZBAA -> ZSPD via A593\"",
        
        // 只有起点和终点
        "ZBAA ZSPD",
        
        // 复杂格式
        "ZBAA ZBAA TEPID G212 VGT DCT YY STAR ZSPD",
    ];

    for (i, route_str) in test_cases.iter().enumerate() {
        println!("测试案例 {}: {}", i + 1, route_str);
        println!("{}", "=".repeat(60));

        match kit.parse_route_flexible(route_str) {
            Ok(parsed) => {
                print_parsed_route(&parsed);
            }
            Err(e) => {
                println!("解析失败: {}\n", e);
            }
        }
    }

    println!("=== 解析测试完成 ===");

    Ok(())
}

fn print_parsed_route(parsed: &ParsedRoute) {
    println!("原始输入: {}", parsed.raw_input);
    println!("解析结果: {}", if parsed.is_valid { "✓ 有效" } else { "✗ 无效" });
    println!();

    if let Some(dep) = &parsed.departure {
        println!("起飞机场: {} ({:?})", dep.identifier, dep.name);
    } else {
        println!("起飞机场: 未识别");
    }

    if let Some(dest) = &parsed.destination {
        println!("目的机场: {} ({:?})", dest.identifier, dest.name);
    } else {
        println!("目的机场: 未识别");
    }

    if let Some(sid) = &parsed.sid {
        println!("SID: {}", sid);
    }

    if let Some(star) = &parsed.star {
        println!("STAR: {}", star);
    }

    println!("\n航路元素 ({} 个):", parsed.elements.len());
    for (i, element) in parsed.elements.iter().enumerate() {
        print!("  {}. ", i + 1);
        match element {
            RouteElement::Waypoint(wp) => {
                println!("航点: {} ({})", wp.identifier, format_coord(&wp.coordinate));
            }
            RouteElement::Airway { identifier, segments } => {
                println!("航路: {} ({} 个航段)", identifier, segments.len());
            }
            RouteElement::Direct { from, to } => {
                println!("直飞: {} -> {}", from.identifier, to.identifier);
            }
            RouteElement::SID(name) => {
                println!("SID: {}", name);
            }
            RouteElement::STAR(name) => {
                println!("STAR: {}", name);
            }
            RouteElement::Unknown(s) => {
                println!("未识别: {}", s);
            }
        }
    }

    if !parsed.warnings.is_empty() {
        println!("\n警告 ({} 个):", parsed.warnings.len());
        for warning in &parsed.warnings {
            println!("  ⚠ {}", warning);
        }
    }

    if !parsed.errors.is_empty() {
        println!("\n错误 ({} 个):", parsed.errors.len());
        for error in &parsed.errors {
            println!("  ✗ {}", error);
        }
    }

    println!("\n");
}

fn format_coord(coord: &Coordinate) -> String {
    format!("{:.4}°N, {:.4}°E", coord.latitude, coord.longitude)
}
