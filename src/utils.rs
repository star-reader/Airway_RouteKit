/// 工具函数模块

/// 标准化ICAO代码（转大写，去除空格）
pub fn normalize_icao(icao: &str) -> String {
    icao.trim().to_uppercase()
}

/// 验证ICAO代码格式
pub fn validate_icao(icao: &str) -> bool {
    let normalized = normalize_icao(icao);
    // 4个字母
    normalized.len() == 4 && normalized.chars().all(|c| c.is_ascii_alphabetic())
}

/// 标准化航点标识符
pub fn normalize_waypoint_id(id: &str) -> String {
    id.trim().to_uppercase()
}

/// 标准化航路标识符
pub fn normalize_airway_id(id: &str) -> String {
    id.trim().to_uppercase()
}

/// 海里转千米
pub fn nm_to_km(nm: f64) -> f64 {
    nm * 1.852
}

/// 千米转海里
pub fn km_to_nm(km: f64) -> f64 {
    km / 1.852
}

/// 英尺转米
pub fn feet_to_meters(feet: i32) -> f64 {
    feet as f64 * 0.3048
}

/// 米转英尺
pub fn meters_to_feet(meters: f64) -> i32 {
    (meters / 0.3048).round() as i32
}

/// 节转千米/小时
pub fn knots_to_kmh(knots: f64) -> f64 {
    knots * 1.852
}

/// 千米/小时转节
pub fn kmh_to_knots(kmh: f64) -> f64 {
    kmh / 1.852
}

/// 计算预计飞行时间（分钟）
/// distance_nm: 距离（海里）
/// speed_knots: 速度（节）
pub fn calculate_flight_time_minutes(distance_nm: f64, speed_knots: f64) -> f64 {
    if speed_knots <= 0.0 {
        return 0.0;
    }
    (distance_nm / speed_knots) * 60.0
}

/// 移除字符串中的多余空格
pub fn normalize_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 分割航路字符串
/// 支持多种分隔符：空格、->、via、DCT等
pub fn split_route_string(route: &str) -> Vec<String> {
    let normalized = route
        .replace("->", " ")
        .replace("→", " ")
        .replace("via", " ")
        .replace("VIA", " ")
        .replace("/", " ")
        .replace(",", " ")
        .replace("DCT", " DCT ")
        .replace("dct", " DCT ");
    
    normalize_spaces(&normalized)
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

/// 检测字符串是否是SID
pub fn is_sid_pattern(s: &str) -> bool {
    let upper = s.to_uppercase();
    upper.ends_with("SID") || upper.contains("DEPARTURE")
}

/// 检测字符串是否是STAR
pub fn is_star_pattern(s: &str) -> bool {
    let upper = s.to_uppercase();
    upper.ends_with("STAR") || upper.contains("ARRIVAL")
}

/// 检测字符串是否是跑道标识符
pub fn is_runway_pattern(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 2 || s.len() > 3 {
        return false;
    }
    
    // 检查是否以数字开头
    let first_chars: String = s.chars().take(2).collect();
    if let Ok(num) = first_chars.parse::<u32>() {
        if (1..=36).contains(&num) {
            // 可选的L/C/R后缀
            if s.len() == 2 {
                return true;
            }
            if s.len() == 3 {
                let last = s.chars().last().unwrap();
                return matches!(last, 'L' | 'C' | 'R' | 'l' | 'c' | 'r');
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_icao() {
        assert_eq!(normalize_icao(" zbaa "), "ZBAA");
        assert_eq!(normalize_icao("zspd"), "ZSPD");
    }

    #[test]
    fn test_validate_icao() {
        assert!(validate_icao("ZBAA"));
        assert!(validate_icao("zspd"));
        assert!(!validate_icao("ZBA"));
        assert!(!validate_icao("ZBAA1"));
        assert!(!validate_icao("12345"));
    }

    #[test]
    fn test_unit_conversions() {
        assert!((nm_to_km(100.0) - 185.2).abs() < 0.1);
        assert!((km_to_nm(185.2) - 100.0).abs() < 0.1);
        assert_eq!(feet_to_meters(1000).round(), 305.0);
    }

    #[test]
    fn test_split_route_string() {
        let route = "ZBAA -> TEPID via G212 DCT VYK STAR ZSPD";
        let parts = split_route_string(route);
        assert!(parts.contains(&"ZBAA".to_string()));
        assert!(parts.contains(&"DCT".to_string()));
        assert!(parts.contains(&"ZSPD".to_string()));
    }

    #[test]
    fn test_pattern_detection() {
        assert!(is_sid_pattern("TEPID6D"));
        assert!(is_star_pattern("VGT1A"));
        assert!(is_runway_pattern("36R"));
        assert!(is_runway_pattern("09"));
        assert!(!is_runway_pattern("37R"));
        assert!(!is_runway_pattern("ABC"));
    }
}
