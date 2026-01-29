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
 * Find routes
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
 * @return Error message string (static string, does not need to be freed)
 */
const char* routekit_last_error(void);

/**
 * Check if RouteKit instance is valid
 * 
 * @param handle RouteKit instance handle
 * @return true if valid, false if invalid
 */
bool routekit_is_valid(void* handle);

#ifdef __cplusplus
}
#endif

#endif /* ROUTEKIT_H */
