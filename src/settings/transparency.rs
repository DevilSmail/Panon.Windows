// transparency.rs — 注册表透明效果读写（← TransparencyChecker.cs）
// 阶段 8：EnableTransparency + UseOLEDTaskbarTransparency
// P2 修复：添加 transparency_original.json 持久化，对齐 C# 行为
//
// 注册表路径：HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\Advanced
// - EnableTransparency (DWORD): 系统全局透明效果开关
// - UseOLEDTaskbarTransparency (DWORD): OLED 任务栏透明模式
//
// 行为对齐 C# TransparencyChecker:
// - 首次启动：读注册表 → 保存到 transparency_original.json（持久化真正原始值）
// - 后续启动：从 transparency_original.json 加载（不受之前运行修改影响）
// - apply(): 立即写注册表
// - 退出时：不恢复（保持用户设置，与 C# 一致）
// - restore(): 仅在卸载时调用，从持久化快照恢复真正原始值

use std::fs;
use std::mem::size_of;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_WRITE, REG_DWORD, REG_SAM_FLAGS, REG_VALUE_TYPE,
};

const PERSONALIZE_KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const EXPLORER_ADVANCED_KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced");
const VAL_ENABLE_TRANSPARENCY: PCWSTR = w!("EnableTransparency");
const VAL_USE_OLED: PCWSTR = w!("UseOLEDTaskbarTransparency");

/// transparency_original.json 数据结构（对齐 C# OriginalSnapshot）
#[derive(Serialize, Deserialize)]
struct OriginalSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_transparency: Option<u32>,
    oled_key_existed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_oled_taskbar_transparency: Option<u32>,
}

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

/// 删除注册表值（卸载恢复时使用）
#[allow(dead_code)]
unsafe fn delete_value(hkey: HKEY, name: PCWSTR) -> bool {
    use windows::Win32::System::Registry::RegDeleteValueW;
    RegDeleteValueW(hkey, name).is_ok()
}

/// 打开 Themes\Personalize 键
unsafe fn open_personalize_key(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE_KEY, 0, access, &mut hkey).is_ok() {
        Some(hkey)
    } else {
        None
    }
}

/// 打开 Explorer\Advanced 键
unsafe fn open_advanced_key(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, EXPLORER_ADVANCED_KEY, 0, access, &mut hkey).is_ok() {
        Some(hkey)
    } else {
        None
    }
}

/// 读取 EnableTransparency 当前值（Themes\Personalize）
fn read_enable_transparency() -> Option<u32> {
    unsafe {
        let hkey = open_personalize_key(KEY_READ)?;
        let val = read_dword(hkey, VAL_ENABLE_TRANSPARENCY);
        let _ = RegCloseKey(hkey);
        val
    }
}

/// 读取 UseOLEDTaskbarTransparency 当前值（Explorer\Advanced）
fn read_use_oled() -> Option<u32> {
    unsafe {
        let hkey = open_advanced_key(KEY_READ)?;
        let val = read_dword(hkey, VAL_USE_OLED);
        let _ = RegCloseKey(hkey);
        val
    }
}

/// 写入 EnableTransparency（Themes\Personalize）
fn write_enable_transparency(value: u32) -> bool {
    unsafe {
        let hkey = match open_personalize_key(KEY_WRITE) {
            Some(h) => h,
            None => return false,
        };
        let ok = write_dword(hkey, VAL_ENABLE_TRANSPARENCY, value);
        let _ = RegCloseKey(hkey);
        ok
    }
}

/// 写入 UseOLEDTaskbarTransparency（Explorer\Advanced）
fn write_use_oled(value: u32) -> bool {
    unsafe {
        let hkey = match open_advanced_key(KEY_WRITE) {
            Some(h) => h,
            None => return false,
        };
        let ok = write_dword(hkey, VAL_USE_OLED, value);
        let _ = RegCloseKey(hkey);
        ok
    }
}

/// 删除 UseOLEDTaskbarTransparency（卸载恢复时使用）
#[allow(dead_code)]
unsafe fn delete_use_oled() -> bool {
    let hkey = match open_advanced_key(KEY_WRITE) {
        Some(h) => h,
        None => return false,
    };
    let ok = delete_value(hkey, VAL_USE_OLED);
    let _ = RegCloseKey(hkey);
    ok
}

