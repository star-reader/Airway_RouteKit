/// FFI接口模块 - 供Go等外部语言调用
/// 
/// 提供C ABI兼容的接口

use crate::error::Result;
use crate::models::*;
use crate::RouteKit;
use libc::{c_char, c_void};
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::ptr;

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = RefCell::new(None);
}

fn set_last_error(err: String) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = Some(err);
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
}

/// FFI错误码
#[repr(C)]
pub enum FFIErrorCode {
    Success = 0,
    InvalidParameter = 1,
    DatabaseError = 2,
    NotFound = 3,
    ParseError = 4,
    InternalError = 5,
}

/// C字符串辅助函数
unsafe fn c_str_to_string(c_str: *const c_char) -> Result<String> {
    if c_str.is_null() {
        return Err(crate::error::RouteKitError::General("空指针".to_string()));
    }
    
    let c_str = CStr::from_ptr(c_str);
    Ok(c_str.to_str()
        .map_err(|e| crate::error::RouteKitError::General(format!("UTF-8转换错误: {}", e)))?
        .to_string())
}

/// 创建RouteKit实例
/// 
/// # Safety
/// 
/// db_path 必须是有效的C字符串指针
#[no_mangle]
pub unsafe extern "C" fn routekit_new(db_path: *const c_char) -> *mut c_void {
    clear_last_error();
    
    let db_path_str = match c_str_to_string(db_path) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid db_path parameter: {}", e));
            return ptr::null_mut();
        }
    };

    match RouteKit::new(&db_path_str) {
        Ok(kit) => Box::into_raw(Box::new(kit)) as *mut c_void,
        Err(e) => {
            set_last_error(format!("Failed to create RouteKit: {}", e));
            ptr::null_mut()
        }
    }
}

/// 销毁RouteKit实例
/// 
/// # Safety
/// 
/// handle 必须是通过 routekit_new 创建的有效指针
#[no_mangle]
pub unsafe extern "C" fn routekit_free(handle: *mut c_void) {
    if !handle.is_null() {
        let _ = Box::from_raw(handle as *mut RouteKit);
    }
}

/// 查找航路（简化版本，返回JSON字符串）
/// 
/// # Safety
/// 
/// handle 必须是有效的RouteKit实例
/// departure_icao 和 destination_icao 必须是有效的C字符串
/// 
/// # Returns
/// 
/// 返回JSON格式的航路信息，调用者负责释放返回的字符串
#[no_mangle]
pub unsafe extern "C" fn routekit_find_routes(
    handle: *mut c_void,
    departure_icao: *const c_char,
    destination_icao: *const c_char,
    max_routes: usize,
) -> *mut c_char {
    clear_last_error();
    
    if handle.is_null() {
        set_last_error("Invalid handle: null pointer".to_string());
        return ptr::null_mut();
    }

    let kit = &*(handle as *const RouteKit);

    let dep = match c_str_to_string(departure_icao) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid departure_icao: {}", e));
            return ptr::null_mut();
        }
    };

    let dest = match c_str_to_string(destination_icao) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid destination_icao: {}", e));
            return ptr::null_mut();
        }
    };

    let request = RouteRequest {
        departure_icao: dep,
        destination_icao: dest,
        flight_level: Some(FlightLevel::High),
        route_preference: RoutePreference::Balanced,
        max_routes,
    };

    match kit.find_routes_simple(&request) {
        Ok(routes) => match serde_json::to_string(&routes) {
            Ok(json) => match CString::new(json) {
                Ok(c_str) => c_str.into_raw(),
                Err(e) => {
                    set_last_error(format!("Failed to create C string: {}", e));
                    ptr::null_mut()
                }
            },
            Err(e) => {
                set_last_error(format!("Failed to serialize routes: {}", e));
                ptr::null_mut()
            }
        },
        Err(e) => {
            set_last_error(format!("Route search failed: {}", e));
            ptr::null_mut()
        }
    }
}

/// 解析航路字符串
/// 
/// # Safety
/// 
/// handle 必须是有效的RouteKit实例
/// route_string 必须是有效的C字符串
/// 
/// # Returns
/// 
/// 返回JSON格式的解析结果，调用者负责释放返回的字符串
#[no_mangle]
pub unsafe extern "C" fn routekit_parse_route(
    handle: *mut c_void,
    route_string: *const c_char,
) -> *mut c_char {
    clear_last_error();
    
    if handle.is_null() {
        set_last_error("Invalid handle: null pointer".to_string());
        return ptr::null_mut();
    }

    let kit = &*(handle as *const RouteKit);

    let route_str = match c_str_to_string(route_string) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid route_string: {}", e));
            return ptr::null_mut();
        }
    };

    match kit.parse_route(&route_str) {
        Ok(parsed) => match serde_json::to_string(&parsed) {
            Ok(json) => match CString::new(json) {
                Ok(c_str) => c_str.into_raw(),
                Err(e) => {
                    set_last_error(format!("Failed to create C string: {}", e));
                    ptr::null_mut()
                }
            },
            Err(e) => {
                set_last_error(format!("Failed to serialize parsed route: {}", e));
                ptr::null_mut()
            }
        },
        Err(e) => {
            set_last_error(format!("Route parsing failed: {}", e));
            ptr::null_mut()
        }
    }
}

/// 释放FFI返回的字符串
/// 
/// # Safety
/// 
/// s 必须是通过FFI函数返回的有效字符串指针
#[no_mangle]
pub unsafe extern "C" fn routekit_free_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = CString::from_raw(s);
    }
}

/// 获取最后一次错误信息
/// 
/// # Safety
/// 
/// 返回的字符串指针在下次调用任何FFI函数前有效
/// 调用者应该立即复制字符串内容，不需要手动释放
#[no_mangle]
pub unsafe extern "C" fn routekit_last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        match e.borrow().as_ref() {
            Some(err) => {
                match CString::new(err.as_str()) {
                    Ok(c_str) => {
                        let ptr = c_str.as_ptr();
                        std::mem::forget(c_str);
                        ptr
                    }
                    Err(_) => b"Failed to convert error message\0".as_ptr() as *const c_char,
                }
            }
            None => b"No error\0".as_ptr() as *const c_char,
        }
    })
}

/// 检查RouteKit实例是否有效
#[no_mangle]
pub unsafe extern "C" fn routekit_is_valid(handle: *mut c_void) -> bool {
    !handle.is_null()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_basic() {
        // FFI测试需要实际的数据库文件
        // 这里只测试基本的指针操作
        let null_ptr: *mut c_void = ptr::null_mut();
        assert!(!unsafe { routekit_is_valid(null_ptr) });
    }
}
