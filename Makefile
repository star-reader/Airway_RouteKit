.PHONY: all build test clean doc examples bench fmt clippy install

all: build

# 构建项目
build:
	@echo "正在构建RouteKit..."
	cargo build

# 构建发布版本
release:
	@echo "正在构建RouteKit发布版本..."
	cargo build --release

# 运行测试
test:
	@echo "运行测试..."
	cargo test

# 运行测试（详细输出）
test-verbose:
	@echo "运行测试（详细输出）..."
	cargo test -- --nocapture

# 运行集成测试
test-integration:
	@echo "运行集成测试..."
	cargo test --test integration_test

# 生成文档
doc:
	@echo "生成文档..."
	cargo doc --no-deps --open

# 运行示例
examples:
	@echo "运行基本示例..."
	cargo run --example basic_usage
	@echo "\n运行解析示例..."
	cargo run --example advanced_parsing

# 运行性能基准测试
bench:
	@echo "运行性能基准测试..."
	cargo bench

# 代码格式化
fmt:
	@echo "格式化代码..."
	cargo fmt

# 代码检查
clippy:
	@echo "代码检查..."
	cargo clippy -- -D warnings

# 清理构建产物
clean:
	@echo "清理构建产物..."
	cargo clean

# 安装到系统
install:
	@echo "安装到系统..."
	cargo install --path .

# 检查所有
check-all: fmt clippy test
	@echo "所有检查通过！"

# 构建FFI库
ffi:
	@echo "构建FFI库..."
	cargo build --release
	@echo "FFI库已生成："
	@echo "  Linux:   target/release/libroutekit.so"
	@echo "  macOS:   target/release/libroutekit.dylib"
	@echo "  Windows: target/release/routekit.dll"

# 生成头文件（已手动创建）
header:
	@echo "头文件位于: routekit.h"

# 帮助信息
help:
	@echo "RouteKit 构建系统"
	@echo ""
	@echo "可用目标："
	@echo "  make build           - 构建项目（调试版本）"
	@echo "  make release         - 构建发布版本"
	@echo "  make test            - 运行所有测试"
	@echo "  make test-verbose    - 运行测试（详细输出）"
	@echo "  make test-integration- 运行集成测试"
	@echo "  make doc             - 生成并打开文档"
	@echo "  make examples        - 运行示例程序"
	@echo "  make bench           - 运行性能基准测试"
	@echo "  make fmt             - 格式化代码"
	@echo "  make clippy          - 代码质量检查"
	@echo "  make clean           - 清理构建产物"
	@echo "  make check-all       - 运行所有检查"
	@echo "  make ffi             - 构建FFI库"
	@echo "  make help            - 显示此帮助信息"
