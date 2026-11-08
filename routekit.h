#ifndef ROUTEKIT_H
#define ROUTEKIT_H

#include <stddef.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Create RouteKit instance
 * 
 * @param db_path Database file path (UTF-8 encoded C string)
 * @return RouteKit instance handle, returns NULL on failure
 */
void* routekit_new(const char* db_path);

/**
 * Destroy RouteKit instance
 * 
 * @param handle RouteKit instance handle
 */
void routekit_free(void* handle);

/**
 * Find simplified routes using High category, Balanced preference, and default
 * SearchOptions/RoutingPolicy. Uses the same engine as the detailed interfaces.
 * Each JSON array entry contains route_string and total_distance_nm only.
 * max_routes is an upper bound; zero returns []. For flight-level/preference
 * selection, full segments, or diagnostics, use the detailed search functions.
 *
 * @param handle RouteKit instance handle
 * @param departure_icao Departure airport ICAO code
 * @param destination_icao Destination airport ICAO code
 * @param max_routes Maximum number of routes to return
 * @return JSON string of route information, needs to be freed with routekit_free_string
 */
char* routekit_find_routes(
    void* handle,
    const char* departure_icao,
    const char* destination_icao,
    size_t max_routes
);

/**
 * Find detailed routes using request_json fields departure_icao, destination_icao,
 * flight_level ("High", "Low", {"Custom":350}, or null), route_preference, and
 * max_routes. options_json accepts detour_budget, oceanic_paths, allow_simulation,
 * planning_time_unix, and cruise_speed_knots; "{}" defaults to a 1.5x great-circle
 * distance + 200 NM budget, no supplemental paths, simulation disabled, no planning
 * time, and 450 kt. Example options: {"cruise_speed_knots":480,"detour_budget":null}.
 * Returns JSON with full routes and matching diagnostics, or NULL on failure;
 * read routekit_last_error() for errors and release results with routekit_free_string().
 */
char* routekit_find_routes_with_options(
    void* handle,
    const char* request_json,
    const char* options_json
);

/**
 * Find routes using the same request_json as routekit_find_routes_with_options().
 * context_json accepts options (SearchOptions), policy (routing weights and
 * thresholds), regions ({"regions":[...]}), and capabilities (string array).
 * "{}" uses default search options, policy values regression_weight=2.0,
 * equivalent_ratio=0.002, equivalent_min_nm=3.0, equivalent_max_nm=20.0,
 * detour_warning_ratio=1.15, and empty regions/capabilities. Example context:
 * {"policy":{"regression_weight":3.0}}. Returns JSON with result (routes and
 * diagnostics), per-route quality, and skipped_connections, or NULL on failure;
 * read routekit_last_error() for errors and release results with routekit_free_string().
 */
char* routekit_find_routes_with_context(
    void* handle,
    const char* request_json,
    const char* context_json
);

/**
 * Parse route string
 * 
 * @param handle RouteKit instance handle
 * @param route_string Route string
 * @return JSON string of parsing result, needs to be freed with routekit_free_string
 */
char* routekit_parse_route(void* handle, const char* route_string);

/**
 * Free FFI returned string
 * 
 * @param s String pointer to free
 */
void routekit_free_string(char* s);

/**
 * Get last error message
 * 
 * @return Borrowed thread-local message, or "No error". Do not free it.
 *         Copy it immediately on the OS thread where the failed call ran.
 *         A later initialization/search/parse call clears or replaces it.
 */
const char* routekit_last_error(void);

/**
 * Check if RouteKit instance is valid
 * 
 * @param handle RouteKit instance handle
 * @return true if handle is non-NULL, false otherwise. This check does not
 *         detect dangling pointers or validate ownership. Do not use a handle
 *         after routekit_free, and never free the same handle twice.
 */
bool routekit_is_valid(void* handle);

#ifdef __cplusplus
}
#endif

#endif /* ROUTEKIT_H */
