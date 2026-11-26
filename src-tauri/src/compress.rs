use std::borrow::Cow;
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageEncoder, ImageFormat};
use serde::{Deserialize, Serialize};
use webp::{AnimDecoder, AnimEncoder, AnimFrame, BitstreamFeatures, Encoder, WebPConfig};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressOutcome {
    pub origin_size: u64,
    pub output_size: u64,
    pub saved: bool,
    pub best_already: bool,
    pub output: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResizeSpec {
    /// width | height | percent
    pub mode: String,
    /// 目标宽/高（像素）或百分比（如 50 = 50%）
    pub value: f64,
    /// 按宽/高时默认锁比例；百分比忽略此字段
    pub lock_aspect: bool,
}

/// GIF 重编码参数（仅对 GIF 生效）
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GifOptions {
    /// 帧率上限，0 = 保持原始帧率
    pub max_fps: u32,
    /// 调色板颜色数 2–256
    pub max_colors: u32,
    /// none | bayer | floyd_steinberg
    pub dither: String,
    /// 最大宽度（像素），0 = 不缩放
    pub max_width: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum CompressError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Image(#[from] image::ImageError),
}

impl From<&str> for CompressError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<String> for CompressError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

pub fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "webp"
            )
        })
        .unwrap_or(false)
}

pub fn is_supported_model(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "glb" | "gltf"))
        .unwrap_or(false)
}

pub fn detect_format(path: &Path, bytes: &[u8]) -> Result<ImageFormat, CompressError> {
    if let Ok(format) = image::guess_format(bytes) {
        return Ok(format);
    }
    ImageFormat::from_path(path).map_err(|e| CompressError::Message(e.to_string()))
}

fn is_apng(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|w| w == b"acTL")
}

fn is_animated(format: ImageFormat, bytes: &[u8]) -> bool {
    match format {
        ImageFormat::Gif => gif_frame_count(bytes).map(|n| n > 1).unwrap_or(true),
        ImageFormat::Png => is_apng(bytes),
        ImageFormat::WebP => BitstreamFeatures::new(bytes)
            .map(|f| f.has_animation())
            .unwrap_or(false),
        _ => false,
    }
}

fn gif_frame_count(bytes: &[u8]) -> Result<usize, CompressError> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|e| CompressError::Message(e.to_string()))?;
    let mut n = 0usize;
    while decoder
        .read_next_frame()
        .map_err(|e| CompressError::Message(e.to_string()))?
        .is_some()
    {
        n += 1;
        if n > 1 {
            break;
        }
    }
    Ok(n)
}

pub fn compress_bytes(
    path: &Path,
    bytes: &[u8],
    quality: u8,
    gif: Option<&GifOptions>,
) -> Result<CompressOutcome, CompressError> {
    let quality = quality.clamp(10, 100);
    let format = detect_format(path, bytes)?;
    let output = match format {
        ImageFormat::Jpeg => compress_jpeg(bytes, quality)?,
        ImageFormat::Png => compress_png(bytes, quality)?,
        ImageFormat::WebP => compress_webp(bytes, quality)?,
        ImageFormat::Gif => compress_gif(bytes, quality, gif)?,
        _ => {
            return Err(CompressError::Message(format!(
                "不支持的格式: {}",
                path.display()
            )))
        }
    };

    finish_outcome(bytes, output, false)
}

/// 改尺寸（可附带质量压缩）。动图暂不支持改尺寸。
pub fn resize_and_compress(
    path: &Path,
    bytes: &[u8],
    quality: u8,
    spec: &ResizeSpec,
) -> Result<CompressOutcome, CompressError> {
    let quality = quality.clamp(10, 100);
    let format = detect_format(path, bytes)?;

    if is_animated(format, bytes) {
        return Err(CompressError::Message(
            "动图暂不支持改尺寸，请使用「压缩」区（不改宽高）".into(),
        ));
    }

    let img = image::load_from_memory(bytes)?;
    let (ow, oh) = img.dimensions();
    let (nw, nh) = compute_target_size(ow, oh, spec)?;
    let dims_changed = nw != ow || nh != oh;

    let resized = if dims_changed {
        img.resize_exact(nw, nh, FilterType::Lanczos3)
    } else {
        img
    };

    let output = encode_image(format, &resized, quality)?;
    finish_outcome(bytes, output, dims_changed)
}

