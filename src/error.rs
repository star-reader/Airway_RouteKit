use thiserror::Error;

/// 错误类型
#[derive(Error, Debug)]
pub enum RouteKitError {
    #[error("数据库错误: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("数据库连接池错误: {0}")]
    PoolError(String),

    #[error("航点未找到: {0}")]
    WaypointNotFound(String),

    #[error("机场未找到: {0}")]
    AirportNotFound(String),

    #[error("航路未找到: 从 {from} 到 {to}")]
    RouteNotFound { from: String, to: String },

    #[error("航路段无效: {0}")]
    InvalidSegment(String),

    #[error("解析错误: {0}")]
    ParseError(String),

    #[error("无效的ICAO代码: {0}")]
    InvalidIcao(String),

    #[error("无效的坐标: lat={lat}, lon={lon}")]
    InvalidCoordinate { lat: f64, lon: f64 },

    #[error("高度限制违反: {0}")]
    AltitudeRestrictionViolation(String),

    #[error("IO错误: {0}")]
    IoError(#[from] std::io::Error),

    #[error("序列化错误: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("空间索引错误: {0}")]
    SpatialIndexError(String),

    #[error("{0}")]
    General(String),
}

/// RouteKit结果类型
pub type Result<T> = std::result::Result<T, RouteKitError>;

impl From<r2d2::Error> for RouteKitError {
    fn from(err: r2d2::Error) -> Self {
        RouteKitError::PoolError(err.to_string())
    }
}
