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