fn finish_outcome(
    origin_bytes: &[u8],
    output: Vec<u8>,
    force_save: bool,
) -> Result<CompressOutcome, CompressError> {
    let origin_size = origin_bytes.len() as u64;
    let output_size = output.len() as u64;

    if output_size == 0 || (!force_save && output_size >= origin_size) {
        Ok(CompressOutcome {
            origin_size,
            output_size: origin_size,
            saved: false,
            best_already: true,
            output: origin_bytes.to_vec(),
        })
    } else {
        Ok(CompressOutcome {
            origin_size,
            output_size,
            saved: true,
            best_already: false,
            output,
        })
    }
}

fn compute_target_size(ow: u32, oh: u32, spec: &ResizeSpec) -> Result<(u32, u32), CompressError> {
    let ow = ow.max(1) as f64;
    let oh = oh.max(1) as f64;
    let mode = spec.mode.to_ascii_lowercase();
    let value = spec.value;

    let (nw, nh) = match mode.as_str() {
        "percent" => {
            if !(1.0..=400.0).contains(&value) {
                return Err(CompressError::Message("百分比请输入 1–400".into()));
            }
            let s = value / 100.0;
            ((ow * s).round(), (oh * s).round())
        }
        "width" => {
            if !(1.0..=16384.0).contains(&value) {
                return Err(CompressError::Message("宽度请输入 1–16384".into()));
            }
            let nw = value.round();
            let nh = if spec.lock_aspect {
                (oh * nw / ow).round()
            } else {
                oh
            };
            (nw, nh)
        }
        "height" => {
            if !(1.0..=16384.0).contains(&value) {
                return Err(CompressError::Message("高度请输入 1–16384".into()));
            }
            let nh = value.round();
            let nw = if spec.lock_aspect {
                (ow * nh / oh).round()
            } else {
                ow
            };
            (nw, nh)
        }
        other => {
            return Err(CompressError::Message(format!("未知改尺寸模式: {other}")));
        }
    };

    Ok((nw.max(1.0) as u32, nh.max(1.0) as u32))
}

fn encode_image(
    format: ImageFormat,
    img: &DynamicImage,
    quality: u8,
) -> Result<Vec<u8>, CompressError> {
    match format {
        ImageFormat::Jpeg => {
            let rgb = img.to_rgb8();
            let mut out = Vec::new();
            let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
            encoder.encode_image(&DynamicImage::ImageRgb8(rgb))?;
            Ok(out)
        }
        ImageFormat::Png => {
            let rgba = img.to_rgba8();
            let mut raw = Vec::new();
            image::codecs::png::PngEncoder::new(&mut raw).write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )?;
            compress_png(&raw, quality)
        }
        ImageFormat::WebP => {
            let encoder = Encoder::from_image(img)
                .map_err(|e| CompressError::Message(format!("WebP 编码失败: {e}")))?;
            Ok(encoder.encode(quality as f32).to_vec())
        }
        ImageFormat::Gif => {
            let mut rgba = img.to_rgba8().into_raw();
            let (w, h) = img.dimensions();
            let speed = match quality {
                90..=100 => 1,
                70..=89 => 3,
                50..=69 => 6,
                30..=49 => 10,
                _ => 15,
            };
            let mut out = Vec::new();
            {
                let mut encoder = gif::Encoder::new(&mut out, w as u16, h as u16, &[])
                    .map_err(|e| CompressError::Message(e.to_string()))?;
                let frame = gif::Frame::from_rgba_speed(w as u16, h as u16, &mut rgba, speed);
                encoder
                    .write_frame(&frame)
                    .map_err(|e| CompressError::Message(e.to_string()))?;
            }
            Ok(out)
        }
        _ => Err(CompressError::Message("不支持的编码格式".into())),
    }
}

fn compress_jpeg(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    let img = image::load_from_memory(bytes)?.to_rgb8();
    let mut out = Vec::with_capacity(bytes.len());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder.encode_image(&DynamicImage::ImageRgb8(img))?;
    Ok(out)
}

