SHELL := /bin/sh
.DEFAULT_GOAL := all

CARGO ?= cargo
RUSTC ?= rustc
CARGO_FLAGS ?=
TARGET_DIR ?= $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target)
CARGO_ZIGBUILD ?= $(if $(wildcard $(TARGET_DIR)/build-tools/bin/cargo-zigbuild),$(TARGET_DIR)/build-tools/bin/cargo-zigbuild,cargo-zigbuild)
ZIGBUILD_VERSION := 0.23.2
LIBS_DIR ?= libs
DIST_DIR ?= dist
PREFIX ?= /usr/local
DESTDIR ?=
NAVDATA ?= raw_data/e_navdata.s3db
DEPARTURE ?= ZBAA
DESTINATION ?= ZSPD
GO ?= go
NATIVE_LIB_DIR = $(LIBS_DIR)/$(dir $(shell sh scripts/library-path.sh "$(NATIVE_TARGET)"))
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)
HOST_TARGET := $(shell $(RUSTC) -vV | sed -n 's/^host: //p')
NATIVE_TARGET := $(if $(CARGO_BUILD_TARGET),$(CARGO_BUILD_TARGET),$(HOST_TARGET))
NATIVE_RELEASE_DIR = $(TARGET_DIR)/$(if $(CARGO_BUILD_TARGET),$(CARGO_BUILD_TARGET)/)release
LINUX_SONAME_ARG := -C link-arg=-Wl,-soname,libroutekit.so
NATIVE_LINK_ARGS = $(if $(findstring -unknown-linux-gnu,$(NATIVE_TARGET)),-- $(LINUX_SONAME_ARG))
LINUX_TARGET_AMD64 := x86_64-unknown-linux-gnu
LINUX_TARGET_ARM64 := aarch64-unknown-linux-gnu
LINUX_GLIBC ?= 2.28
LINUX_ABI = $(if $(LINUX_GLIBC),.$(LINUX_GLIBC))
LINUX_BUILD_ENV = LIBSQLITE3_SYS_USE_PKG_CONFIG=0 RUSTFLAGS="$(RUSTFLAGS) -C link-arg=-Wl,-soname,libroutekit.so"

.PHONY: all build release ffi setup-cross check-cross build-linux build-linux-amd64 build-linux-arm64 \
        libs libs-native libs-linux headers header package package-native \
        test test-verbose check-all \
        doc examples example-cpp example-go fmt fmt-check clippy clean install help

all: build

build:
	$(CARGO) rustc --locked --release --lib --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS) $(NATIVE_LINK_ARGS)

release ffi: build

build-linux: build-linux-amd64

setup-cross:
	$(CARGO) install --locked --root "$(TARGET_DIR)/build-tools" cargo-zigbuild --version "$(ZIGBUILD_VERSION)" $(CARGO_FLAGS)

check-cross:
	sh scripts/check-cross.sh "$(CARGO_ZIGBUILD)"

build-linux-amd64: check-cross
	$(LINUX_BUILD_ENV) "$(CARGO_ZIGBUILD)" zigbuild --locked --release --lib --target "$(LINUX_TARGET_AMD64)$(LINUX_ABI)" --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS)

build-linux-arm64: check-cross
	$(LINUX_BUILD_ENV) "$(CARGO_ZIGBUILD)" zigbuild --locked --release --lib --target "$(LINUX_TARGET_ARM64)$(LINUX_ABI)" --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS)

headers:
	mkdir -p "$(LIBS_DIR)"
	cp routekit.h "$(LIBS_DIR)/"

header:
	@printf '%s\n' 'Public header: routekit.h (all FFI interfaces)'

libs-native: build headers
	sh scripts/stage-library.sh "$(NATIVE_TARGET)" "$(NATIVE_RELEASE_DIR)" "$(LIBS_DIR)"

libs-linux: build-linux-amd64 build-linux-arm64 headers
	sh scripts/stage-library.sh "$(LINUX_TARGET_AMD64)" "$(TARGET_DIR)/$(LINUX_TARGET_AMD64)/release" "$(LIBS_DIR)"
	sh scripts/stage-library.sh "$(LINUX_TARGET_ARM64)" "$(TARGET_DIR)/$(LINUX_TARGET_ARM64)/release" "$(LIBS_DIR)"

