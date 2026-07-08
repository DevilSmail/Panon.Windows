// transparency.rs — 注册表透明效果读写（← TransparencyChecker.cs）
// 阶段 8：EnableTransparency + UseOLEDTaskbarTransparency
//
// 注册表路径：HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced
// - EnableTransparency (DWORD): 系统全局透明效果开关
// - UseOLEDTaskbarTransparency (DWORD): OLED 任务栏透明模式
//
// 启动时快照原始值，退出时恢复，避免修改用户系统配置而不还原。

use std::mem::size_of;

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_WRITE, REG_DWORD, REG_SAM_FLAGS, REG_VALUE_TYPE,
};

const ADVANCED_KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");
const VAL_ENABLE_TRANSPARENCY: PCWSTR = w!("EnableTransparency");
const VAL_USE_OLED: PCWSTR = w!("UseOLEDTaskbarTransparency");

/// 读取注册表 DWORD 值
unsafe fn read_dword(hkey: HKEY, name: PCWSTR) -> Option<u32> {
    let mut value: u32 = 0;
    let mut len: u32 = size_of::<u32>() as u32;
    let mut reg_type: REG_VALUE_TYPE = REG_DWORD;
    let result = RegQueryValueExW(
        hkey,
        name,
        None,
        Some(&mut reg_type),
        Some(&mut value as *mut u32 as *mut u8),
        Some(&mut len),
    );
    if result.is_ok() {
        Some(value)
    } else {
        None
    }
}

/// 写入注册表 DWORD 值
unsafe fn write_dword(hkey: HKEY, name: PCWSTR, value: u32) -> bool {
    let data_bytes: &[u8] = std::slice::from_raw_parts(&value as *const u32 as *const u8, 4);
    let result = RegSetValueExW(hkey, name, 0, REG_DWORD, Some(data_bytes));
    result.is_ok()
}

/// 打开 Advanced 注册表键
unsafe fn open_key(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, ADVANCED_KEY, 0, access, &mut hkey).is_ok() {
        Some(hkey)
    } else {
        None
    }
}

/// 读取 EnableTransparency 当前值
pub fn read_enable_transparency() -> Option<u32> {
    unsafe {
        let hkey = open_key(KEY_READ)?;
        let val = read_dword(hkey, VAL_ENABLE_TRANSPARENCY);
        let _ = RegCloseKey(hkey);
        val
    }
}

/// 读取 UseOLEDTaskbarTransparency 当前值
pub fn read_use_oled() -> Option<u32> {
    unsafe {
        let hkey = open_key(KEY_READ)?;
        let val = read_dword(hkey, VAL_USE_OLED);
        let _ = RegCloseKey(hkey);
        val
    }
}

/// 写入 EnableTransparency
pub fn write_enable_transparency(value: u32) -> bool {
    unsafe {
        let hkey = match open_key(KEY_WRITE) {
            Some(h) => h,
            None => return false,
        };
        let ok = write_dword(hkey, VAL_ENABLE_TRANSPARENCY, value);
        let _ = RegCloseKey(hkey);
        ok
    }
}

/// 写入 UseOLEDTaskbarTransparency
pub fn write_use_oled(value: u32) -> bool {
    unsafe {
        let hkey = match open_key(KEY_WRITE) {
            Some(h) => h,
            None => return false,
        };
        let ok = write_dword(hkey, VAL_USE_OLED, value);
        let _ = RegCloseKey(hkey);
        ok
    }
}

/// 透明效果管理器：启动时快照原始值，退出时恢复
pub struct TransparencyManager {
    original_enable: Option<u32>,
    original_oled: Option<u32>,
}

impl TransparencyManager {
    /// 创建管理器并快照当前注册表值
    pub fn new() -> Self {
        Self {
            original_enable: read_enable_transparency(),
            original_oled: read_use_oled(),
        }
    }

    /// 应用透明效果设置到注册表
    pub fn apply(&self, enable_transparency: bool, use_oled: bool) {
        let e = if enable_transparency { 1 } else { 0 };
        let o = if use_oled { 1 } else { 0 };
        if !write_enable_transparency(e) {
            eprintln!("[transparency] failed to write EnableTransparency");
        }
        if !write_use_oled(o) {
            eprintln!("[transparency] failed to write UseOLEDTaskbarTransparency");
        }
    }

    /// 恢复启动时的原始注册表值
    pub fn restore(&self) {
        if let Some(v) = self.original_enable {
            if !write_enable_transparency(v) {
                eprintln!("[transparency] failed to restore EnableTransparency");
            }
        }
        if let Some(v) = self.original_oled {
            if !write_use_oled(v) {
                eprintln!("[transparency] failed to restore UseOLEDTaskbarTransparency");
            }
        }
    }
}

impl Default for TransparencyManager {
    fn default() -> Self {
        Self::new()
    }
}
