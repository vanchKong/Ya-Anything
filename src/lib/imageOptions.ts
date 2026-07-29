/** 图片处理：压缩（不改尺寸）与改尺寸 */

export type ImageOpMode = "compress" | "resize";

export type ResizeMode = "width" | "height" | "percent";

export interface ImageResizeOptions {
  mode: ResizeMode;
  /** 宽/高像素，或百分比 1–400 */
  value: number;
  /** 按宽/高时默认锁比例 */
  lockAspect: boolean;
}

export const DEFAULT_IMAGE_RESIZE: ImageResizeOptions = {
  mode: "width",
  value: 1280,
  lockAspect: true,
};

export const RESIZE_MODE_LABELS: Record<ResizeMode, string> = {
  width: "按宽",
  height: "按高",
  percent: "百分比",
};