fn compress_png(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    let raw = if is_apng(bytes) {
        bytes.to_vec()
    } else if quality >= 85 {
        bytes.to_vec()
    } else {
        quantize_png(bytes, quality)?
    };

    let preset = match quality {
        90..=100 => 3,
        70..=89 => 4,
        50..=69 => 5,
        _ => 6,
    };
    let options = oxipng::Options::from_preset(preset);
    oxipng::optimize_from_memory(&raw, &options).map_err(|e| CompressError::Message(e.to_string()))
}

fn quantize_png(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    let rgba = image::load_from_memory(bytes)?.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut pixels = rgba.into_raw();

    let colors = match quality {
        70..=84 => 256,
        50..=69 => 192,
        30..=49 => 128,
        _ => 64,
    };

    let quantizer = color_quant::NeuQuant::new(10, colors, &pixels);
    for chunk in pixels.chunks_exact_mut(4) {
        quantizer.map_pixel(chunk);
    }

    let img = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or_else(|| CompressError::Message("PNG 量化失败".into()))?;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(
        img.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

fn compress_webp(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    if BitstreamFeatures::new(bytes)
        .map(|f| f.has_animation())
        .unwrap_or(false)
    {
        return compress_animated_webp(bytes, quality);
    }

    let img = image::load_from_memory(bytes)?;
    let encoder = Encoder::from_image(&img)
        .map_err(|e| CompressError::Message(format!("WebP 编码失败: {e}")))?;
    let encoded = encoder.encode(quality as f32);
    Ok(encoded.to_vec())
}

fn compress_animated_webp(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    let decoded = AnimDecoder::new(bytes)
        .decode()
        .map_err(|e| CompressError::Message(format!("动图 WebP 解码失败: {e}")))?;

    if decoded.len() == 0 {
        return Err(CompressError::Message("动图 WebP 无有效帧".into()));
    }

    let first = decoded
        .get_frame(0)
        .ok_or_else(|| CompressError::Message("动图 WebP 无有效帧".into()))?;
    let width = first.width();
    let height = first.height();

    let mut config =
        WebPConfig::new().map_err(|_| CompressError::Message("WebP 配置初始化失败".into()))?;
    if quality >= 92 {
        config.lossless = 1;
        config.quality = 100.0;
    } else {
        config.lossless = 0;
        config.quality = quality as f32;
    }

    let owned: Vec<(Vec<u8>, webp::PixelLayout, u32, u32, i32)> = (&decoded)
        .into_iter()
        .map(|frame| {
            (
                frame.get_image().to_vec(),
                frame.get_layout(),
                frame.width(),
                frame.height(),
                frame.get_time_ms(),
            )
        })
        .collect();

    let mut encoder = AnimEncoder::new(width, height, &config);
    encoder.set_loop_count(decoded.loop_count as i32);
    let bg = decoded.bg_color;
    encoder.set_bgcolor([
        (bg & 0xff) as u8,
        ((bg >> 8) & 0xff) as u8,
        ((bg >> 16) & 0xff) as u8,
        ((bg >> 24) & 0xff) as u8,
    ]);

    for (img, layout, w, h, ts) in &owned {
        encoder.add_frame(AnimFrame::new(img, *layout, *w, *h, *ts, None));
    }

    let encoded = encoder
        .try_encode()
        .map_err(|e| CompressError::Message(format!("动图 WebP 编码失败: {e:?}")))?;
    Ok(encoded.to_vec())
}

fn gif_speed(quality: u8) -> i32 {
    match quality {
        90..=100 => 1,
        70..=89 => 3,
        50..=69 => 6,
        30..=49 => 10,
        _ => 15,
    }
}

/// 质量越低，调色板色数越少（主要体积杠杆之一）
fn gif_colors(quality: u8) -> usize {
    match quality {
        90..=100 => 256,
        70..=89 => 128,
        50..=69 => 96,
        30..=49 => 64,
        _ => 32,
    }
}

/// 整段动画共用的调色板。颜色种类少时直接精确建表（无损、且省掉 768 字节开销），
/// 颜色多时才用 NeuQuant 量化。索引稳定才能可靠判断“像素没变”。
enum GifPalette {
    Exact {
        rgb: Vec<u8>,
        transparent: u8,
    },
    Quantized {
        rgb: Vec<u8>,
        nq: color_quant::NeuQuant,
        transparent: u8,
    },
}

fn rgb_key(px: &[u8]) -> u32 {
    ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32
}

impl GifPalette {
    fn rgb(&self) -> &[u8] {
        match self {
            GifPalette::Exact { rgb, .. } | GifPalette::Quantized { rgb, .. } => rgb,
        }
    }

    fn transparent(&self) -> u8 {
        match self {
            GifPalette::Exact { transparent, .. } | GifPalette::Quantized { transparent, .. } => {
                *transparent
            }
        }
    }

    /// 不透明颜色数（末位是透明占位）
    fn opaque_len(&self) -> usize {
        self.rgb().len() / 3 - 1
    }

    fn color_at(&self, idx: u8) -> [u8; 3] {
        let rgb = self.rgb();
        let o = idx as usize * 3;
        [rgb[o], rgb[o + 1], rgb[o + 2]]
    }

    /// 最接近的调色板索引；带抖动的像素值可能不再是调色板里已有的颜色
    fn nearest(&self, r: i32, g: i32, b: i32) -> u8 {
        match self {
            GifPalette::Quantized { nq, .. } => nq.index_of(&[
                r.clamp(0, 255) as u8,
                g.clamp(0, 255) as u8,
                b.clamp(0, 255) as u8,
                255,
            ]) as u8,
            GifPalette::Exact { rgb, .. } => {
                let mut best = 0u8;
                let mut best_d = i32::MAX;
                for i in 0..self.opaque_len() {
                    let o = i * 3;
                    let dr = r - rgb[o] as i32;
                    let dg = g - rgb[o + 1] as i32;
                    let db = b - rgb[o + 2] as i32;
                    let d = dr * dr + dg * dg + db * db;
                    if d < best_d {
                        best_d = d;
                        best = i as u8;
                        if d == 0 {
                            break;
                        }
                    }
                }
                best
            }
        }
    }
}

fn build_gif_palette(canvases: &[GifCanvasFrame], colors: usize, speed: i32) -> GifPalette {
    let total_px: usize = canvases.iter().map(|f| f.rgba.len() / 4).sum();
    let step = (total_px / 200_000).max(1);
    let mut sample = Vec::new();
    for f in canvases {
        for px in f.rgba.chunks_exact(4).step_by(step) {
            if px[3] != 0 {
                sample.extend_from_slice(px);
            }
        }
    }
    if sample.is_empty() {
        sample.extend_from_slice(&[0, 0, 0, 255]);
    }

    // 颜色很少（贴纸/图标类）：精确调色板，无损且省掉整张调色板开销
    let exact_max = colors.clamp(2, 256).min(64);
    let mut seen: HashSet<u32> = HashSet::new();
    for px in sample.chunks_exact(4) {
        seen.insert(rgb_key(px));
        if seen.len() > exact_max {
            break;
        }
    }
    if seen.len() <= exact_max {
        let mut rgb = Vec::with_capacity(seen.len() * 3);
        for key in seen {
            rgb.push((key >> 16) as u8);
            rgb.push((key >> 8) as u8);
            rgb.push(key as u8);
        }
        let transparent = (rgb.len() / 3) as u8;
        rgb.extend_from_slice(&[0, 0, 0]);
        return GifPalette::Exact { rgb, transparent };
    }

    let colors = colors.clamp(2, 255);
    let nq = color_quant::NeuQuant::new(speed, colors, &sample);
    let mut rgb = nq.color_map_rgb();
    let transparent = (rgb.len() / 3) as u8;
    rgb.extend_from_slice(&[0, 0, 0]);
    GifPalette::Quantized {
        rgb,
        nq,
        transparent,
    }
}

/// 8x8 有序抖动矩阵（0–63）
const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// 把整张画布映射成调色板索引；alpha=0 的像素写成透明索引。
fn map_canvas_indices(
    rgba: &[u8],
    width: usize,
    height: usize,
    palette: &GifPalette,
    transparent: u8,
    dither: &str,
) -> Vec<u8> {
    let mut out = vec![transparent; width * height];

    if dither == "floyd_steinberg" {
        let mut buf: Vec<f32> = Vec::with_capacity(width * height * 3);
        for px in rgba.chunks_exact(4) {
            buf.push(px[0] as f32);
            buf.push(px[1] as f32);
            buf.push(px[2] as f32);
        }
        let mut add = |buf: &mut Vec<f32>, x: usize, y: usize, err: [f32; 3], w: f32| {
            let o = (y * width + x) * 3;
            buf[o] += err[0] * w;
            buf[o + 1] += err[1] * w;
            buf[o + 2] += err[2] * w;
        };
        for y in 0..height {
            let ltr = y % 2 == 0;
            for step in 0..width {
                let x = if ltr { step } else { width - 1 - step };
                let i = y * width + x;
                if rgba[i * 4 + 3] == 0 {
                    continue;
                }
                let o = i * 3;
                let r = buf[o].round().clamp(0.0, 255.0) as i32;
                let g = buf[o + 1].round().clamp(0.0, 255.0) as i32;
                let b = buf[o + 2].round().clamp(0.0, 255.0) as i32;
                let idx = palette.nearest(r, g, b);
                out[i] = idx;
                let [pr, pg, pb] = palette.color_at(idx);
                let err = [
                    r as f32 - pr as f32,
                    g as f32 - pg as f32,
                    b as f32 - pb as f32,
                ];
                if err[0].abs() < 0.5 && err[1].abs() < 0.5 && err[2].abs() < 0.5 {
                    continue;
                }
                let x_next = if ltr {
                    if x + 1 < width {
                        Some(x + 1)
                    } else {
                        None
                    }
                } else if x > 0 {
                    Some(x - 1)
                } else {
                    None
                };
                let x_back = if ltr {
                    if x > 0 {
                        Some(x - 1)
                    } else {
                        None
                    }
                } else if x + 1 < width {
                    Some(x + 1)
                } else {
                    None
                };
                if let Some(xn) = x_next {
                    add(&mut buf, xn, y, err, 7.0 / 16.0);
                }
                if y + 1 < height {
                    if let Some(xb) = x_back {
                        add(&mut buf, xb, y + 1, err, 3.0 / 16.0);
                    }
                    add(&mut buf, x, y + 1, err, 5.0 / 16.0);
                    if let Some(xn) = x_next {
                        add(&mut buf, xn, y + 1, err, 1.0 / 16.0);
                    }
                }
            }
        }
        return out;
    }

    let bayer_amp = if dither == "bayer" {
        // 抖动幅度取量化台阶的一小部分：既能缓解色带，又不会把差分压缩冲垮
        255.0 / (palette.opaque_len().max(2) as f32).cbrt() * 0.25
    } else {
        0.0
    };

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let o = i * 4;
            if rgba[o + 3] == 0 {
                continue;
            }
            let (r, g, b) = if bayer_amp > 0.0 {
                let off = (BAYER8[y % 8][x % 8] as f32 / 64.0 - 0.5) * bayer_amp;
                (
                    rgba[o] as f32 + off,
                    rgba[o + 1] as f32 + off,
                    rgba[o + 2] as f32 + off,
                )
            } else {
                (rgba[o] as f32, rgba[o + 1] as f32, rgba[o + 2] as f32)
            };
            out[i] = palette.nearest(r.round() as i32, g.round() as i32, b.round() as i32);
        }
    }
    out
}

