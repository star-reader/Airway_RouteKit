use crate::models::Coordinate;

/// 地球半径（海里）
pub const EARTH_RADIUS_NM: f64 = 3440.065;

/// 地球半径（千米）
pub const EARTH_RADIUS_KM: f64 = 6371.0;

/// 度转弧度
pub fn deg_to_rad(deg: f64) -> f64 {
    deg * std::f64::consts::PI / 180.0
}

/// 弧度转度
pub fn rad_to_deg(rad: f64) -> f64 {
    rad * 180.0 / std::f64::consts::PI
}

/// 计算两点间的大圆距离（海里）
/// 使用Haversine公式
pub fn haversine_distance_nm(coord1: &Coordinate, coord2: &Coordinate) -> f64 {
    let lat1 = deg_to_rad(coord1.latitude);
    let lat2 = deg_to_rad(coord2.latitude);
    let delta_lat = deg_to_rad(coord2.latitude - coord1.latitude);
    let delta_lon = deg_to_rad(coord2.longitude - coord1.longitude);

    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_NM * c
}

/// 计算两点间的大圆距离（千米）
pub fn haversine_distance_km(coord1: &Coordinate, coord2: &Coordinate) -> f64 {
    let lat1 = deg_to_rad(coord1.latitude);
    let lat2 = deg_to_rad(coord2.latitude);
    let delta_lat = deg_to_rad(coord2.latitude - coord1.latitude);
    let delta_lon = deg_to_rad(coord2.longitude - coord1.longitude);

    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_KM * c
}

/// 计算从coord1到coord2的初始航向（度，0-360）
/// 返回真航向
pub fn calculate_bearing(coord1: &Coordinate, coord2: &Coordinate) -> f64 {
    let lat1 = deg_to_rad(coord1.latitude);
    let lat2 = deg_to_rad(coord2.latitude);
    let delta_lon = deg_to_rad(coord2.longitude - coord1.longitude);

    let y = delta_lon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * delta_lon.cos();

    let bearing = rad_to_deg(y.atan2(x));

    // 标准化到0-360度
    (bearing + 360.0) % 360.0
}

/// 计算中点坐标
pub fn calculate_midpoint(coord1: &Coordinate, coord2: &Coordinate) -> crate::error::Result<Coordinate> {
    let lat1 = deg_to_rad(coord1.latitude);
    let lat2 = deg_to_rad(coord2.latitude);
    let lon1 = deg_to_rad(coord1.longitude);
    let delta_lon = deg_to_rad(coord2.longitude - coord1.longitude);

    let bx = lat2.cos() * delta_lon.cos();
    let by = lat2.cos() * delta_lon.sin();

    let lat3 = (lat1.sin() + lat2.sin()).atan2(
        ((lat1.cos() + bx).powi(2) + by.powi(2)).sqrt()
    );
    let lon3 = lon1 + by.atan2(lat1.cos() + bx);

    Coordinate::new(rad_to_deg(lat3), rad_to_deg(lon3))
}

/// 根据起点、距离和航向计算目标点
/// distance_nm: 距离（海里）
/// bearing: 航向（度）
pub fn calculate_destination(
    start: &Coordinate,
    distance_nm: f64,
    bearing: f64,
) -> crate::error::Result<Coordinate> {
    let lat1 = deg_to_rad(start.latitude);
    let lon1 = deg_to_rad(start.longitude);
    let brng = deg_to_rad(bearing);
    let angular_distance = distance_nm / EARTH_RADIUS_NM;

    let lat2 = (lat1.sin() * angular_distance.cos()
        + lat1.cos() * angular_distance.sin() * brng.cos())
    .asin();

    let lon2 = lon1
        + (brng.sin() * angular_distance.sin() * lat1.cos())
            .atan2(angular_distance.cos() - lat1.sin() * lat2.sin());

    Coordinate::new(rad_to_deg(lat2), rad_to_deg(lon2))
}

/// 检查点是否在边界框内
pub fn is_in_bounding_box(
    point: &Coordinate,
    min_lat: f64,
    max_lat: f64,
    min_lon: f64,
    max_lon: f64,
) -> bool {
    point.latitude >= min_lat
        && point.latitude <= max_lat
        && point.longitude >= min_lon
        && point.longitude <= max_lon
}

/// 计算边界框
pub struct BoundingBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

impl BoundingBox {
    /// 根据中心点和半径创建边界框
    pub fn from_center_radius(center: &Coordinate, radius_nm: f64) -> Self {
        let delta_lat = rad_to_deg(radius_nm / EARTH_RADIUS_NM);
        let delta_lon = rad_to_deg(
            radius_nm / (EARTH_RADIUS_NM * deg_to_rad(center.latitude).cos())
        );

        Self {
            min_lat: (center.latitude - delta_lat).max(-90.0),
            max_lat: (center.latitude + delta_lat).min(90.0),
            min_lon: (center.longitude - delta_lon).max(-180.0),
            max_lon: (center.longitude + delta_lon).min(180.0),
        }
    }

    /// 检查点是否在边界框内
    pub fn contains(&self, point: &Coordinate) -> bool {
        is_in_bounding_box(point, self.min_lat, self.max_lat, self.min_lon, self.max_lon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haversine_distance() {
        // 北京首都国际机场 (ZBAA) 到 上海浦东国际机场 (ZSPD)
        let zbaa = Coordinate::new(40.0801, 116.5846).unwrap();
        let zspd = Coordinate::new(31.1434, 121.8052).unwrap();
        
        let distance = haversine_distance_nm(&zbaa, &zspd);
        // 实际距离约为 534 海里
        assert!((distance - 534.0).abs() < 10.0);
    }

    #[test]
    fn test_bearing() {
        let coord1 = Coordinate::new(40.0, 116.0).unwrap();
        let coord2 = Coordinate::new(31.0, 121.0).unwrap();
        
        let bearing = calculate_bearing(&coord1, &coord2);
        assert!(bearing >= 0.0 && bearing < 360.0);
    }

    #[test]
    fn test_midpoint() {
        let coord1 = Coordinate::new(40.0, 116.0).unwrap();
        let coord2 = Coordinate::new(30.0, 120.0).unwrap();
        
        let midpoint = calculate_midpoint(&coord1, &coord2).unwrap();
        assert!((midpoint.latitude - 35.0).abs() < 1.0);
        assert!((midpoint.longitude - 118.0).abs() < 1.0);
    }

    #[test]
    fn test_bounding_box() {
        let center = Coordinate::new(40.0, 116.0).unwrap();
        let bbox = BoundingBox::from_center_radius(&center, 100.0);
        
        assert!(bbox.contains(&center));
        
        let far_point = Coordinate::new(50.0, 130.0).unwrap();
        assert!(!bbox.contains(&far_point));
    }
}
