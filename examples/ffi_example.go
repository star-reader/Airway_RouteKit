package main

/*
#cgo CFLAGS: -I${SRCDIR}/..
#cgo LDFLAGS: -lroutekit
#include "routekit.h"
#include <stdlib.h>
*/
import "C"
import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"runtime"
	"unsafe"
)

type RouteResult struct {
	RouteString     string  `json:"route_string"`
	TotalDistanceNM float64 `json:"total_distance_nm"`
}

type ParsedRoute struct {
	RawInput string            `json:"raw_input"`
	IsValid  bool              `json:"is_valid"`
	Elements []json.RawMessage `json:"elements"`
	Warnings []string          `json:"warnings"`
	Errors   []string          `json:"errors"`
}

type RouteKit struct{ handle unsafe.Pointer }

func lastError() error { return errors.New(C.GoString(C.routekit_last_error())) }

func New(database string) (*RouteKit, error) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	path := C.CString(database)
	defer C.free(unsafe.Pointer(path))
	handle := C.routekit_new(path)
	if handle == nil {
		return nil, lastError()
	}
	return &RouteKit{handle: handle}, nil
}

func (kit *RouteKit) Close() {
	if kit.handle != nil {
		C.routekit_free(kit.handle)
		kit.handle = nil
	}
}

func (kit *RouteKit) FindRoutes(departure, destination string, count int) ([]RouteResult, error) {
	if kit.handle == nil || count < 0 {
		return nil, errors.New("invalid handle or route count")
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	dep, dest := C.CString(departure), C.CString(destination)
	defer C.free(unsafe.Pointer(dep))
	defer C.free(unsafe.Pointer(dest))
	output := C.routekit_find_routes(kit.handle, dep, dest, C.size_t(count))
	if output == nil {
		return nil, lastError()
	}
	defer C.routekit_free_string(output)
	var routes []RouteResult
	if err := json.Unmarshal([]byte(C.GoString(output)), &routes); err != nil {
		return nil, err
	}
	return routes, nil
}

func (kit *RouteKit) ParseRoute(input string) (*ParsedRoute, error) {
	if kit.handle == nil {
		return nil, errors.New("closed handle")
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	text := C.CString(input)
	defer C.free(unsafe.Pointer(text))
	output := C.routekit_parse_route(kit.handle, text)
	if output == nil {
		return nil, lastError()
	}
	defer C.routekit_free_string(output)
	var parsed ParsedRoute
	if err := json.Unmarshal([]byte(C.GoString(output)), &parsed); err != nil {
		return nil, err
	}
	return &parsed, nil
}

func run() error {
	database, departure, destination := "raw_data/e_dfd_PMDG.s3db", "ZBAA", "ZSPD"
	if len(os.Args) > 1 {
		database = os.Args[1]
	}
	if len(os.Args) > 2 {
		departure = os.Args[2]
	}
	if len(os.Args) > 3 {
		destination = os.Args[3]
	}
	kit, err := New(database)
	if err != nil {
		return err
	}
	defer kit.Close()
	routes, err := kit.FindRoutes(departure, destination, 1)
	if err != nil {
		return err
	}
	for _, route := range routes {
		fmt.Printf("%s (%.1f NM)\n", route.RouteString, route.TotalDistanceNM)
		parsed, err := kit.ParseRoute(route.RouteString)
		if err != nil {
			return err
		}
		fmt.Printf("valid=%v, warnings=%v, errors=%v\n", parsed.IsValid, parsed.Warnings, parsed.Errors)
	}
	return nil
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
