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
) -> Result<CompressOutcome, CompressError> {
    let quality = quality.clamp(10, 100);
    let format = detect_format(path, bytes)?;
    let output = match format {
        ImageFormat::Jpeg => compress_jpeg(bytes, quality)?,
        ImageFormat::Png => compress_png(bytes, quality)?,
        ImageFormat::WebP => compress_webp(bytes, quality)?,
        ImageFormat::Gif => compress_gif(bytes, quality)?,
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
            return Err(CompressError::Message(format!(
                "未知改尺寸模式: {other}"
            )));
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

fn compress_gif(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|e| CompressError::Message(e.to_string()))?;

    let width = decoder.width();
    let height = decoder.height();
    let mut frames = Vec::new();

    while let Some(frame) = decoder
        .read_next_frame()
        .map_err(|e| CompressError::Message(e.to_string()))?
    {
        frames.push(frame.clone());
    }

    if frames.is_empty() {
        return Err(CompressError::Message("GIF 无有效帧".into()));
    }

    let speed = match quality {
        90..=100 => 1,
        70..=89 => 3,
        50..=69 => 6,
        30..=49 => 10,
        _ => 15,
    };

    let mut out = Vec::with_capacity(bytes.len());
    {
        let mut encoder = gif::Encoder::new(&mut out, width, height, &[])
            .map_err(|e| CompressError::Message(e.to_string()))?;
        let _ = encoder.set_repeat(gif::Repeat::Infinite);

        for frame in frames {
            let mut rgba = frame.buffer.to_vec();
            let mut new_frame = gif::Frame::from_rgba_speed(width, height, &mut rgba, speed);
            new_frame.delay = frame.delay;
            new_frame.top = frame.top;
            new_frame.left = frame.left;
            encoder
                .write_frame(&new_frame)
                .map_err(|e| CompressError::Message(e.to_string()))?;
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
            let optimized = oxipng::optimize_from_memory(&out, &oxipng::Options::from_preset(4))
                .unwrap_or(out);
            Ok((optimized, "image/png".into()))
        }
        other => Err(CompressError::Message(format!("不支持的贴图格式: {other}"))),
    }
}
