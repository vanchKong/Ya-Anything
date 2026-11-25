import { exists } from "@tauri-apps/plugin-fs";
import {
  COMPRESS_PRESETS,
  DEFAULT_GLB_OPTIONS,
  optionsFromPreset,
  type CompressPresetId,
  type DracoPrecision,
  type DracoSpeed,
  type GlbCompressOptions,
  type MeshCompress,
  type MeshoptLevel,
  type TextureFormat,
  type TextureMaxSize,
} from "./glbOptions";
import {
  DEFAULT_IMAGE_RESIZE,
  type ImageResizeOptions,
  type ResizeMode,
} from "./imageOptions";
import type { LocalePreference } from "../i18n/types";

export type CompressKind = "image" | "model";

/** 输出模式：原地覆盖 / 同目录加后缀 / 指定目录 */
export type OutputMode = "overwrite" | "newfile" | "custom";

export const DEFAULT_OUTPUT_MODE: OutputMode = "overwrite";

const KEYS = {
  kind: "ya-compress-kind",
  quality: "ya-quality",
  theme: "ya-theme",
  locale: "ya-locale",
  glbOptions: "ya-glb-options",
  imageResize: "ya-image-resize",
  outputMode: "ya-output-mode",
  outputDir: "ya-output-dir",
} as const;

export function loadCompressKind(): CompressKind {
  const v = localStorage.getItem(KEYS.kind);
  return v === "model" || v === "image" ? v : "image";
}

export function saveCompressKind(kind: CompressKind) {
  localStorage.setItem(KEYS.kind, kind);
}

export function loadQuality(): number {
  const n = Number(localStorage.getItem(KEYS.quality) || 80);
  if (!Number.isFinite(n)) return 80;
  return Math.min(90, Math.max(10, Math.round(n / 10) * 10));
}

export function saveQuality(quality: number) {
  localStorage.setItem(KEYS.quality, String(quality));
}

export function loadLocalePreference(): LocalePreference {
  const v = localStorage.getItem(KEYS.locale);
  if (
    v === "system" ||
    v === "zh" ||
    v === "en" ||
    v === "ja" ||
    v === "ko" ||
    v === "ru"
  ) {
    return v;
  }
  return "system";
}

export function saveLocalePreference(pref: LocalePreference) {
  localStorage.setItem(KEYS.locale, pref);
}

function clampInt(n: number, min: number, max: number, fallback: number) {
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, Math.round(n)));
}

export function loadImageResizeOptions(): ImageResizeOptions {
  try {
    const raw = JSON.parse(localStorage.getItem(KEYS.imageResize) || "{}");
    const mode: ResizeMode =
      raw.mode === "height" || raw.mode === "percent" || raw.mode === "width"
        ? raw.mode
        : DEFAULT_IMAGE_RESIZE.mode;

    // 兼容旧版单一 value 字段
    const legacy = Number(raw.value);
    let width = Number(raw.width);
    let height = Number(raw.height);
    let percent = Number(raw.percent);

    if (!Number.isFinite(width) && Number.isFinite(legacy) && mode === "width") {
      width = legacy;
    }
    if (!Number.isFinite(height) && Number.isFinite(legacy) && mode === "height") {
      height = legacy;
    }
    if (
      !Number.isFinite(percent) &&
      Number.isFinite(legacy) &&
      mode === "percent"
    ) {
      percent = legacy;
    }

    return {
      mode,
      width: clampInt(width, 1, 16384, DEFAULT_IMAGE_RESIZE.width),
      height: clampInt(height, 1, 16384, DEFAULT_IMAGE_RESIZE.height),
      percent: clampInt(percent, 1, 400, DEFAULT_IMAGE_RESIZE.percent),
      lockAspect: raw.lockAspect !== false,
    };
  } catch {
    return { ...DEFAULT_IMAGE_RESIZE };
  }
}

export function saveImageResizeOptions(options: ImageResizeOptions) {
  localStorage.setItem(KEYS.imageResize, JSON.stringify(options));
}

function matchesPreset(
  options: GlbCompressOptions,
  id: Exclude<CompressPresetId, "custom">,
): boolean {
  const base = COMPRESS_PRESETS[id];
  return (Object.keys(base) as (keyof typeof base)[]).every(
    (k) => options[k] === base[k],
  );
}