/// 按帧率上限抽帧：被丢掉的帧时长并入上一帧，总时长不变
fn cap_frame_rate(frames: Vec<GifCanvasFrame>, max_fps: u32) -> Vec<GifCanvasFrame> {
    if max_fps == 0 || frames.len() <= 1 {
        return frames;
    }
    let interval_ms = 1000.0 / max_fps as f64;
    let mut out: Vec<GifCanvasFrame> = Vec::with_capacity(frames.len());
    let mut elapsed_ms = 0.0f64;
    let mut next_due = 0.0f64;
    let last_index = frames.len() - 1;
    for (i, f) in frames.into_iter().enumerate() {
        let start = elapsed_ms;
        elapsed_ms += (f.delay.max(1) as f64) * 10.0;
        let keep = out.is_empty() || i == last_index || start + 1e-6 >= next_due;
        if keep {
            out.push(GifCanvasFrame {
                rgba: f.rgba,
                delay: f.delay,
            });
            next_due = start + interval_ms;
        } else if let Some(last) = out.last_mut() {
            last.delay = (last.delay as u32 + f.delay as u32).min(u16::MAX as u32) as u16;
        }
    }
    out
}

fn gif_indexed_frame(
    indices: &[u8],
    width: u16,
    height: u16,
    left: u16,
    top: u16,
    transparent: u8,
    delay: u16,
) -> gif::Frame<'static> {
    let mut frame = gif::Frame::default();
    frame.width = width;
    frame.height = height;
    frame.left = left;
    frame.top = top;
    frame.buffer = Cow::Owned(indices.to_vec());
    frame.palette = None; // 使用全局调色板
    frame.transparent = Some(transparent);
    frame.delay = delay;
    frame.dispose = gif::DisposalMethod::Keep;
    frame
}

