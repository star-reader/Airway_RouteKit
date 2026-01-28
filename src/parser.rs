use crate::database::DatabasePool;
use crate::error::{Result, RouteKitError};
use crate::models::*;
use crate::utils::*;
use lazy_static::lazy_static;
use regex::Regex;
use std::sync::Arc;

lazy_static! {
    /// ICAO代码模式
    static ref ICAO_PATTERN: Regex = Regex::new(r"^[A-Z]{4}$").unwrap();
    
    /// 航路模式（如：A593, G212, J146）
    static ref AIRWAY_PATTERN: Regex = Regex::new(r"^[A-Z]\d+$").unwrap();
    
    /// SID模式（通常以数字结尾，包含字母）
    static ref SID_PATTERN: Regex = Regex::new(r"^[A-Z0-9]+\d[A-Z]?$").unwrap();
    
    /// STAR模式
    static ref STAR_PATTERN: Regex = Regex::new(r"^[A-Z0-9]+\d[A-Z]?$").unwrap();
    
    /// 跑道模式
    static ref RUNWAY_PATTERN: Regex = Regex::new(r"^([0-2]\d|3[0-6])[LCR]?$").unwrap();
}

/// 航路解析器
pub struct RouteParser {
    db_pool: Arc<DatabasePool>,
}

impl RouteParser {
    pub fn new(db_pool: Arc<DatabasePool>) -> Self {
        Self { db_pool }
    }

    /// 解析航路字符串
    pub fn parse(&self, route_string: &str) -> Result<ParsedRoute> {
        let mut parsed = ParsedRoute {
            raw_input: route_string.to_string(),
            departure: None,
            destination: None,
            sid: None,
            star: None,
            elements: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
            is_valid: false,
        };

        // 预处理：标准化输入
        let normalized = self.normalize_input(route_string);
        let tokens = split_route_string(&normalized);

        if tokens.is_empty() {
            parsed.errors.push("输入为空".to_string());
            return Ok(parsed);
        }

        // 解析各个部分
        self.parse_tokens(&tokens, &mut parsed)?;

        // 验证解析结果
        parsed.is_valid = parsed.errors.is_empty() 
            && parsed.departure.is_some() 
            && parsed.destination.is_some();

        Ok(parsed)
    }

    /// 标准化输入
    fn normalize_input(&self, input: &str) -> String {
        let mut normalized = input.to_uppercase();

        // 移除常见的无关字符
        normalized = normalized.replace(['(', ')', '"', '\'', '[', ']', '{', '}'], " ");
        
        // 标准化分隔符
        normalized = normalized.replace("->", " ");
        normalized = normalized.replace("→", " ");
        normalized = normalized.replace("..", " ");
        
        // 处理中文标点
        normalized = normalized.replace("，", " ");
        normalized = normalized.replace("。", " ");
        
        normalize_spaces(&normalized)
    }

    /// 解析令牌序列
    fn parse_tokens(&self, tokens: &[String], parsed: &mut ParsedRoute) -> Result<()> {
        let mut i = 0;
        let mut last_waypoint: Option<Waypoint> = None;
        let mut expecting_destination = false;

        while i < tokens.len() {
            let token = &tokens[i];
            let token_upper = token.to_uppercase();

            // 跳过无意义的词
            if matches!(
                token_upper.as_str(),
                "VIA" | "TO" | "FROM" | "AT" | "FL" | "THE"
            ) {
                i += 1;
                continue;
            }

            // 解析起飞机场（第一个ICAO代码）
            if parsed.departure.is_none() && self.is_airport_code(&token_upper) {
                match self.db_pool.load_airport(&token_upper) {
                    Ok(airport) => {
                        parsed.departure = Some(airport);
                    }
                    Err(_) => {
                        parsed.warnings.push(format!("机场 {} 未在数据库中找到", token_upper));
                    }
                }
                i += 1;
                continue;
            }

            // 处理跑道标识符
            if is_runway_pattern(&token_upper) {
                // 跑道信息通常跟在机场后面
                parsed.warnings.push(format!("忽略跑道标识符: {}", token_upper));
                i += 1;
                continue;
            }

            // 处理SID
            if token_upper.contains("SID") || (parsed.departure.is_some() && parsed.sid.is_none()) {
                if let Some(sid_name) = self.extract_procedure_name(&token_upper, "SID") {
                    parsed.sid = Some(sid_name.clone());
                    parsed.elements.push(RouteElement::SID(sid_name));
                }
                i += 1;
                continue;
            }

            // 处理STAR
            if token_upper.contains("STAR") {
                if let Some(star_name) = self.extract_procedure_name(&token_upper, "STAR") {
                    parsed.star = Some(star_name.clone());
                    parsed.elements.push(RouteElement::STAR(star_name));
                    expecting_destination = true;
                }
                i += 1;
                continue;
            }

            // 处理DCT（直飞）
            if token_upper == "DCT" {
                if let Some(from_wp) = last_waypoint.clone() {
                    // 下一个应该是航点
                    if i + 1 < tokens.len() {
                        i += 1;
                        if let Some(to_wp) = self.try_find_waypoint(&tokens[i])? {
                            parsed.elements.push(RouteElement::Direct {
                                from: from_wp.clone(),
                                to: to_wp.clone(),
                            });
                            last_waypoint = Some(to_wp);
                        }
                    }
                }
                i += 1;
                continue;
            }

            // 处理航路
            if AIRWAY_PATTERN.is_match(&token_upper) {
                if let Ok(segments) = self.db_pool.find_airway_segments(&token_upper) {
                    if !segments.is_empty() {
                        parsed.elements.push(RouteElement::Airway {
                            identifier: token_upper.clone(),
                            segments: segments.clone(),
                        });
                        if let Some(last_seg) = segments.last() {
                            last_waypoint = Some(last_seg.waypoint.clone());
                        }
                    } else {
                        parsed.warnings.push(format!("航路 {} 未找到航段", token_upper));
                    }
                } else {
                    parsed.warnings.push(format!("航路 {} 未在数据库中找到", token_upper));
                }
                i += 1;
                continue;
            }

            // 处理航点
            if let Some(waypoint) = self.try_find_waypoint(&token_upper)? {
                parsed.elements.push(RouteElement::Waypoint(waypoint.clone()));
                last_waypoint = Some(waypoint);
                i += 1;
                continue;
            }

            // 最后尝试作为目的机场
            if self.is_airport_code(&token_upper) && (expecting_destination || i == tokens.len() - 1) {
                match self.db_pool.load_airport(&token_upper) {
                    Ok(airport) => {
                        parsed.destination = Some(airport);
                    }
                    Err(_) => {
                        parsed.warnings.push(format!("目的机场 {} 未找到", token_upper));
                    }
                }
                i += 1;
                continue;
            }

            // 无法识别的元素
            parsed.elements.push(RouteElement::Unknown(token_upper.clone()));
            parsed.warnings.push(format!("无法识别的元素: {}", token_upper));
            i += 1;
        }

        // 如果没有明确的目的机场，尝试从最后的ICAO代码推断
        if parsed.destination.is_none() && !tokens.is_empty() {
            let last_token = &tokens[tokens.len() - 1];
            if self.is_airport_code(last_token) {
                if let Ok(airport) = self.db_pool.load_airport(last_token) {
                    parsed.destination = Some(airport);
                }
            }
        }

        Ok(())
    }

