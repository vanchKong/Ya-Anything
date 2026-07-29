/** 压缩预设：选档位会整表填充；改任意项后变为自定义 */
export type CompressPresetId =
  | "max"
  | "compact"
  | "balanced"
  | "fast"
  | "custom";

export type TextureFormat = "keep" | "png" | "jpeg" | "webp";
export type TextureMaxSize = 0 | 512 | 1024 | 2048;
/**
 * 贴图编码器：
 * - canvas：浏览器编码（接近初版：更快，同质量下体积更大，约 80MB→10MB 量级）
 * - rust：libwebp（更慢更小，可到数 MB）
 */
export type TextureEncoder = "canvas" | "rust";

/** 网格压缩算法（无 / Draco / Meshopt） */
export type MeshCompress = "none" | "draco" | "meshopt";

/** Draco 编码速度 0=最慢通常最小，10=最快 */
export type DracoSpeed = 0 | 3 | 5 | 8 | 10;

/** Draco 几何量化精度（位数打包） */
export type DracoPrecision = "low" | "medium" | "high";

export type MeshoptLevel = "medium" | "high";

export interface DracoBitDepth {
  quantizePosition: number;
  quantizeNormal: number;
  quantizeTexcoord: number;
  quantizeColor: number;
  quantizeGeneric: number;
}

export const DRACO_PRECISION_BITS: Record<DracoPrecision, DracoBitDepth> = {
  low: {
    quantizePosition: 11,
    quantizeNormal: 8,
    quantizeTexcoord: 10,
    quantizeColor: 8,
    quantizeGeneric: 10,
  },
  medium: {
    quantizePosition: 14,
    quantizeNormal: 10,
    quantizeTexcoord: 12,
    quantizeColor: 8,
    quantizeGeneric: 12,
  },
  high: {
    quantizePosition: 16,
    quantizeNormal: 12,
    quantizeTexcoord: 14,
    quantizeColor: 10,
    quantizeGeneric: 14,
  },
};

export interface GlbCompressOptions {
  preset: CompressPresetId;

  /** 去重 accessor/纹理（几乎无损，通常很快） */
  dedup: boolean;
  /** 焊接重合顶点——大网格可能很慢 */
  weld: boolean;
  /** 动画关键帧重采样 */
  resample: boolean;
  prune: boolean;
  joinMeshes: boolean;
  simplify: boolean;
  simplifyRatio: number;

  meshCompress: MeshCompress;
  /** 无网格压缩、或 Meshopt 前置时使用 */
  quantize: boolean;
  dracoSpeed: DracoSpeed;
  dracoPrecision: DracoPrecision;
  meshoptLevel: MeshoptLevel;

  textureFormat: TextureFormat;
  textureMaxSize: TextureMaxSize;
  /** 贴图 JPG/WebP 质量 40–95；仅 format≠保持 时生效 */
  textureQuality: number;
  /** 贴图编码实现：canvas≈初版速度/体积，rust=更强压缩 */
  textureEncoder: TextureEncoder;
}

type PresetOptions = Omit<GlbCompressOptions, "preset">;

/**
 * 档位由狠到轻：极致 → 轻量(用户基准) → 均衡 → 快速保真
 * 仅极致开启强力贴图编码。
 */
const PRESET_MAX: PresetOptions = {
  dedup: true,
  weld: true,
  resample: true,
  prune: true,
  joinMeshes: false,
  simplify: false,
  simplifyRatio: 0.5,
  meshCompress: "draco",
  quantize: false,
  dracoSpeed: 3,
  dracoPrecision: "low",
  meshoptLevel: "medium",
  textureFormat: "webp",
  textureMaxSize: 512,
  textureQuality: 65,
  textureEncoder: "rust",
};

/** 用户基准：比均衡更狠一档，仍用快速贴图编码 */
const PRESET_COMPACT: PresetOptions = {
  dedup: true,
  weld: true,
  resample: true,
  prune: true,
  joinMeshes: false,
  simplify: false,
  simplifyRatio: 0.5,
  meshCompress: "draco",
  quantize: false,
  dracoSpeed: 5,
  dracoPrecision: "low",
  meshoptLevel: "medium",
  textureFormat: "webp",
  textureMaxSize: 512,
  textureQuality: 80,
  textureEncoder: "canvas",
};

const PRESET_BALANCED: PresetOptions = {
  dedup: true,
  weld: true,
  resample: true,
  prune: true,
  joinMeshes: false,
  simplify: false,
  simplifyRatio: 0.5,
  meshCompress: "draco",
  quantize: false,
  dracoSpeed: 5,
  dracoPrecision: "medium",
  meshoptLevel: "medium",
  textureFormat: "webp",
  textureMaxSize: 1024,
  textureQuality: 85,
  textureEncoder: "canvas",
};

const PRESET_FAST: PresetOptions = {
  dedup: true,
  weld: false,
  resample: true,
  prune: true,
  joinMeshes: false,
  simplify: false,
  simplifyRatio: 0.5,
  meshCompress: "draco",
  quantize: false,
  dracoSpeed: 8,
  dracoPrecision: "high",
  meshoptLevel: "medium",
  textureFormat: "webp",
  textureMaxSize: 2048,
  textureQuality: 90,
  textureEncoder: "canvas",
};

export const COMPRESS_PRESETS: Record<
  Exclude<CompressPresetId, "custom">,
  PresetOptions
> = {
  max: PRESET_MAX,
  compact: PRESET_COMPACT,
  balanced: PRESET_BALANCED,
  fast: PRESET_FAST,
};

export const PRESET_LABELS: Record<CompressPresetId, string> = {
  max: "极致小体积",
  compact: "轻量",
  balanced: "均衡",
  fast: "快速保真",
  custom: "自定义",
};

export function optionsFromPreset(
  id: Exclude<CompressPresetId, "custom">,
): GlbCompressOptions {
  return { preset: id, ...COMPRESS_PRESETS[id] };
}

/** 是否会跑贴图转码（体积与耗时的最大头） */
export function willReencodeTextures(options: GlbCompressOptions): boolean {
  return options.textureFormat !== "keep" || options.textureMaxSize > 0;
}

export const DEFAULT_GLB_OPTIONS: GlbCompressOptions =
  optionsFromPreset("balanced");