/// 1x1 透明帧：画面不变，只推进时间（延时）
fn write_gif_blank(
    encoder: &mut gif::Encoder<&mut Vec<u8>>,
    transparent: u8,
    delay: u16,
) -> Result<(), CompressError> {
    let frame = gif_indexed_frame(&[transparent], 1, 1, 0, 0, transparent, delay);
    encoder
        .write_frame(&frame)
        .map_err(|e| CompressError::Message(e.to_string()))
}

struct GifCanvasFrame {
    rgba: Vec<u8>,
    delay: u16,
}

/// 解码 GIF 并自行按 offset/disposal 合成每一帧。
/// 不含透明通道的 GIF 用背景色合成（与浏览器渲染一致），这样才能用差分帧重编码；
/// 含透明通道的 GIF 背景保持透明。
fn decode_gif_canvases(
    bytes: &[u8],
) -> Result<(Vec<GifCanvasFrame>, gif::Repeat, usize, usize), CompressError> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|e| CompressError::Message(e.to_string()))?;

    let width = decoder.width() as usize;
    let height = decoder.height() as usize;
    let repeat = decoder.repeat();
    let bg_index = decoder.bg_color();
    let global_palette = decoder.global_palette().map(|p| p.to_vec());

    struct RawFrame {
        left: usize,
        top: usize,
        width: usize,
        height: usize,
        delay: u16,
        dispose: gif::DisposalMethod,
        transparent: Option<u8>,
        palette: Vec<u8>,
        indices: Vec<u8>,
    }

    let mut raws: Vec<RawFrame> = Vec::new();
    loop {
        let frame = {
            let Some(frame) = decoder
                .read_next_frame()
                .map_err(|e| CompressError::Message(e.to_string()))?
            else {
                break;
            };
            frame.clone()
        };
        let palette = decoder
            .palette()
            .map_err(|e| CompressError::Message(e.to_string()))?
            .to_vec();
        raws.push(RawFrame {
            left: frame.left as usize,
            top: frame.top as usize,
            width: frame.width as usize,
            height: frame.height as usize,
            delay: frame.delay,
            dispose: frame.dispose,
            transparent: frame.transparent,
            palette,
            indices: frame.buffer.to_vec(),
        });
    }

    if raws.is_empty() {
        return Err(CompressError::Message("GIF 无有效帧".into()));
    }
    if width == 0 || height == 0 || width > u16::MAX as usize || height > u16::MAX as usize {
        return Err(CompressError::Message("GIF 尺寸异常".into()));
    }

    let transparent_bg = raws.iter().any(|f| f.transparent.is_some());
    let bg: [u8; 4] = if transparent_bg {
        [0, 0, 0, 0]
    } else {
        match (global_palette.as_deref(), bg_index) {
            (Some(p), Some(i)) if i * 3 + 2 < p.len() => {
                [p[i * 3], p[i * 3 + 1], p[i * 3 + 2], 255]
            }
            _ => [0, 0, 0, 255],
        }
    };

    let mut canvas = vec![0u8; width * height * 4];
    for px in canvas.chunks_exact_mut(4) {
        px.copy_from_slice(&bg);
    }

    let mut canvases = Vec::with_capacity(raws.len());
    for f in &raws {
        let before = if f.dispose == gif::DisposalMethod::Previous {
            Some(canvas.clone())
        } else {
            None
        };

        for y in 0..f.height {
            let cy = f.top + y;
            if cy >= height {
                break;
            }
            for x in 0..f.width {
                let cx = f.left + x;
                if cx >= width {
                    continue;
                }
                let idx = f.indices[y * f.width + x];
                if f.transparent == Some(idx) {
                    continue;
                }
                let p = idx as usize * 3;
                if p + 2 >= f.palette.len() {
                    continue;
                }
                let o = (cy * width + cx) * 4;
                canvas[o] = f.palette[p];
                canvas[o + 1] = f.palette[p + 1];
                canvas[o + 2] = f.palette[p + 2];
                canvas[o + 3] = 255;
            }
        }

        canvases.push(GifCanvasFrame {
            rgba: canvas.clone(),
            delay: f.delay,
        });

        match f.dispose {
            gif::DisposalMethod::Background => {
                for y in 0..f.height {
                    let cy = f.top + y;
                    if cy >= height {
                        break;
                    }
                    let start = (cy * width + f.left) * 4;
                    let end = (cy * width + (f.left + f.width).min(width)) * 4;
                    for px in canvas[start..end].chunks_exact_mut(4) {
                        px.copy_from_slice(&bg);
                    }
                }
            }
            gif::DisposalMethod::Previous => {
                if let Some(b) = before {
                    canvas = b;
                }
            }
            _ => {}
        }
    }

    Ok((canvases, repeat, width, height))
}

