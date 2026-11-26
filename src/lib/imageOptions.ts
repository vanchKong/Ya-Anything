/** 图片处理：压缩（不改尺寸）与改尺寸 */

export type ImageOpMode = "compress" | "resize";

export type ResizeMode = "width" | "height" | "percent";

export interface ImageResizeOptions {
  mode: ResizeMode;
  /** 按宽目标像素 */
  width: number;
  /** 按高目标像素 */
  height: number;
  /** 百分比 1–400，默认 100 */
  percent: number;
  /** 按宽/高时默认锁比例 */
  lockAspect: boolean;
}

export const DEFAULT_IMAGE_RESIZE: ImageResizeOptions = {
  mode: "width",
  width: 1280,
  height: 1280,
  percent: 100,
  lockAspect: true,
};

/** 按当前模式取出后端需要的 value */
export function resizeValueForMode(options: ImageResizeOptions): number {
  if (options.mode === "percent") return options.percent;
  if (options.mode === "height") return options.height;
  return options.width;
}

/** GIF 抖动算法 */
export type GifDither = "none" | "bayer" | "floyd_steinberg";

export interface GifCompressOptions {
  /** 帧率上限，0 = 保持原始帧率 */
  maxFps: number;
  /** 调色板颜色数 2–256 */
  maxColors: number;
  dither: GifDither;
  /** 最大宽度（像素），0 = 不缩放 */
  maxWidth: number;
}

/** 与 mediaCompressHub 一致的默认值：12fps / 128 色 / Bayer */
export const DEFAULT_GIF_OPTIONS: GifCompressOptions = {
  maxFps: 12,
  maxColors: 128,
  dither: "bayer",
  maxWidth: 0,
};

export type GifPresetId =
  | "default"
  | "light"
  | "balanced"
  | "aggressive"
  | "custom";

export const GIF_PRESETS: Record<
  Exclude<GifPresetId, "custom">,
  GifCompressOptions
> = {
  default: DEFAULT_GIF_OPTIONS,
  light: { maxFps: 20, maxColors: 220, dither: "none", maxWidth: 0 },
  balanced: { maxFps: 15, maxColors: 144, dither: "none", maxWidth: 1280 },
  aggressive: { maxFps: 8, maxColors: 64, dither: "none", maxWidth: 720 },
};

export function gifPresetOf(options: GifCompressOptions): GifPresetId {
  for (const id of ["default", "light", "balanced", "aggressive"] as const) {
    const p = GIF_PRESETS[id];
    if (
      p.maxFps === options.maxFps &&
      p.maxColors === options.maxColors &&
      p.dither === options.dither &&
      p.maxWidth === options.maxWidth
    ) {
      return id;
    }
  }
  return "custom";
}