# Stage the native macOS library alongside both Linux targets.
libs: build libs-linux
	@case "$(NATIVE_TARGET)" in \
	  *-apple-darwin) sh scripts/stage-library.sh "$(NATIVE_TARGET)" "$(NATIVE_RELEASE_DIR)" "$(LIBS_DIR)" ;; \
	  *-unknown-linux-gnu) printf '%s\n' 'Linux libraries staged; build libs-native on macOS for the macOS library.' ;; \
	  *) printf '%s\n' 'Unsupported native platform for make libs.' >&2; exit 1 ;; \
	esac

package: libs
	sh scripts/package.sh "$(LIBS_DIR)" "$(DIST_DIR)" "$(VERSION)" all

package-native: libs-native
	sh scripts/package.sh "$(LIBS_DIR)" "$(DIST_DIR)" "$(VERSION)" "$(NATIVE_TARGET)"

test:
	$(CARGO) test --locked --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS)

test-verbose:
	$(CARGO) test --locked --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS) -- --nocapture

doc:
	$(CARGO) doc --locked --no-deps --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS)

examples:
	$(CARGO) run --locked --release --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS) --example basic_usage -- "$(NAVDATA)" "$(DEPARTURE)" "$(DESTINATION)"
	$(CARGO) run --locked --release --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS) --example advanced_parsing -- "$(NAVDATA)"
	$(CARGO) run --locked --release --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS) --example advanced_usage -- "$(NAVDATA)" "$(DEPARTURE)" "$(DESTINATION)"

example-cpp: libs-native
	mkdir -p "$(TARGET_DIR)/examples"
	$(CXX) -std=c++17 -Wall -Wextra -Werror -I"$(LIBS_DIR)" examples/ffi_example.cpp -L"$(abspath $(NATIVE_LIB_DIR))" -lroutekit "-Wl,-rpath,$(abspath $(NATIVE_LIB_DIR))" -o "$(TARGET_DIR)/examples/ffi_example"
	"$(TARGET_DIR)/examples/ffi_example" "$(NAVDATA)"

example-go: libs-native
	CGO_ENABLED=1 CGO_LDFLAGS="-L$(abspath $(NATIVE_LIB_DIR)) -Wl,-rpath,$(abspath $(NATIVE_LIB_DIR))" $(GO) run examples/ffi_example.go "$(NAVDATA)" "$(DEPARTURE)" "$(DESTINATION)"

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --locked --all-targets --target-dir "$(TARGET_DIR)" $(CARGO_FLAGS) -- -D warnings

check-all: fmt-check clippy test

clean:
	$(CARGO) clean --target-dir "$(TARGET_DIR)"

# Install actual library files; this crate has no cargo-installable executable.
install: libs-native
	sh scripts/install.sh "$(NATIVE_TARGET)" "$(LIBS_DIR)" "$(DESTDIR)$(PREFIX)"

help:
	@printf '%s\n' \
	  'RouteKit build system' \
	  '  make build / release / ffi  Build native release libraries' \
	  '  make build-linux           Build Linux amd64 (alias)' \
	  '  make setup-cross           Install pinned cross-build tool under target/build-tools' \
	  '  make build-linux-amd64     Build Linux x86_64 using cargo-zigbuild' \
	  '  make build-linux-arm64     Build Linux aarch64 using cargo-zigbuild' \
	  '  make libs                  Stage native macOS and both Linux libraries' \
	  '  make libs-native           Stage only the current target library and header' \
	  '  make libs-linux            Stage Linux amd64 and arm64 libraries' \
	  '  make package               Archive macOS + Linux bundle (macOS library required)' \
	  '  make package-native        Archive only the current target' \
	  '  make test / test-verbose   Run ordinary tests' \
	  '  make examples              Run Rust calling examples' \
	  '  make example-cpp / example-go  Build and run native FFI examples' \
	  '  make fmt / clippy / check-all / doc' \
	  '  make install PREFIX=...    Install native shared library and header' \
	  '  make clean                 Clean Cargo artifacts' \
	  'Options: CARGO_FLAGS=--offline NAVDATA=... DEPARTURE=ZBAA DESTINATION=ZSPD LIBS_DIR=libs DIST_DIR=dist' \
	  'Linux tools: Zig, cargo-zigbuild, installed Rust Linux targets; glibc default: 2.28'
