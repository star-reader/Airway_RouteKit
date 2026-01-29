// Go FFI调用示例
//
// 使用方法:
// 1. 先编译Rust库: cargo build --release
// 2. 编译并运行: go run examples/ffi_example.go

package main

// #cgo LDFLAGS: -L./target/release -lroutekit
// #include "routekit.h"
// #include <stdlib.h>
import "C"
import (
	"encoding/json"
	"fmt"
	"unsafe"
)

// Route 航路结构
type Route struct {
	Departure struct {
		IcaoCode   string `json:"icao_code"`
		Identifier string `json:"identifier"`
		Coordinate struct {
			Latitude  float64 `json:"latitude"`
			Longitude float64 `json:"longitude"`
		} `json:"coordinate"`
	} `json:"departure"`
	Destination struct {
		IcaoCode   string `json:"icao_code"`
		Identifier string `json:"identifier"`
		Coordinate struct {
			Latitude  float64 `json:"latitude"`
			Longitude float64 `json:"longitude"`
		} `json:"coordinate"`
	} `json:"destination"`
	Segments         []interface{} `json:"segments"`
	TotalDistanceNm  float64       `json:"total_distance_nm"`
	EstimatedTimeMin *float64      `json:"estimated_time_minutes"`
}

// ParsedRoute 解析后的航路
type ParsedRoute struct {
	RawInput    string        `json:"raw_input"`
	IsValid     bool          `json:"is_valid"`
	Elements    []interface{} `json:"elements"`
	Warnings    []string      `json:"warnings"`
	Errors      []string      `json:"errors"`
	Departure   interface{}   `json:"departure"`
	Destination interface{}   `json:"destination"`
}

// RouteKit Go包装
type RouteKit struct {
	handle unsafe.Pointer
}

// New 创建RouteKit实例
func New(dbPath string) (*RouteKit, error) {
	cPath := C.CString(dbPath)
	defer C.free(unsafe.Pointer(cPath))

	handle := C.routekit_new(cPath)
	if handle == nil {
		return nil, fmt.Errorf("failed to create RouteKit instance")
	}

	return &RouteKit{handle: handle}, nil
}

// Close 关闭RouteKit实例
func (rk *RouteKit) Close() {
	if rk.handle != nil {
		C.routekit_free(rk.handle)
		rk.handle = nil
	}
}

// FindRoutes 查找航路
func (rk *RouteKit) FindRoutes(departure, destination string, maxRoutes int) ([]Route, error) {
	if rk.handle == nil {
		return nil, fmt.Errorf("RouteKit instance is closed")
	}

	cDeparture := C.CString(departure)
	defer C.free(unsafe.Pointer(cDeparture))

	cDestination := C.CString(destination)
	defer C.free(unsafe.Pointer(cDestination))

	cJson := C.routekit_find_routes(rk.handle, cDeparture, cDestination, C.size_t(maxRoutes))
	if cJson == nil {
		return nil, fmt.Errorf("failed to find routes")
	}
	defer C.routekit_free_string(cJson)

	jsonStr := C.GoString(cJson)

	var routes []Route
	if err := json.Unmarshal([]byte(jsonStr), &routes); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %v", err)
	}

	return routes, nil
}

// ParseRoute 解析航路字符串
func (rk *RouteKit) ParseRoute(routeString string) (*ParsedRoute, error) {
	if rk.handle == nil {
		return nil, fmt.Errorf("RouteKit instance is closed")
	}

	cRouteString := C.CString(routeString)
	defer C.free(unsafe.Pointer(cRouteString))

	cJson := C.routekit_parse_route(rk.handle, cRouteString)
	if cJson == nil {
		return nil, fmt.Errorf("failed to parse route")
	}
	defer C.routekit_free_string(cJson)

	jsonStr := C.GoString(cJson)

	var parsed ParsedRoute
	if err := json.Unmarshal([]byte(jsonStr), &parsed); err != nil {
		return nil, fmt.Errorf("failed to parse JSON: %v", err)
	}

	return &parsed, nil
}

func main() {
	fmt.Println("=== RouteKit Go FFI 示例 ===\n")

	// 创建RouteKit实例
	kit, err := New("raw_data/e_dfd_PMDG.s3db")
	if err != nil {
		fmt.Printf("错误: %v\n", err)
		return
	}
	defer kit.Close()

	fmt.Println("✓ RouteKit实例创建成功\n")

	// 解析航路字符串
	fmt.Println("1. 解析航路字符串...")
	routeStr := "ZBAA SID TEPID G212 VYK STAR ZSPD"
	parsed, err := kit.ParseRoute(routeStr)
	if err != nil {
		fmt.Printf("解析失败: %v\n", err)
	} else {
		fmt.Printf("   输入: %s\n", parsed.RawInput)
		fmt.Printf("   有效: %v\n", parsed.IsValid)
		fmt.Printf("   元素数: %d\n", len(parsed.Elements))
		if len(parsed.Warnings) > 0 {
			fmt.Printf("   警告: %v\n", parsed.Warnings)
		}
		if len(parsed.Errors) > 0 {
			fmt.Printf("   错误: %v\n", parsed.Errors)
		}
	}
	fmt.Println()

	// 查找航路
	fmt.Println("2. 查找航路...")
	routes, err := kit.FindRoutes("ZBAA", "ZSPD", 3)
	if err != nil {
		fmt.Printf("查找失败: %v\n", err)
	} else {
		fmt.Printf("   找到 %d 条航路\n", len(routes))
		for i, route := range routes {
			fmt.Printf("\n   航路 %d:\n", i+1)
			fmt.Printf("     起飞: %s\n", route.Departure.Identifier)
			fmt.Printf("     目的: %s\n", route.Destination.Identifier)
			fmt.Printf("     总距离: %.2f 海里\n", route.TotalDistanceNm)
			if route.EstimatedTimeMin != nil {
				fmt.Printf("     预计时间: %.0f 分钟\n", *route.EstimatedTimeMin)
			}
		}
	}

	fmt.Println("\n=== 示例完成 ===")
}
