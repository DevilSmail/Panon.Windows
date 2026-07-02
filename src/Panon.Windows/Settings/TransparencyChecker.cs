using Microsoft.Win32;
using System.Text.Json;
using Panon.Windows.Helpers;

namespace Panon.Windows.Settings;

/// <summary>
/// 透明度检测与控制器
/// 两套值：原始值（首个快照，持久化到文件，卸载恢复用）+ 运行值（用户自由开关）
/// 退出不恢复，卸载时调用 RestoreOriginal 复原
/// </summary>
public sealed class TransparencyChecker
{
    private const string PersonalizeKey = @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
    private const string ExplorerAdvancedKey = @"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

    // 原始值快照持久化路径
    private static readonly string SnapshotPath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "Panon", "transparency_original.json");

    // 原始值快照
    private int? _originalEnableTransparency;
    private bool _originalOledKeyExisted;
    private int? _originalUseOLEDTaskbarTransparency;
    private bool _captured;

    /// <summary>
    /// 当前是否已执行 Enable（运行态）
    /// </summary>
    public bool IsEnabled { get; private set; }

    /// <summary>
    /// 实时读取系统透明效果 (EnableTransparency)
    /// </summary>
    public bool IsTransparencyEnabled
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(PersonalizeKey);
            return key?.GetValue("EnableTransparency") is int value && value == 1;
        }
    }

    /// <summary>
    /// 实时读取任务栏增强透明 (UseOLEDTaskbarTransparency)
    /// </summary>
    public bool IsOLEDTaskbarTransparencyEnabled
    {
        get
        {
            using var key = Registry.CurrentUser.OpenSubKey(ExplorerAdvancedKey);
            var value = key?.GetValue("UseOLEDTaskbarTransparency");
            return value is int v && v == 1;
        }
    }

    /// <summary>
    /// 仅同步运行态标记（不写注册表也不写文件）
    /// </summary>
    public void MarkEnabled()
    {
        IsEnabled = true;
    }

    /// <summary>
    /// 记录首次启动时的原始注册表状态（持久化到文件，卸载恢复用）
    /// </summary>
    public void CaptureOriginalState()
    {
        if (_captured) return;

        // 先尝试从持久化文件加载（程序被多次启动也不会丢失）
        if (TryLoadSnapshot())
        {
            _captured = true;
            return;
        }

        // 首次启动：从注册表读取并持久化
        using (var key = Registry.CurrentUser.OpenSubKey(PersonalizeKey))
        {
            var val = key?.GetValue("EnableTransparency");
            _originalEnableTransparency = val as int?;
        }

        using (var key = Registry.CurrentUser.OpenSubKey(ExplorerAdvancedKey))
        {
            var val = key?.GetValue("UseOLEDTaskbarTransparency");
            _originalOledKeyExisted = val != null;
            _originalUseOLEDTaskbarTransparency = val as int?;
        }

        SaveSnapshot();
        _captured = true;
    }

    public void Enable()
    {
        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(PersonalizeKey);
            key?.SetValue("EnableTransparency", 1, RegistryValueKind.DWord);
        }
        catch { DebugLog.Write("Transparency registry operation failed"); }

        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(ExplorerAdvancedKey);
            key?.SetValue("UseOLEDTaskbarTransparency", 1, RegistryValueKind.DWord);
        }
        catch { DebugLog.Write("Transparency registry operation failed"); }

        IsEnabled = true;
    }

    public void Disable()
    {
        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(PersonalizeKey);
            key?.SetValue("EnableTransparency", 0, RegistryValueKind.DWord);
        }
        catch { DebugLog.Write("Transparency registry operation failed"); }

        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(ExplorerAdvancedKey);
            key?.SetValue("UseOLEDTaskbarTransparency", 0, RegistryValueKind.DWord);
        }
        catch { DebugLog.Write("Transparency registry operation failed"); }

        IsEnabled = false;
    }

    /// <summary>
    /// 恢复为原始注册表状态（卸载时调用）
    /// </summary>
    public void RestoreOriginal()
    {
        if (!_captured) return;

        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(PersonalizeKey);
            if (_originalEnableTransparency.HasValue)
                key?.SetValue("EnableTransparency", _originalEnableTransparency.Value, RegistryValueKind.DWord);
            else
                key?.DeleteValue("EnableTransparency", throwOnMissingValue: false);
        }
        catch { DebugLog.Write("Transparency registry operation failed"); }

        try
        {
            using var key = Registry.CurrentUser.CreateSubKey(ExplorerAdvancedKey);
            if (_originalOledKeyExisted && _originalUseOLEDTaskbarTransparency.HasValue)
                key?.SetValue("UseOLEDTaskbarTransparency", _originalUseOLEDTaskbarTransparency.Value, RegistryValueKind.DWord);
            else
                key?.DeleteValue("UseOLEDTaskbarTransparency", throwOnMissingValue: false);
        }
        catch { DebugLog.Write("Transparency registry operation failed"); }

        IsEnabled = false;
    }

    #region 快照持久化

    private void SaveSnapshot()
    {
        try
        {
            var dir = Path.GetDirectoryName(SnapshotPath)!;
            Directory.CreateDirectory(dir);
            var data = new OriginalSnapshot
            {
                EnableTransparency = _originalEnableTransparency,
                OledKeyExisted = _originalOledKeyExisted,
                UseOLEDTaskbarTransparency = _originalUseOLEDTaskbarTransparency
            };
            File.WriteAllText(SnapshotPath, JsonSerializer.Serialize(data));
        }
        catch { DebugLog.Write("Transparency registry operation failed"); }
    }

    private bool TryLoadSnapshot()
    {
        try
        {
            if (!File.Exists(SnapshotPath)) return false;
            var json = File.ReadAllText(SnapshotPath);
            var data = JsonSerializer.Deserialize<OriginalSnapshot>(json);
            if (data == null) return false;
            _originalEnableTransparency = data.EnableTransparency;
            _originalOledKeyExisted = data.OledKeyExisted;
            _originalUseOLEDTaskbarTransparency = data.UseOLEDTaskbarTransparency;
            return true;
        }
        catch
        {
            return false;
        }
    }

    private sealed class OriginalSnapshot
    {
        public int? EnableTransparency { get; set; }
        public bool OledKeyExisted { get; set; }
        public int? UseOLEDTaskbarTransparency { get; set; }
    }

    #endregion
}
