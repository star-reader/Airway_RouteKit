# Airway RouteKit

[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A Rust library for procedure-aware enroute routing and navigation-string parsing, with a C ABI for C++, Go, and other languages. It reads a local navigation database and returns routes, segment details, and optional planning diagnostics.

[**Algorithm and implementation reference**](docs/algorithm-reference.md) — detailed architecture, formulas, search logic, validation, results, and limitations.

## Features

- Directed-airway search with altitude constraints, verified SID/STAR boundaries, and route preferences.
- Contextual parsing of airways, direct legs, ambiguous waypoints, and oceanic coordinates.
- Great-circle calculations, spatial waypoint queries, and GeoJSON route previews.
- Optional supplemental paths, regional rules, and route-quality diagnostics.

## Build

Requires a Rust toolchain, a native C compiler, and a compatible SQLite navigation database. Navigation data is not included.

```bash
git clone https://github.com/star-reader/Airway_RouteKit.git
cd Airway_RouteKit
make build
```

Static and shared libraries are generated in `target/release/`.

```bash
make libs-native     # Stage the native shared library and routekit.h in libs/
make libs            # Stage macOS and Linux amd64/arm64 shared libraries
make package         # Create a distribution archive in dist/
```

Linux cross-builds require Zig and the Rust Linux targets. `make setup-cross` installs the pinned cargo-zigbuild tool locally. Use `make help` for available commands.

## Rust Usage

Add the local library to your `Cargo.toml`:

```toml
[dependencies]
routekit = { package = "airway_routekit", path = "../Airway_RouteKit" }
```

Search for routes and parse the returned navigation strings:

```rust
use routekit::{FlightLevel, RouteKit, RoutePreference, RouteRequest};

fn main() -> routekit::Result<()> {
    let kit = RouteKit::new("raw_data/e_navdata.s3db")?;
    let request = RouteRequest {
        departure_icao: "ZBAA".into(),
        destination_icao: "ZSPD".into(),
        flight_level: Some(FlightLevel::Custom(350)),
        route_preference: RoutePreference::Balanced,
        max_routes: 3,
    };

    for route in kit.find_routes(&request)? {
        let text = route.to_route_string();
        println!("{} ({:.1} NM)", text, route.total_distance_nm);
        let parsed = kit.parse_route(&text)?;
        println!("Valid: {}; errors: {:?}", parsed.is_valid, parsed.errors);
    }
    Ok(())
}
```

`Custom(350)` means FL350; `High` and `Low` select airway categories. Preferences are `Balanced`, `AirwayPreferred`, and `DirectPreferred`. `find_routes_simple()` returns only route strings and distances. A successful parse call can still return `is_valid: false`.

For geographic and spatial queries, using the same `kit`:

```rust
let from = routekit::Coordinate::new(40.0, 116.0)?;
let to = routekit::Coordinate::new(31.0, 121.0)?;
let distance_nm = routekit::geo::haversine_distance_nm(&from, &to);
let nearest = kit.find_nearest_waypoint(&from);
let nearby = kit.find_waypoints_within_radius(&from, 50.0);
```

`find_routes_with_options()` accepts supplemental paths and search options. `find_routes_with_context()` also supports regional rules, quality policies, and capability declarations. See the [basic](examples/basic_usage.rs), [parsing](examples/advanced_parsing.rs), and [advanced](examples/advanced_usage.rs) examples.

## C++ and Go Usage

All C ABI functions are declared in [routekit.h](routekit.h). The basic search returns a JSON array of `route_string` and `total_distance_nm`; the additional search interfaces return full routes and diagnostics.

```cpp
#include "routekit.h"
#include <iostream>
#include <memory>

int main() {
    using Kit = std::unique_ptr<void, decltype(&routekit_free)>;
    using Text = std::unique_ptr<char, decltype(&routekit_free_string)>;
    Kit kit(routekit_new("raw_data/e_navdata.s3db"), routekit_free);
    if (!kit) {
        std::cerr << routekit_last_error() << '\n';
        return 1;
    }
    Text json(routekit_find_routes(kit.get(), "ZBAA", "ZSPD", 1),
              routekit_free_string);
    if (!json) {
        std::cerr << routekit_last_error() << '\n';
        return 1;
    }
    std::cout << json.get() << '\n';
}
```

The [Go example](examples/ffi_example.go) provides a cgo wrapper with the same lifecycle:

```go
kit, err := New("raw_data/e_navdata.s3db")
if err != nil {
    return err
}
defer kit.Close()
routes, err := kit.FindRoutes("ZBAA", "ZSPD", 1)
if err != nil {
    return err
}
fmt.Println(routes)
```

Run the complete calling examples from the repository root:

```bash
make example-cpp
make example-go
```

Release returned JSON strings with `routekit_free_string()`. On a NULL result, copy `routekit_last_error()` immediately on the same OS thread; that borrowed error string must not be freed. JSON parameters and defaults for the advanced interfaces are described in the header.

## Notes

- Supply legally obtained compatible navigation data; database connections are read-only.
- Route distance covers enroute segments. SID/STAR boundaries guide the search without expanding procedure trajectories.
- Supplemental paths and regional rules are caller-provided; the library does not fetch live tracks or establish operational authorization.
- Intended for education and simulation, not real-world flight navigation.

API reference: `cargo doc --no-deps`. Issues and pull requests are welcome.

## License

[MIT](LICENSE).
