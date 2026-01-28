/**
 * RouteKit FFI 头文件
 * 
 * 供C/Go等语言调用的FFI接口定义
 */

#ifndef ROUTEKIT_H
#define ROUTEKIT_H

#include <stddef.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * 创建RouteKit实例
 * 
 * @param db_path 数据库文件路径（UTF-8编码的C字符串）
 * @return RouteKit实例句柄，失败返回NULL
 */
void* routekit_new(const char* db_path);

/**
 * 销毁RouteKit实例
 * 
 * @param handle RouteKit实例句柄
 */
void routekit_free(void* handle);

/**
 * 查找航路
 * 
 * @param handle RouteKit实例句柄
 * @param departure_icao 起飞机场ICAO代码
 * @param destination_icao 目的机场ICAO代码
 * @param max_routes 最大返回航路数
 * @return JSON格式的航路信息，需要调用routekit_free_string释放
 */
char* routekit_find_routes(
    void* handle,
    const char* departure_icao,
    const char* destination_icao,
    size_t max_routes
);

/**
 * 解析航路字符串
 * 
 * @param handle RouteKit实例句柄
 * @param route_string 航路字符串
 * @return JSON格式的解析结果，需要调用routekit_free_string释放
 */
char* routekit_parse_route(void* handle, const char* route_string);

/**
 * 释放FFI返回的字符串
 * 
 * @param s 需要释放的字符串指针
 */
void routekit_free_string(char* s);

/**
 * 获取最后一次错误信息
 * 
 * @return 错误信息字符串（静态字符串，不需要释放）
 */
const char* routekit_last_error(void);

/**
 * 检查RouteKit实例是否有效
 * 
 * @param handle RouteKit实例句柄
 * @return true表示有效，false表示无效
 */
bool routekit_is_valid(void* handle);

#ifdef __cplusplus
}
#endif

#endif /* ROUTEKIT_H */
