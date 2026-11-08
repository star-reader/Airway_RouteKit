#ifndef ROUTEKIT_ADVANCED_H
#define ROUTEKIT_ADVANCED_H
#include "routekit.h"
#ifdef __cplusplus
extern "C" {
#endif
char* routekit_find_routes_with_options(
    void* handle, const char* request_json, const char* options_json
);
char* routekit_find_routes_with_context(
    void* handle, const char* request_json, const char* context_json
);
#ifdef __cplusplus
}
#endif
#endif
