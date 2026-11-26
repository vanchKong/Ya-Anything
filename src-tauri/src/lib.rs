mod compress;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use compress::{
    compress_bytes, compress_texture_image, is_supported_image, is_supported_model,
    resize_and_compress, GifOptions, ResizeSpec,
};
use serde::Serialize;
use tauri::State;
use walkdir::WalkDir;

#[derive(Clone)]
pub struct SessionState {
    /// 原图字节，用于会话内回退与对比
    originals: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// 已成功覆盖写入的路径（压缩结果读磁盘，避免再拷一份到内存）
    overwritten: Arc<Mutex<HashSet<String>>>,
    /// 压缩后字节，供非 overwrite 模式 get_preview 使用（key = 原路径）
    compressed: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            originals: Arc::new(Mutex::new(HashMap::new())),
            overwritten: Arc::new(Mutex::new(HashSet::new())),
            compressed: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectResult {
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressResult {
    pub path: String,
    pub name: String,
    pub origin_size: u64,
    pub output_size: u64,
    pub ratio: f64,
    pub status: String,
    pub message: String,
    pub output_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResult {
    pub mime: String,
    pub origin_base64: String,
    pub compressed_base64: String,
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// 给可能冲突的路径加编号后缀：a.png → a.2.png → a.3.png …
/// 只对 newfile/custom 模式生效；overwrite 直接写入原路径
fn resolve_conflict_path(target: &PathBuf, is_overwrite: bool) -> PathBuf {
    if is_overwrite {
        return target.clone();
    }
    if !target.exists() {
        return target.clone();
    }
    let parent = target.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let stem = target
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output")
        .to_string();
    let ext = target
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| format!(".{s}"))
        .unwrap_or_default();
    for n in 2..=9999 {
        let candidate = parent.join(format!("{stem}.{n}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // 极端兜底，理论不会到
    parent.join(format!("{stem}.dup{ext}"))
}

fn mime_of(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn collect_from_path(root: &Path, out: &mut Vec<String>, model: bool) {
    let check = if model {
        is_supported_model as fn(&Path) -> bool
    } else {
        is_supported_image as fn(&Path) -> bool
    };

    if root.is_file() {
        if check(root) {
            out.push(root.to_string_lossy().to_string());
        }
        return;
    }

    if !root.is_dir() {
        return;
    }

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') {
            continue;
        }
        if check(path) {
            out.push(path.to_string_lossy().to_string());
        }
    }
}

fn compress_image_inner(
    originals: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    overwritten: Arc<Mutex<HashSet<String>>>,
    compressed: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    path: String,
    quality: u8,
    resize: Option<ResizeSpec>,
    output_path: Option<String>,
    gif_options: Option<GifOptions>,
) -> Result<CompressResult, String> {
    let path_buf = PathBuf::from(&path);
    if !is_supported_image(&path_buf) {
        return Err("不支持的文件类型".into());
    }

    let origin = {
        let mut map = originals.lock().map_err(|_| "状态锁定失败".to_string())?;
        if let Some(existing) = map.get(&path) {
            existing.clone()
        } else {
            let bytes = fs::read(&path_buf).map_err(|e| format!("读取失败: {e}"))?;
            map.insert(path.clone(), bytes.clone());
            bytes
        }
    };
    let origin_size = origin.len() as u64;

    let outcome = if let Some(spec) = resize.as_ref() {
        resize_and_compress(&path_buf, &origin, quality, spec).map_err(|e| e.to_string())?
    } else {
        compress_bytes(&path_buf, &origin, quality, gif_options.as_ref())
            .map_err(|e| e.to_string())?
    };

    if outcome.best_already {
        overwritten
            .lock()
            .map_err(|_| "状态锁定失败".to_string())?
            .remove(&path);

        return Ok(CompressResult {
            path: path.clone(),
            name: file_name(&path),
            origin_size,
            output_size: origin_size,
            ratio: 0.0,
            status: "best".into(),
            message: if resize.is_some() {
                "尺寸未变且已是最佳".into()
            } else {
                "已是最佳效果".into()
            },
            output_path: None,
        });
    }

    let is_overwrite = output_path.is_none();
    let requested_target = output_path.as_deref().unwrap_or(&path);
    let requested_buf = PathBuf::from(requested_target);
    let target_buf = resolve_conflict_path(&requested_buf, is_overwrite);

    if let Some(parent) = target_buf.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).ok();
        }
    }
    fs::write(&target_buf, &outcome.output).map_err(|e| format!("写入失败: {e}"))?;

    // 仅原地覆盖才记录到 overwritten（用于回退对比）
    if is_overwrite {
        overwritten
            .lock()
            .map_err(|_| "状态锁定失败".to_string())?
            .insert(path.clone());
    }

    // 所有压缩结果都存入 compressed map，供 get_preview 对比使用
    {
        let mut map = compressed
            .lock()
            .map_err(|_| "状态锁定失败".to_string())?;
        map.insert(path.clone(), outcome.output.clone());
    }

    let ratio = if origin_size == 0 {
        0.0
    } else {
        ((origin_size as i64 - outcome.output_size as i64) as f64 / origin_size as f64) * 100.0
    };

    Ok(CompressResult {
        path: path.clone(),
        name: file_name(target_buf.to_str().unwrap_or(requested_target)),
        origin_size,
        output_size: outcome.output_size,
        ratio,
        status: "compressed".into(),
        message: if resize.is_some() {
            "已改尺寸".into()
        } else {
            String::new()
        },
        output_path: Some(target_buf.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
async fn collect_images(paths: Vec<String>) -> Result<CollectResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut collected = Vec::new();
        for p in paths {
            collect_from_path(Path::new(&p), &mut collected, false);
        }
        collected.sort();
        collected.dedup();
        CollectResult { paths: collected }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn collect_models(paths: Vec<String>) -> Result<CollectResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut collected = Vec::new();
        for p in paths {
            collect_from_path(Path::new(&p), &mut collected, true);
        }
        collected.sort();
        collected.dedup();
        CollectResult { paths: collected }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn compress_image(
    state: State<'_, SessionState>,
    path: String,
    quality: u8,
    output_path: Option<String>,
    gif_options: Option<GifOptions>,
) -> Result<CompressResult, String> {
    let originals = state.originals.clone();
    let overwritten = state.overwritten.clone();
    let compressed = state.compressed.clone();
    tauri::async_runtime::spawn_blocking(move || {
        compress_image_inner(
            originals,
            overwritten,
            compressed,
            path,
            quality,
            None,
            output_path,
            gif_options,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn resize_image(
    state: State<'_, SessionState>,
    path: String,
    quality: u8,
    mode: String,
    value: f64,
    lock_aspect: bool,
    output_path: Option<String>,
) -> Result<CompressResult, String> {
    let originals = state.originals.clone();
    let overwritten = state.overwritten.clone();
    let compressed = state.compressed.clone();
    let spec = ResizeSpec {
        mode,
        value,
        lock_aspect,
    };
    tauri::async_runtime::spawn_blocking(move || {
        compress_image_inner(
            originals,
            overwritten,
            compressed,
            path,
            quality,
            Some(spec),
            output_path,
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn restore_image(
    state: State<'_, SessionState>,
    path: String,
) -> Result<CompressResult, String> {
    let originals = state.originals.clone();
    let overwritten = state.overwritten.clone();
    let compressed = state.compressed.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let origin = {
            let map = originals.lock().map_err(|_| "状态锁定失败".to_string())?;
            map.get(&path)
                .cloned()
                .ok_or_else(|| "无可回退的原图（仅支持本会话）".to_string())?
        };

        let origin_size = origin.len() as u64;
        fs::write(&path, &origin).map_err(|e| format!("回退写入失败: {e}"))?;
        overwritten
            .lock()
            .map_err(|_| "状态锁定失败".to_string())?
            .remove(&path);
        compressed
            .lock()
            .map_err(|_| "状态锁定失败".to_string())?
            .remove(&path);

        Ok(CompressResult {
            path: path.clone(),
            name: file_name(&path),
            origin_size,
            output_size: origin_size,
            ratio: 0.0,
            status: "restored".into(),
            message: String::new(),
            output_path: None,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_preview(
    state: State<'_, SessionState>,
    path: String,
) -> Result<PreviewResult, String> {
    let originals = state.originals.clone();
    let compressed_map = state.compressed.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let origin = {
            let map = originals.lock().map_err(|_| "状态锁定失败".to_string())?;
            map.get(&path)
                .cloned()
                .ok_or_else(|| "找不到原图预览数据".to_string())?
        };

        // 优先从 compressed map 取压缩后的 bytes（覆盖所有模式：overwrite / newfile / custom）
        // 如果找不到（比如还没压缩过或已清理），fallback 到读磁盘
        let compressed_bytes = {
            let map = compressed_map
                .lock()
                .map_err(|_| "状态锁定失败".to_string())?;
            map.get(&path).cloned()
        }
        .unwrap_or_else(|| fs::read(&path).unwrap_or_else(|_| origin.clone()));

        Ok(PreviewResult {
            mime: mime_of(&path).to_string(),
            origin_base64: BASE64.encode(origin),
            compressed_base64: BASE64.encode(compressed_bytes),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureCompressResult {
    pub data_base64: String,
    pub mime: String,
}

#[tauri::command]
async fn compress_texture(
    data_base64: String,
    target_format: String,
    quality: u8,
    max_size: u32,
) -> Result<TextureCompressResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = BASE64
            .decode(data_base64.as_bytes())
            .map_err(|e| format!("贴图解码失败: {e}"))?;
        let (out, mime) = compress_texture_image(&bytes, &target_format, quality, max_size)
            .map_err(|e| e.to_string())?;
        Ok(TextureCompressResult {
            data_base64: BASE64.encode(out),
            mime,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextureFileResult {
    pub mime: String,
    pub output_size: u64,
}

/// 大贴图走临时文件，避免 base64 IPC 失败导致「几乎没压贴图」
#[tauri::command]
async fn compress_texture_file(
    input_path: String,
    output_path: String,
    target_format: String,
    quality: u8,
    max_size: u32,
) -> Result<TextureFileResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = fs::read(&input_path).map_err(|e| format!("读取贴图失败: {e}"))?;
        let (out, mime) = compress_texture_image(&bytes, &target_format, quality, max_size)
            .map_err(|e| e.to_string())?;
        let output_size = out.len() as u64;
        fs::write(&output_path, &out).map_err(|e| format!("写入贴图失败: {e}"))?;
        Ok(TextureFileResult { mime, output_size })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn clear_session(state: State<'_, SessionState>, paths: Option<Vec<String>>) -> Result<(), String> {
    let mut originals = state
        .originals
        .lock()
        .map_err(|_| "状态锁定失败".to_string())?;
    let mut overwritten = state
        .overwritten
        .lock()
        .map_err(|_| "状态锁定失败".to_string())?;
    let mut compressed = state
        .compressed
        .lock()
        .map_err(|_| "状态锁定失败".to_string())?;

    if let Some(list) = paths {
        for p in list {
            originals.remove(&p);
            overwritten.remove(&p);
            compressed.remove(&p);
        }
    } else {
        originals.clear();
        overwritten.clear();
        compressed.clear();
    }
    Ok(())
}

#[tauri::command]
async fn remove_file(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if Path::new(&path).exists() {
            fs::remove_file(&path).map_err(|e| format!("删除失败: {e}"))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(SessionState::new())
        .invoke_handler(tauri::generate_handler![
            collect_images,
            collect_models,
            compress_image,
            resize_image,
            compress_texture,
            compress_texture_file,
            restore_image,
            get_preview,
            clear_session,
            remove_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
