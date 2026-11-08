#include "routekit.h"
#include <iostream>
#include <memory>
#include <stdexcept>

using Kit = std::unique_ptr<void, decltype(&routekit_free)>;
using Text = std::unique_ptr<char, decltype(&routekit_free_string)>;

void print_result(char* result) {
    Text owned(result, routekit_free_string);
    if (!owned) throw std::runtime_error(routekit_last_error());
    std::cout << owned.get() << '\n';
}

int main(int argc, char** argv) {
    try {
        const char* database = argc > 1 ? argv[1] : "raw_data/e_navdata.s3db";
        Kit kit(routekit_new(database), routekit_free);
        if (!kit) throw std::runtime_error(routekit_last_error());
        print_result(routekit_find_routes(kit.get(), "ZBAA", "ZSPD", 1));
        print_result(routekit_parse_route(kit.get(),
            "ZBAA ELKUR W40 YQG W142 DALIM A593 VMB W161 SASAN ZSPD"));
        const char* request = R"({"departure_icao":"ZBAA","destination_icao":"ZSPD","flight_level":"High","route_preference":"Balanced","max_routes":1})";
        print_result(routekit_find_routes_with_options(kit.get(), request, "{}"));
        print_result(routekit_find_routes_with_context(kit.get(), request, "{}"));
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