export function loadGlbOptions(): GlbCompressOptions {
  let raw: Record<string, unknown>;
  try {
    const text = localStorage.getItem(KEYS.glbOptions);
    raw = text ? JSON.parse(text) : {};
  } catch {
    raw = {};
  }

  // 旧 dracoPreset → 整表套用新预设
  if (
    raw.dracoPreset === "max" ||
    raw.dracoPreset === "compact" ||
    raw.dracoPreset === "balanced" ||
    raw.dracoPreset === "fast"
  ) {
    return optionsFromPreset(raw.dracoPreset);
  }

  // 命名预设以代码为准，避免旧 localStorage（如 textureFormat: webp）盖掉预设更新
  if (
    raw.preset === "max" ||
    raw.preset === "compact" ||
    raw.preset === "balanced" ||
    raw.preset === "fast"
  ) {
    return optionsFromPreset(raw.preset);
  }

  const formats = ["keep", "png", "jpeg", "webp"] as const;
  const textureFormat = formats.includes(raw.textureFormat as TextureFormat)
    ? (raw.textureFormat as TextureFormat)
    : DEFAULT_GLB_OPTIONS.textureFormat;

  let meshCompress: MeshCompress = DEFAULT_GLB_OPTIONS.meshCompress;
  if (
    raw.meshCompress === "meshopt" ||
    raw.meshCompress === "draco" ||
    raw.meshCompress === "none"
  ) {
    meshCompress = raw.meshCompress;
  } else if (
    raw.geometryCompress === "meshopt" ||
    raw.geometryCompress === "draco" ||
    raw.geometryCompress === "none"
  ) {
    meshCompress = raw.geometryCompress;
  } else if (raw.draco === true) {
    meshCompress = "draco";
  } else if (raw.draco === false) {
    meshCompress = "none";
  }

  const speedN = Number(raw.dracoSpeed);
  const dracoSpeed = ([0, 3, 5, 8, 10] as const).includes(speedN as DracoSpeed)
    ? (speedN as DracoSpeed)
    : DEFAULT_GLB_OPTIONS.dracoSpeed;

  const dracoPrecision: DracoPrecision =
    raw.dracoPrecision === "low" ||
    raw.dracoPrecision === "medium" ||
    raw.dracoPrecision === "high"
      ? raw.dracoPrecision
      : DEFAULT_GLB_OPTIONS.dracoPrecision;

  const meshoptLevel: MeshoptLevel =
    raw.meshoptLevel === "medium" || raw.meshoptLevel === "high"
      ? raw.meshoptLevel
      : DEFAULT_GLB_OPTIONS.meshoptLevel;

  const merged: GlbCompressOptions = {
    ...DEFAULT_GLB_OPTIONS,
    preset: "custom",
    meshCompress,
    dracoSpeed,
    dracoPrecision,
    meshoptLevel,
    textureFormat,
    textureMaxSize: ([0, 512, 1024, 2048] as const).includes(
      raw.textureMaxSize as TextureMaxSize,
    )
      ? (raw.textureMaxSize as TextureMaxSize)
      : DEFAULT_GLB_OPTIONS.textureMaxSize,
    textureQuality: Math.min(
      95,
      Math.max(
        40,
        Number(raw.textureQuality) || DEFAULT_GLB_OPTIONS.textureQuality,
      ),
    ),
    simplifyRatio: Math.min(
      1,
      Math.max(
        0.05,
        Number(raw.simplifyRatio) || DEFAULT_GLB_OPTIONS.simplifyRatio,
      ),
    ),
    quantize: Boolean(raw.quantize ?? DEFAULT_GLB_OPTIONS.quantize),
    prune: Boolean(raw.prune ?? DEFAULT_GLB_OPTIONS.prune),
    joinMeshes: Boolean(raw.joinMeshes ?? DEFAULT_GLB_OPTIONS.joinMeshes),
    simplify: Boolean(raw.simplify ?? DEFAULT_GLB_OPTIONS.simplify),
    dedup: Boolean(raw.dedup ?? DEFAULT_GLB_OPTIONS.dedup),
    weld: Boolean(raw.weld ?? DEFAULT_GLB_OPTIONS.weld),
    resample: Boolean(raw.resample ?? DEFAULT_GLB_OPTIONS.resample),
    textureEncoder:
      raw.textureEncoder === "rust" || raw.textureEncoder === "canvas"
        ? raw.textureEncoder
        : DEFAULT_GLB_OPTIONS.textureEncoder,
  };

  // 曾被「预设定义变更」打成自定义（典型：旧 webp → 新 keep）时，尽量收回命名预设
  for (const id of ["max", "compact", "balanced", "fast"] as const) {
    const candidate = { ...merged, textureFormat: "keep" as const, preset: id };
    if (matchesPreset(candidate, id)) {
      return optionsFromPreset(id);
    }
  }

  return merged;
}

export function saveGlbOptions(options: GlbCompressOptions) {
  localStorage.setItem(KEYS.glbOptions, JSON.stringify(options));
}

