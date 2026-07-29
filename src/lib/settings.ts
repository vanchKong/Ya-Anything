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

export type CompressKind = "image" | "model";

const KEYS = {
  kind: "ya-compress-kind",
  quality: "ya-quality",
  theme: "ya-theme",
  glbOptions: "ya-glb-options",
  imageResize: "ya-image-resize",
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

export function loadImageResizeOptions(): ImageResizeOptions {
  try {
    const raw = JSON.parse(localStorage.getItem(KEYS.imageResize) || "{}");
    const mode: ResizeMode =
      raw.mode === "height" || raw.mode === "percent" || raw.mode === "width"
        ? raw.mode
        : DEFAULT_IMAGE_RESIZE.mode;
    const value = Number(raw.value);
    return {
      mode,
      value: Number.isFinite(value)
        ? Math.min(16384, Math.max(1, Math.round(value)))
        : DEFAULT_IMAGE_RESIZE.value,
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

  let preset: CompressPresetId =
    raw.preset === "max" ||
    raw.preset === "compact" ||
    raw.preset === "balanced" ||
    raw.preset === "fast" ||
    raw.preset === "custom"
      ? raw.preset
      : "custom";

  const merged: GlbCompressOptions = {
    ...DEFAULT_GLB_OPTIONS,
    preset,
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

  if (merged.preset !== "custom" && !matchesPreset(merged, merged.preset)) {
    merged.preset = "custom";
  }

  return merged;
}

export function saveGlbOptions(options: GlbCompressOptions) {
  localStorage.setItem(KEYS.glbOptions, JSON.stringify(options));
}

export { KEYS as SETTINGS_KEYS };
