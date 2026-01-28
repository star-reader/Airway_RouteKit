# RouteKit 架构设计文档

## 概述

RouteKit是一个高性能的航空航路计算和解析库，采用模块化设计，充分利用Rust的类型系统和性能优势。

## 系统架构

```
┌─────────────────────────────────────────────────────────────┐
│                         应用层                               │
│  (Rust应用 / Go应用 / 其他通过FFI调用的应用)                  │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      RouteKit API                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ 航路查询接口  │  │ 航路解析接口  │  │  FFI接口     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      核心业务层                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ RouteSearcher│  │ RouteParser  │  │ 配置管理      │      │
│  │  (A*算法)    │  │ (智能解析)    │  │  (Config)    │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      支持服务层                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ SpatialIndex │  │ GeoCalculator│  │ Utils        │      │
│  │  (R-tree)    │  │ (地理计算)    │  │ (工具函数)    │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      数据访问层                               │
│  ┌──────────────┐  ┌──────────────┐                         │
│  │ DatabasePool │  │ 数据模型      │                         │
│  │ (连接池管理)  │  │  (Models)    │                         │
│  └──────────────┘  └──────────────┘                         │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    SQLite数据库                               │
│              (PMDG导航数据 - e_dfd_PMDG.s3db)                │
└─────────────────────────────────────────────────────────────┘
```

## 模块详解

### 1. 数据模型层 (models.rs)

定义了系统中的核心数据结构：

- **Coordinate**: 地理坐标，包含纬度和经度
- **Airport**: 机场信息
- **Waypoint**: 航点信息
- **AirwaySegment**: 航路段
- **Sid/Star**: 标准离场/进场程序
- **Route**: 完整航路
- **ParsedRoute**: 解析后的航路

所有模型都实现了`Serialize`和`Deserialize` trait，便于JSON序列化。

### 2. 数据库层 (database.rs)

**职责**：
- 管理SQLite数据库连接池
- 提供数据查询接口
- 加载机场、航点、航路等数据

**关键特性**：
- 使用r2d2连接池管理，支持并发访问
- 参数化查询，防止SQL注入
- 惰性加载，按需查询数据

**主要接口**：
```rust
pub fn load_airport(&self, icao: &str) -> Result<Airport>
pub fn load_all_waypoints(&self) -> Result<Vec<Waypoint>>
pub fn find_waypoint(&self, identifier: &str) -> Result<Option<Waypoint>>
pub fn find_airway_segments(&self, route_id: &str) -> Result<Vec<AirwaySegment>>
pub fn find_sids(&self, airport_icao: &str) -> Result<Vec<Sid>>
pub fn find_stars(&self, airport_icao: &str) -> Result<Vec<Star>>
```

### 3. 空间索引层 (spatial.rs)

**职责**：
- 构建R-tree空间索引
- 提供高效的地理位置查询

**关键特性**：
- 使用rstar库实现R-tree
- 批量加载优化
- 支持最近邻、半径范围、K近邻查询

**算法复杂度**：
- 构建: O(n log n)
- 查询: O(log n)
- 空间: O(n)

### 4. 地理计算模块 (geo.rs)

**职责**：
- 提供精确的地理计算函数

**核心算法**：
- **Haversine公式**: 计算大圆距离
- **方位角计算**: 计算两点间的航向
- **目标点计算**: 根据起点、距离和航向计算目标点

**精度**：
- 距离误差: < 0.5%
- 航向误差: < 0.1°

### 5. 航路查询模块 (route.rs)

**职责**：
- 实现航路搜索算法
- 匹配SID/STAR程序

**核心算法 - A\*搜索**：

```
function A*(start, goal):
    openSet = {start}
    cameFrom = {}
    
    gScore[start] = 0
    fScore[start] = h(start, goal)
    
    while openSet is not empty:
        current = node in openSet with lowest fScore
        
        if current == goal:
            return reconstruct_path(cameFrom, current)
        
        openSet.remove(current)
        closedSet.add(current)
        
        for neighbor in neighbors(current):
            tentative_gScore = gScore[current] + distance(current, neighbor)
            
            if tentative_gScore < gScore[neighbor]:
                cameFrom[neighbor] = current
                gScore[neighbor] = tentative_gScore
                fScore[neighbor] = gScore[neighbor] + h(neighbor, goal)
                openSet.add(neighbor)
    
    return failure
```

**启发式函数**：
- h(n) = 直线距离(n, goal) × 距离权重