fn resize_frame(rgba: &[u8], width: usize, height: usize, nw: usize, nh: usize) -> Vec<u8> {
    let Some(img) = image::RgbaImage::from_raw(width as u32, height as u32, rgba.to_vec()) else {
        return rgba.to_vec();
    };
    image::imageops::resize(&img, nw as u32, nh as u32, FilterType::Lanczos3).into_raw()
}

fn compress_gif(
    bytes: &[u8],
    quality: u8,
    gif: Option<&GifOptions>,
) -> Result<Vec<u8>, CompressError> {
    let (mut canvases, repeat, mut width, mut height) = decode_gif_canvases(bytes)?;
    let colors = gif
        .map(|g| g.max_colors.clamp(2, 256) as usize)
        .unwrap_or_else(|| gif_colors(quality));
    let speed = gif_speed(quality);
    let dither = gif.map(|g| g.dither.as_str()).unwrap_or("none");

    if let Some(g) = gif {
        // 限宽：按比例缩放每帧画布
        let max_width = g.max_width as usize;
        if max_width > 0 && width > max_width {
            let new_w = max_width;
            let new_h = ((height as f64) * (new_w as f64) / (width as f64)).round() as usize;
            let new_h = new_h.max(1);
            for f in canvases.iter_mut() {
                f.rgba = resize_frame(&f.rgba, width, height, new_w, new_h);
            }
            width = new_w;
            height = new_h;
        }
        canvases = cap_frame_rate(canvases, g.max_fps);
    }

    let (w16, h16) = (width as u16, height as u16);

    // 全局共用调色板 + 索引图：同一颜色在所有帧里索引一致，
    // 才能可靠判断“像素没变”，进而只用差分矩形重编码。
    let palette = build_gif_palette(&canvases, colors, speed);
    let transparent = palette.transparent();
    let indexed: Vec<Vec<u8>> = canvases
        .iter()
        .map(|f| map_canvas_indices(&f.rgba, width, height, &palette, transparent, dither))
        .collect();

    // 出现「上一帧看得见、这一帧变透明」的像素时，透明差分无法表达“擦除”，
    // 退回整帧编码 + 每帧清屏，保证画面正确。
    let has_erase = (1..indexed.len()).any(|i| {
        indexed[i - 1]
            .iter()
            .zip(indexed[i].iter())
            .any(|(p, c)| *p != transparent && *c == transparent)
    });

    let mut out = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut out, w16, h16, palette.rgb())
            .map_err(|e| CompressError::Message(e.to_string()))?;
        let _ = encoder.set_repeat(repeat);

        if has_erase {
            for (i, f) in canvases.iter().enumerate() {
                let mut frame =
                    gif_indexed_frame(&indexed[i], w16, h16, 0, 0, transparent, f.delay);
                frame.dispose = gif::DisposalMethod::Background;
                encoder
                    .write_frame(&frame)
                    .map_err(|e| CompressError::Message(e.to_string()))?;
            }
        } else {
            for i in 0..indexed.len() {
                let cur = &indexed[i];
                let delay = canvases[i].delay;

                if i == 0 {
                    let frame = gif_indexed_frame(cur, w16, h16, 0, 0, transparent, delay);
                    encoder
                        .write_frame(&frame)
                        .map_err(|e| CompressError::Message(e.to_string()))?;
                    continue;
                }

                // 差分帧：只编码与上一帧相比变化的矩形区域，未变像素置透明
                let prev = &indexed[i - 1];
                let (mut min_x, mut min_y, mut max_x, mut max_y) = (width, height, 0usize, 0usize);
                let mut changed = false;
                for y in 0..height {
                    let row = y * width;
                    for x in 0..width {
                        if cur[row + x] != prev[row + x] {
                            changed = true;
                            min_x = min_x.min(x);
                            min_y = min_y.min(y);
                            max_x = max_x.max(x);
                            max_y = max_y.max(y);
                        }
                    }
                }

                if !changed {
                    write_gif_blank(&mut encoder, transparent, delay)?;
                    continue;
                }

                let bw = max_x - min_x + 1;
                let bh = max_y - min_y + 1;
                let mut sub = vec![transparent; bw * bh];
                for y in 0..bh {
                    let src_row = (min_y + y) * width + min_x;
                    let dst_row = y * bw;
                    for x in 0..bw {
                        if cur[src_row + x] != prev[src_row + x] {
                            sub[dst_row + x] = cur[src_row + x];
                        }
                    }
                }

                let frame = gif_indexed_frame(
                    &sub,
                    bw as u16,
                    bh as u16,
                    min_x as u16,
                    min_y as u16,
                    transparent,
                    delay,
                );
                encoder
                    .write_frame(&frame)
                    .map_err(|e| CompressError::Message(e.to_string()))?;
            }
        }
    }

    Ok(out)
}

