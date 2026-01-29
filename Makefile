.PHONY: all build test clean doc examples bench fmt clippy install

all: build

build:
	@echo "building RouteKit..."
	cargo build --release

release:
	@echo "building RouteKit release..."
	cargo build --release

build-linux:
	@echo "building RouteKit release for linux"
	export LIBSQLITE3_SYS_USE_BUNDLED=1 && \
	export LIBSQLITE3_SYS_USE_PKG_CONFIG=0 && \
	cargo zigbuild --release --target x86_64-unknown-linux-gnu

test:
	@echo "running tests..."
	cargo test

test-verbose:
	@echo "running tests (verbose)..."
	cargo test -- --nocapture

test-integration:
	@echo "running integration tests..."
	cargo test --test integration_test

doc:
	@echo "generating documentation..."
	cargo doc --no-deps --open

examples:
	@echo "running basic example..."
	cargo run --example basic_usage
	@echo "\nrunning parsing example..."
	cargo run --example advanced_parsing

bench:
	@echo "running performance benchmarks..."
	cargo bench

fmt:
	@echo "formatting code..."
	cargo fmt

clippy:
	@echo "checking code..."
	cargo clippy -- -D warnings

clean:
	@echo "cleaning build artifacts..."
	cargo clean

install:
	@echo "installing to system..."
	cargo install --path .

check-all: fmt clippy test
	@echo "all checks passed"

ffi:
	@echo "building FFI library..."
	cargo build --release
	@echo "FFI library generated:"
	@echo "  Linux:   target/release/libroutekit.so"
	@echo "  macOS:   target/release/libroutekit.dylib"
	@echo "  Windows: target/release/routekit.dll"

header:
	@echo "header file located at: routekit.h"

help:
	@echo "RouteKit build system"
	@echo ""
	@echo "Available targets:"
	@echo "  make build           - Build project (debug version)"
	@echo "  make release         - Build release version"
	@echo "  make test            - Run all tests"
	@echo "  make test-verbose    - Run tests (verbose output)"
	@echo "  make test-integration- Run integration tests"
	@echo "  make doc             - Generate and open documentation"
	@echo "  make examples        - Run example programs"
	@echo "  make bench           - Run performance benchmarks"
	@echo "  make fmt             - Format code"
	@echo "  make clippy          - Code quality check"
	@echo "  make clean           - Clean build artifacts"
	@echo "  make check-all       - Run all checks"
	@echo "  make ffi             - Build FFI library"
	@echo "  make help            - Show this help message"
