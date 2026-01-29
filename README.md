# Airway RouteKit - Aviation Route Calculation and Parsing Library

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A high-performance aviation route calculation and parsing library developed in Rust, providing core routing functionality for flight planning systems, flight simulation software, and other aviation applications.

## Features

- **Route Calculation**: Find optimal routes between airports with various preferences
- **Route Parsing**: Parse and validate aviation route strings in multiple formats
- **Geographic Calculations**: Accurate great circle distance and bearing calculations
- **Spatial Search**: Find waypoints and navigation aids within specified areas
- **High Performance**: Optimized for real-time aviation applications
- **Comprehensive Database Support**: Works with aviation navigation databases

## Installation

### Prerequisites

- Rust 1.70 or higher
- Aviation navigation database (compatible SQLite format)

### Build from Source

1. Clone the repository:

```bash
git clone https://github.com/star-reader/Airway_RouteKit.git
cd Airway_RouteKit
```

2. Build the library:

```bash
cargo build --release
```

3. The static and dynamic libraries will be available in the `target/release/` directory.

### Using as a Rust Dependency

Add to your `Cargo.toml`:

```toml
[dependencies]
airway_routekit = { path = "/path/to/Airway_RouteKit" }
```

## Quick Start

### Basic Usage

```rust
use routekit::*;

fn main() -> Result<()> {
    // Create RouteKit instance
    let kit = RouteKit::new("path/to/database.s3db")?;

    // Query routes
    let request = RouteRequest {
        departure_icao: "ZBAA".to_string(),
        destination_icao: "ZSPD".to_string(),
        flight_level: Some(FlightLevel::High),
        route_preference: RoutePreference::Balanced,
        max_routes: 3,
    };

    let routes = kit.find_routes(&request)?;
    for route in routes {
        println!("Total route distance: {} nautical miles", route.total_distance_nm);
    }

    Ok(())
}
```

### Route Parsing

```rust
use routekit::RouteKit;

let kit = RouteKit::new("database.s3db")?;

// Support multiple formats
let parsed = kit.parse_route("ZBAA SID TEPID G212 VYK STAR ZSPD")?;

if parsed.is_valid {
    println!("Departure: {:?}", parsed.departure);
    println!("Destination: {:?}", parsed.destination);
    println!("Route elements: {}", parsed.elements.len());
}
```

### Geographic Calculations

```rust
use routekit::{Coordinate, geo};

let beijing = Coordinate::new(39.9042, 116.4074)?;
let shanghai = Coordinate::new(31.2304, 121.4737)?;

// Calculate great circle distance
let distance = geo::haversine_distance_nm(&beijing, &shanghai);
println!("Distance: {:.2} nautical miles", distance);

// Calculate bearing
let bearing = geo::calculate_bearing(&beijing, &shanghai);
println!("Bearing: {:.2}°", bearing);
```

### Spatial Search

```rust
let coord = Coordinate::new(40.0, 116.0)?;

// Find nearest waypoint
if let Some(waypoint) = kit.find_nearest_waypoint(&coord) {
    println!("Nearest waypoint: {}", waypoint.identifier);
}

// Find all waypoints within radius
let nearby = kit.find_waypoints_within_radius(&coord, 50.0);
println!("Found {} waypoints", nearby.len());
```

## Usage from Other Languages

### C++ Integration

The library provides C-compatible bindings for C++ integration:

```cpp
#include <iostream>
#include "routekit_c.h"

int main() {
    // Initialize RouteKit
    RouteKit* kit = routekit_new("database.s3db");
    if (!kit) {
        std::cerr << "Failed to initialize RouteKit" << std::endl;
        return 1;
    }

    // Create route request
    RouteRequest request = {};
    request.departure_icao = "ZBAA";
    request.destination_icao = "ZSPD";
    request.flight_level = FL_HIGH;
    request.route_preference = ROUTE_BALANCED;
    request.max_routes = 3;

    // Find routes
    RouteResponse* response = routekit_find_routes(kit, &request);
    if (response) {
        for (int i = 0; i < response->route_count; i++) {
            std::cout << "Route " << i << ": " 
                      << response->routes[i].total_distance_nm 
                      << " NM" << std::endl;
        }
        routekit_free_response(response);
    }

    // Cleanup
    routekit_destroy(kit);
    return 0;
}
```

Compile with:

```bash
g++ -o example example.cpp -L./target/release -lroutekit -I./include
```

### Go Integration

Use cgo to integrate with the Rust library:

```go
package main

/*
#cgo LDFLAGS: -L./target/release -lroutekit
#cgo CFLAGS: -I./include
#include "routekit_c.h"
*/
import "C"
import (
    "fmt"
    "unsafe"
)

func main() {
    // Initialize RouteKit
    dbPath := C.CString("database.s3db")
    defer C.free(unsafe.Pointer(dbPath))
  
    kit := C.routekit_new(dbPath)
    if kit == nil {
        fmt.Println("Failed to initialize RouteKit")
        return
    }
    defer C.routekit_destroy(kit)

    // Create route request
    request := C.RouteRequest{
        departure_icao:   C.CString("ZBAA"),
        destination_icao: C.CString("ZSPD"),
        flight_level:     C.FL_HIGH,
        route_preference: C.ROUTE_BALANCED,
        max_routes:       3,
    }
    defer C.free(unsafe.Pointer(request.departure_icao))
    defer C.free(unsafe.Pointer(request.destination_icao))

    // Find routes
    response := C.routekit_find_routes(kit, &request)
    if response != nil {
        defer C.routekit_free_response(response)
      
        routes := (*[1 << 28]C.Route)(unsafe.Pointer(response.routes))[:response.route_count:response.route_count]
        for i, route := range routes {
            fmt.Printf("Route %d: %.2f NM\n", i, route.total_distance_nm)
        }
    }
}
```

Build with:

```bash
go build -o example example.go
```

## API Documentation

### Core Components

- **RouteKit**: Main library interface for route operations
- **RouteRequest**: Configuration for route queries
- **RouteResponse**: Contains calculated route information
- **Coordinate**: Geographic coordinate representation
- **Waypoint**: Navigation waypoint data structure

### Route Preferences

- **Direct**: Shortest distance route
- **Balanced**: Balance between distance and efficiency
- **Economic**: Fuel-efficient routing
- **Time**: Time-optimized routing

### Flight Levels

- **Low**: Below FL245
- **High**: FL245 and above
- **Custom**: Specific flight level

## Database Requirements

This library requires an aviation navigation database to function properly. The database should include:

- Airport information (ICAO/IATA codes, coordinates, runways)
- Waypoints and navigation aids
- Airway structures and connections
- Standard Instrument Departures (SIDs)
- Standard Terminal Arrival Routes (STARs)

**Note**: Ensure you have proper licensing and legal access to aviation navigation databases.

## Performance

- Optimized for real-time route calculations
- Efficient spatial indexing for waypoint searches
- Minimal memory footprint
- Thread-safe operations

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Disclaimer

This library is intended for educational and simulation purposes. **It can't use in real flight!**  For aviation applications, ensure compliance with relevant aviation authorities and regulations.