/// 透明效果管理器（对齐 C# TransparencyChecker）
pub struct TransparencyManager {
    original_enable: Option<u32>,
    original_oled_existed: bool,
    original_oled: Option<u32>,
    captured: bool,
}

impl TransparencyManager {
    /// 获取快照文件路径: %APPDATA%/Panon/transparency_original.json
    fn snapshot_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("Panon").join("transparency_original.json"))
    }

    /// 创建管理器并捕获原始注册表状态
    pub fn new() -> Self {
        let mut tm = Self {
            original_enable: None,
            original_oled_existed: false,
            original_oled: None,
            captured: false,
        };
        tm.capture_original_state();
        tm
    }

    /// 捕获原始注册表状态（首次启动持久化到文件，后续从文件加载）
    fn capture_original_state(&mut self) {
        if self.captured {
            return;
        }

        // 先尝试从持久化文件加载（程序被多次启动也不会丢失真正原始值）
        if self.try_load_snapshot() {
            self.captured = true;
            return;
        }

        // 首次启动：从注册表读取并持久化
        self.original_enable = read_enable_transparency();
        self.original_oled = read_use_oled();
        self.original_oled_existed = self.original_oled.is_some();

        self.save_snapshot();
        self.captured = true;
        println!("[transparency] original state captured (EnableTransparency={:?}, UseOLED={:?})",
            self.original_enable, self.original_oled);
    }

    /// 持久化原始状态到 transparency_original.json
    fn save_snapshot(&self) {
        let path = match Self::snapshot_path() {
            Some(p) => p,
            None => {
                eprintln!("[transparency] cannot resolve snapshot path");
                return;
            }
        };

        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let data = OriginalSnapshot {
            enable_transparency: self.original_enable,
            oled_key_existed: self.original_oled_existed,
            use_oled_taskbar_transparency: self.original_oled,
        };

        match serde_json::to_string_pretty(&data) {
            Ok(json) => {
                if let Err(e) = fs::write(&path, json) {
                    eprintln!("[transparency] failed to save snapshot: {}", e);
                }
            }
            Err(e) => eprintln!("[transparency] failed to serialize snapshot: {}", e),
        }
    }

    /// 从 transparency_original.json 加载原始状态
    fn try_load_snapshot(&mut self) -> bool {
        let path = match Self::snapshot_path() {
            Some(p) => p,
            None => return false,
        };

        let json = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return false,
        };

        let data: OriginalSnapshot = match serde_json::from_str(&json) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[transparency] snapshot parse error: {}", e);
                return false;
            }
        };

        self.original_enable = data.enable_transparency;
        self.original_oled_existed = data.oled_key_existed;
        self.original_oled = data.use_oled_taskbar_transparency;
        println!("[transparency] original state loaded from snapshot");
        true
    }

    /// 应用透明效果设置到注册表（即时生效）
    /// 对齐 C#：单开关同时控制 EnableTransparency + UseOLEDTaskbarTransparency
    pub fn apply(&self, enable: bool) {
        let v = if enable { 1 } else { 0 };
        if !write_enable_transparency(v) {
            eprintln!("[transparency] failed to write EnableTransparency");
        }
        if !write_use_oled(v) {
            eprintln!("[transparency] failed to write UseOLEDTaskbarTransparency");
        }
    }

    /// 恢复为原始注册表状态（卸载时调用，对齐 C#）
    /// 退出时不自动恢复，仅在显式卸载时调用
    #[allow(dead_code)]
    pub fn restore(&self) {
        if !self.captured { return; }

        // 恢复 EnableTransparency → Themes\Personalize
        unsafe {
            if let Some(hkey) = open_personalize_key(KEY_WRITE) {
                if let Some(v) = self.original_enable {
                    write_dword(hkey, VAL_ENABLE_TRANSPARENCY, v);
                } else {
                    delete_value(hkey, VAL_ENABLE_TRANSPARENCY);
                }
                let _ = RegCloseKey(hkey);
            }
        }

        // 恢复 UseOLEDTaskbarTransparency → Explorer\Advanced
        unsafe {
            if self.original_oled_existed {
                if let Some(v) = self.original_oled {
                    write_use_oled(v);
                }
            } else {
                delete_use_oled();
            }
        }

        println!("[transparency] restored to original state");
    }
}

impl Default for TransparencyManager {
    fn default() -> Self {
        Self::new()
    }
}