    /// 判断是否是机场代码
    fn is_airport_code(&self, token: &str) -> bool {
        validate_icao(token)
    }

    /// 尝试查找航点
    fn try_find_waypoint(&self, identifier: &str) -> Result<Option<Waypoint>> {
        self.db_pool.find_waypoint(identifier)
    }

    /// 提取程序名称
    fn extract_procedure_name(&self, token: &str, proc_type: &str) -> Option<String> {
        // 移除 "SID" 或 "STAR" 后缀
        let name = token.replace(proc_type, "").trim().to_string();
        if !name.is_empty() {
            Some(name)
        } else {
            Some(token.to_string())
        }
    }

    /// 解析自由格式的航路字符串（高容错性）
    pub fn parse_flexible(&self, route_string: &str) -> Result<ParsedRoute> {
        // 实现更宽松的解析逻辑
        let mut parsed = self.parse(route_string)?;

        // 如果标准解析失败，尝试更多启发式方法
        if !parsed.is_valid {
            self.apply_heuristics(&mut parsed)?;
        }

        Ok(parsed)
    }

    /// 应用启发式规则
    fn apply_heuristics(&self, parsed: &mut ParsedRoute) -> Result<()> {
        // 1. 如果只有两个ICAO代码，假设是起点和终点
        let icao_codes: Vec<_> = parsed
            .elements
            .iter()
            .filter_map(|e| match e {
                RouteElement::Waypoint(wp) if validate_icao(&wp.identifier) => {
                    Some(wp.identifier.clone())
                }
                RouteElement::Unknown(s) if validate_icao(s) => Some(s.clone()),
                _ => None,
            })
            .collect();

        if icao_codes.len() >= 2 && parsed.departure.is_none() {
            if let Ok(dep) = self.db_pool.load_airport(&icao_codes[0]) {
                parsed.departure = Some(dep);
            }
        }

        if icao_codes.len() >= 2 && parsed.destination.is_none() {
            if let Ok(dest) = self.db_pool.load_airport(&icao_codes[icao_codes.len() - 1]) {
                parsed.destination = Some(dest);
            }
        }

        // 2. 尝试识别未知元素
        let mut new_elements = Vec::new();
        for element in &parsed.elements {
            if let RouteElement::Unknown(s) = element {
                // 尝试各种可能性
                if let Ok(Some(wp)) = self.try_find_waypoint(s) {
                    new_elements.push(RouteElement::Waypoint(wp));
                    continue;
                }
                new_elements.push(element.clone());
            } else {
                new_elements.push(element.clone());
            }
        }
        parsed.elements = new_elements;

        // 重新验证
        parsed.is_valid = parsed.errors.is_empty()
            && parsed.departure.is_some()
            && parsed.destination.is_some();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_input() {
        let input = "ZBAA -> TEPID via G212 DCT VYK STAR ZSPD";
        let parser = RouteParser::new(Arc::new(
            DatabasePool::new("test.db", 1).unwrap()
        ));
        let normalized = parser.normalize_input(input);
        assert!(normalized.contains("ZBAA"));
        assert!(normalized.contains("ZSPD"));
    }

    #[test]
    fn test_airway_pattern() {
        assert!(AIRWAY_PATTERN.is_match("G212"));
        assert!(AIRWAY_PATTERN.is_match("A593"));
        assert!(AIRWAY_PATTERN.is_match("J146"));
        assert!(!AIRWAY_PATTERN.is_match("ZBAA"));
        assert!(!AIRWAY_PATTERN.is_match("123"));
    }

    #[test]
    fn test_runway_pattern() {
        assert!(RUNWAY_PATTERN.is_match("36R"));
        assert!(RUNWAY_PATTERN.is_match("09"));
        assert!(RUNWAY_PATTERN.is_match("18L"));
        assert!(!RUNWAY_PATTERN.is_match("37R"));
        assert!(!RUNWAY_PATTERN.is_match("ABC"));
    }
}