export function loadOutputMode(): OutputMode {
  const v = localStorage.getItem(KEYS.outputMode);
  if (v === "overwrite" || v === "newfile" || v === "custom") return v;
  return DEFAULT_OUTPUT_MODE;
}

export function saveOutputMode(mode: OutputMode) {
  localStorage.setItem(KEYS.outputMode, mode);
}

export function loadOutputDir(): string {
  return localStorage.getItem(KEYS.outputDir) || "";
}

export function saveOutputDir(dir: string) {
  localStorage.setItem(KEYS.outputDir, dir);
}

/** 给新文件生成输出路径：原名.base.ext → 原名.compressed.ext */
export function buildNewFilePath(originalPath: string, suffix = "compressed"): string {
  const sep = originalPath.includes("\\") ? "\\" : "/";
  const lastSep = originalPath.lastIndexOf(sep);
  const dir = lastSep >= 0 ? originalPath.slice(0, lastSep + 1) : "";
  const base = lastSep >= 0 ? originalPath.slice(lastSep + 1) : originalPath;

  const dot = base.lastIndexOf(".");
  if (dot > 0) {
    const name = base.slice(0, dot);
    const ext = base.slice(dot);
    return `${dir}${name}.${suffix}${ext}`;
  }
  return `${dir}${base}.${suffix}`;
}

/** 规范化目录路径：统一分隔符为 /，去掉末尾分隔符，便于比较 */
function normalizeDir(dir: string): string {
  return dir.replace(/\\/g, "/").replace(/\/+$/, "");
}

/** 提取文件所在目录（已规范化） */
function sourceDirOf(filePath: string): string {
  const idx = Math.max(filePath.lastIndexOf("/"), filePath.lastIndexOf("\\"));
  const dir = idx >= 0 ? filePath.slice(0, idx) : "";
  return normalizeDir(dir);
}

/**
 * 将原路径映射到输出目录。
 * 聪明地处理"输出目录 == 源文件目录"的情况——此时自动退化为 newfile 行为（加后缀），
 * 避免 custom 模式意外覆盖原文件。
 */
export function buildCustomDirPath(originalPath: string, outputDir: string): string {
  if (!outputDir) return originalPath;

  const nameIdx = Math.max(
    originalPath.lastIndexOf("/"),
    originalPath.lastIndexOf("\\"),
  );
  const name = nameIdx >= 0 ? originalPath.slice(nameIdx + 1) : originalPath;

  // 如果用户选的目录正好是源文件所在目录，自动加后缀避免覆盖
  if (sourceDirOf(originalPath) === normalizeDir(outputDir)) {
    return buildNewFilePath(originalPath);
  }

  // 确保 outputDir 结尾有分隔符
  const dirSep = outputDir.includes("\\") ? "\\" : "/";
  const dir = outputDir.endsWith(dirSep) ? outputDir : `${outputDir}${dirSep}`;
  return `${dir}${name}`;
}

/** 根据 OutputMode 计算输出路径 */
export function resolveOutputPath(
  originalPath: string,
  mode: OutputMode,
  customDir: string,
): string {
  if (mode === "overwrite") return originalPath;
  if (mode === "newfile") return buildNewFilePath(originalPath);
  return buildCustomDirPath(originalPath, customDir);
}

/**
 * 给可能冲突的目标路径追加编号后缀：
 *   a.png → a.2.png → a.3.png …
 * 前端 ModelPanel 在 writeFile 前、Image 等后端处理时由 Rust 侧兜底。
 * 仅文件路径（not overwrite 模式）需要调用。
 */
export async function resolveConflictPath(
  targetPath: string,
  isOverwrite: boolean,
): Promise<string> {
  if (isOverwrite) return targetPath;
  if (!(await exists(targetPath))) return targetPath;

  const sep = targetPath.includes("\\") ? "\\" : "/";
  const lastSep = targetPath.lastIndexOf(sep);
  const dir = lastSep >= 0 ? targetPath.slice(0, lastSep + 1) : "";
  const base = lastSep >= 0 ? targetPath.slice(lastSep + 1) : targetPath;

  const dot = base.lastIndexOf(".");
  let stem: string;
  let ext: string;
  if (dot > 0) {
    stem = base.slice(0, dot);
    ext = base.slice(dot);
  } else {
    stem = base;
    ext = "";
  }

  for (let n = 2; n <= 9999; n++) {
    const candidate = `${dir}${stem}.${n}${ext}`;
    if (!(await exists(candidate))) return candidate;
  }
  return `${dir}${stem}.dup${ext}`;
}

export { KEYS as SETTINGS_KEYS };
