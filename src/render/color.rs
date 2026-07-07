// color.rs — HSL/HSLuv → RGB 渐变（← ColorProcessor.cs）

/// HSL → RGB
/// h: 0~360, s: 0~1, l: 0~1 → (r, g, b) each 0~1
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let mut h = h % 360.0;
    if h < 0.0 {
        h += 360.0;
    }

    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (r + m, g + m, b + m)
}

/// HSLuv → RGB（简化实现，对齐 C# 版本：先用 HSL 近似）
/// h: 0~360, s: 0~100, l: 0~100
pub fn hsluv_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let hsl_h = h % 360.0;
    let hsl_h = if hsl_h < 0.0 { hsl_h + 360.0 } else { hsl_h };
    hsl_to_rgb(hsl_h, s / 100.0, l / 100.0)
}

/// 渐变颜色
/// pos: 0~1
/// 返回 (b, g, r) 字节值（BGRA 字节序，用于直接写入 DIB）
pub fn gradient_color(
    pos: f32,
    use_hsluv: bool,
    hue_from: i32,
    hue_to: i32,
    saturation: i32,
    lightness: i32,
) -> (u8, u8, u8) {
    let hue = hue_from as f32 + (hue_to - hue_from) as f32 * pos;
    let (r, g, b) = if use_hsluv {
        hsluv_to_rgb(hue, saturation as f32, lightness as f32)
    } else {
        hsl_to_rgb(hue, saturation as f32 / 100.0, lightness as f32 / 100.0)
    };
    ((b * 255.0) as u8, (g * 255.0) as u8, (r * 255.0) as u8)
}