/// GLB 贴图压缩：缩放 + 转 WebP/JPG/PNG（libwebp，比浏览器 canvas 小很多）
pub fn compress_texture_image(
    bytes: &[u8],
    target_format: &str,
    quality: u8,
    max_size: u32,
) -> Result<(Vec<u8>, String), CompressError> {
    let quality = quality.clamp(1, 100);
    let mut img = image::load_from_memory(bytes)?;

    if max_size > 0 {
        let (w, h) = img.dimensions();
        let max_dim = w.max(h);
        if max_dim > max_size {
            let scale = max_size as f32 / max_dim as f32;
            let nw = ((w as f32) * scale).round().max(1.0) as u32;
            let nh = ((h as f32) * scale).round().max(1.0) as u32;
            img = img.resize_exact(nw, nh, FilterType::Lanczos3);
        }
    }

    match target_format {
        "webp" => {
            let encoder = Encoder::from_image(&img)
                .map_err(|e| CompressError::Message(format!("WebP 编码失败: {e}")))?;
            Ok((encoder.encode(quality as f32).to_vec(), "image/webp".into()))
        }
        "jpeg" | "jpg" => {
            let rgb = img.to_rgb8();
            let mut out = Vec::new();
            let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
            encoder.encode_image(&DynamicImage::ImageRgb8(rgb))?;
            Ok((out, "image/jpeg".into()))
        }
        "png" => {
            let rgba = img.to_rgba8();
            let mut out = Vec::new();
            image::codecs::png::PngEncoder::new(&mut out).write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                image::ExtendedColorType::Rgba8,
            )?;
            let optimized =
                oxipng::optimize_from_memory(&out, &oxipng::Options::from_preset(4)).unwrap_or(out);
            Ok((optimized, "image/png".into()))
        }
        other => Err(CompressError::Message(format!("不支持的贴图格式: {other}"))),
    }
}