**代价函数**：
- g(n) = 实际距离 × (1 ± 航路偏好权重)

### 6. 航路解析模块 (parser.rs)

**职责**：
- 解析各种格式的航路字符串
- 容错处理

**解析流程**：

```
输入字符串
    ↓
预处理（标准化、移除噪声）
    ↓
分词（按分隔符分割）
    ↓
识别元素（机场/航点/航路/程序）
    ↓
数据库查询验证
    ↓
构建结构化结果
    ↓
输出ParsedRoute
```

**支持的格式**：
- 标准格式: `ZBAA SID TEPID G212 VYK STAR ZSPD`
- 箭头格式: `ZBAA -> ZSPD via G212`
- 点分格式: `ZBAA..TEPID..G212..VYK..ZSPD`
- 混合格式: 自动识别分隔符

### 7. FFI接口层 (ffi.rs)

**职责**：
- 提供C ABI接口
- 支持Go等语言调用

**内存管理**：
- Rust侧分配，外部侧释放
- 使用Box管理生命周期
- 提供专门的释放函数

**接口设计**：
```c
void* routekit_new(const char* db_path);
void routekit_free(void* handle);
char* routekit_find_routes(...);
void routekit_free_string(char* s);
```

## 线程安全

### 并发策略

1. **读写锁**：
   - SpatialIndex使用`RwLock`，支持多读单写
   - 适合读多写少的场景

2. **连接池**：
   - DatabasePool内部使用r2d2管理
   - 自动处理并发连接

3. **无锁数据结构**：
   - 只读数据（如配置）可以安全共享
   - 使用Arc进行引用计数

### 线程安全保证

- 所有公共API都是`Send + Sync`
- 内部状态通过Arc和RwLock保护
- 数据库访问通过连接池序列化

## 性能优化

### 1. 空间局部性

- 使用R-tree减少搜索空间
- 缓存热点数据

### 2. 算法优化

- A*算法使用二叉堆优化
- 提前终止搜索（达到目标或超时）

### 3. 内存优化

- 避免不必要的克隆
- 使用Arc共享不可变数据
- 连接池重用连接

### 4. 编译优化

```toml
[profile.release]
opt-level = 3        # 最高优化级别
lto = true           # 链接时优化
codegen-units = 1    # 单编译单元（更好的优化）
```

## 错误处理

### 错误类型层次

```
RouteKitError
├── DatabaseError
├── WaypointNotFound
├── AirportNotFound
├── RouteNotFound
├── ParseError
├── InvalidCoordinate
└── ...
```

### 错误处理策略

1. **使用Result类型**：所有可能失败的操作都返回`Result<T>`
2. **错误传播**：使用`?`运算符传播错误
3. **详细错误信息**：包含上下文信息，便于调试
4. **用户友好**：错误信息面向最终用户

## 扩展性设计

### 1. 插件化

- 解析器可扩展（自定义规则）
- 算法可配置（权重调整）

### 2. 数据源抽象

- DatabasePool可替换为其他数据源
- 支持多数据库同时使用

### 3. 版本兼容

- API设计考虑向后兼容
- 使用Option和Default处理新增字段

## 测试策略

### 1. 单元测试

- 每个模块都有独立的测试
- 覆盖关键算法和边界情况

### 2. 集成测试

- 测试模块间的交互
- 使用真实数据库进行测试

### 3. 性能测试

- 使用Criterion进行基准测试
- 持续监控性能回归

### 4. 属性测试

- 使用proptest验证不变量
- 随机输入测试鲁棒性

## 部署建议

### 1. 库使用

```bash
# 编译发布版本
cargo build --release

# 库文件位于
target/release/libroutekit.so    # Linux
target/release/libroutekit.dylib # macOS
target/release/routekit.dll      # Windows
```

### 2. Docker部署

```dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/libroutekit.so /usr/local/lib/
COPY --from=builder /app/raw_data /data
```

### 3. 性能调优

- 根据实际负载调整连接池大小
- 调整空间索引搜索半径
- 配置合适的搜索深度限制

## 未来规划

1. **缓存层**：
   - LRU缓存常用航路
   - Redis支持分布式缓存

2. **更多算法**：
   - Dijkstra算法
   - 遗传算法优化

3. **实时更新**：
   - 支持NOTAM实时更新
   - 临时限制区处理

4. **可视化**：
   - 生成航路地图
   - 3D可视化

5. **机器学习**：
   - 航路推荐
   - 异常检测
