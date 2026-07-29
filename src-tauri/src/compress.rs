use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageEncoder, ImageFormat};
use serde::Serialize;
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
    // PNG 动画控制块
    bytes.windows(4).any(|w| w == b"acTL")
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

    let origin_size = bytes.len() as u64;
    let output_size = output.len() as u64;

    if output_size == 0 || output_size >= origin_size {
        Ok(CompressOutcome {
            origin_size,
            output_size: origin_size,
            saved: false,
            best_already: true,
            output: bytes.to_vec(),
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

fn compress_jpeg(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    let img = image::load_from_memory(bytes)?.to_rgb8();
    let mut out = Vec::with_capacity(bytes.len());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder.encode_image(&DynamicImage::ImageRgb8(img))?;
    Ok(out)
}

fn compress_png(bytes: &[u8], quality: u8) -> Result<Vec<u8>, CompressError> {
    // APNG：仅做无损优化，避免量化把动画压成静图
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

    // 先拷贝帧数据，再借给 encoder（AnimFrame 需要引用生命周期）
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

    // speed: 1 = 最慢最好，30 = 最快；质量越高 speed 越低
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
