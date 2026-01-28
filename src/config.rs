use serde::{Deserialize, Serialize};

/// RouteKit配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub db_pool_size: u32,
    pub max_search_depth: usize,
    pub search_timeout_ms: u64,

    /// 空间索引最近邻搜索半径（海里）
    pub spatial_search_radius_nm: f64,
    pub enable_cache: bool,
    pub cache_size: usize,
    pub verbose_logging: bool,
    pub search_weights: SearchWeights,
}

/// 航路搜索权重配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchWeights {
    /// 距离权重
    pub distance_weight: f64,
    /// 航路优先权重（相对于直飞）
    pub airway_preference_weight: f64,
    /// 高度适配权重
    pub altitude_match_weight: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_pool_size: 10,
            max_search_depth: 1000,
            search_timeout_ms: 5000,
            spatial_search_radius_nm: 50.0,
            enable_cache: true,
            cache_size: 1000,
            verbose_logging: false,
            search_weights: SearchWeights::default(),
        }
    }
}

impl Default for SearchWeights {
    fn default() -> Self {
        Self {
            distance_weight: 1.0,
            airway_preference_weight: 0.8,
            altitude_match_weight: 0.5,
        }
    }
}

impl Config {
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::RouteKitError;

        if self.db_pool_size == 0 {
            return Err(RouteKitError::ConfigError(
                "数据库连接池大小必须大于0".to_string(),
            ));
        }

        if self.max_search_depth == 0 {
            return Err(RouteKitError::ConfigError(
                "搜索深度必须大于0".to_string(),
            ));
        }

        if self.spatial_search_radius_nm <= 0.0 {
            return Err(RouteKitError::ConfigError(
                "空间搜索半径必须大于0".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    pub fn db_pool_size(mut self, size: u32) -> Self {
        self.config.db_pool_size = size;
        self
    }

    pub fn max_search_depth(mut self, depth: usize) -> Self {
        self.config.max_search_depth = depth;
        self
    }

    pub fn search_timeout_ms(mut self, timeout: u64) -> Self {
        self.config.search_timeout_ms = timeout;
        self
    }

    pub fn spatial_search_radius_nm(mut self, radius: f64) -> Self {
        self.config.spatial_search_radius_nm = radius;
        self
    }

    pub fn enable_cache(mut self, enable: bool) -> Self {
        self.config.enable_cache = enable;
        self
    }

    pub fn cache_size(mut self, size: usize) -> Self {
        self.config.cache_size = size;
        self
    }

    pub fn verbose_logging(mut self, verbose: bool) -> Self {
        self.config.verbose_logging = verbose;
        self
    }

    pub fn search_weights(mut self, weights: SearchWeights) -> Self {
        self.config.search_weights = weights;
        self
    }

    pub fn build(self) -> crate::error::Result<Config> {
        self.config.validate()?;
        Ok(self.config)
    }
}
