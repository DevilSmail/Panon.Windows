using System.Text.Json;
using System.Text.Json.Serialization;

namespace Panon.Windows.Settings;

/// <summary>
/// 设置管理器，负责 JSON 文件读写
/// </summary>
public sealed class SettingsManager
{
    private static readonly string SettingsPath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
        "Panon", "settings.json");

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        Converters = { new JsonStringEnumConverter() }
    };

    public AppSettings Current { get; private set; } = new();

    public event Action<AppSettings>? SettingsChanged;

    public SettingsManager()
    {
        Load();
    }

    public void Load()
    {
        try
        {
            if (File.Exists(SettingsPath))
            {
                var json = File.ReadAllText(SettingsPath);
                Current = JsonSerializer.Deserialize<AppSettings>(json, JsonOptions) ?? new AppSettings();

                // 防御：修复被污染的字段（之前有 bug 的运行保存了无效值），仅重置无效字段
                bool dirty = false;
                if (Current.Gravity < 0 || Current.Gravity > 4) { Current.Gravity = 2; dirty = true; }       // 默认 South
                if (Current.OverlayMode < 1 || Current.OverlayMode > 2) { Current.OverlayMode = 1; dirty = true; } // 默认任务栏覆盖在频谱上面
                if (Current.HslLightness < 0 || Current.HslLightness > 95) { Current.HslLightness = 50; dirty = true; }
                if (Current.HsluvLightness < 0 || Current.HsluvLightness > 95) { Current.HsluvLightness = 50; dirty = true; }
                if (Current.BassResolutionLevel < 0 || Current.BassResolutionLevel > 6) { Current.BassResolutionLevel = 4; dirty = true; }
                if (Current.Fps < 10 || Current.Fps > 60) { Current.Fps = 30; dirty = true; }
                if (Current.FillMode < 0 || Current.FillMode > 1) { Current.FillMode = 1; dirty = true; } // 默认仅空白区域

                // 如果修复了无效字段，立即写回干净的配置文件
                if (dirty) Save();
            }
        }
        catch
        {
            Current = new AppSettings();
        }
    }

    public void Save()
    {
        try
        {
            var dir = Path.GetDirectoryName(SettingsPath)!;
            Directory.CreateDirectory(dir);
            var json = JsonSerializer.Serialize(Current, JsonOptions);
            File.WriteAllText(SettingsPath, json);
            SettingsChanged?.Invoke(Current);
        }
        catch
        {
            // 静默失败，不影响运行
        }
    }

    public void Update(Action<AppSettings> updater)
    {
        updater(Current);
        Save();
    }
}
