import { FileUtils, ImageUtils, TextureChannel, WebIO } from "@gltf-transform/core";
import { ALL_EXTENSIONS, EXTTextureWebP } from "@gltf-transform/extensions";
import {
  dedup,
  draco,
  flatten,
  getTextureChannelMask,
  join,
  listTextureSlots,
  meshopt,
  prune,
  quantize,
  reorder,
  resample,
  simplify,
  weld,
} from "@gltf-transform/functions";
import { MeshoptDecoder, MeshoptEncoder, MeshoptSimplifier } from "meshoptimizer";
import { invoke } from "@tauri-apps/api/core";
import { join as pathJoin, tempDir } from "@tauri-apps/api/path";
import { remove, writeFile, readFile } from "@tauri-apps/plugin-fs";
import type { Document } from "@gltf-transform/core";
import {
  DRACO_PRECISION_BITS,
  type GlbCompressOptions,
} from "./glbOptions";

export type { GlbCompressOptions, TextureFormat } from "./glbOptions";
export { DEFAULT_GLB_OPTIONS } from "./glbOptions";

type DracoFactory = (config?: {
  locateFile?: (file: string) => string;
}) => Promise<unknown>;

type Draco3dGltf = {
  createEncoderModule: DracoFactory;
  createDecoderModule: DracoFactory;
};

let ioPromise: Promise<WebIO> | null = null;
let simplifierReady: Promise<void> | null = null;
let meshoptReady: Promise<void> | null = null;

function locateDracoFile(file: string): string {
  return `/draco/${file}`;
}

async function loadDraco3d(): Promise<Draco3dGltf> {
  const mod = (await import("draco3dgltf")) as unknown as
    | Draco3dGltf
    | { default: Draco3dGltf };
  if ("createEncoderModule" in mod) return mod;
  return (mod as { default: Draco3dGltf }).default;
}

async function ensureSimplifier() {
  if (!simplifierReady) simplifierReady = MeshoptSimplifier.ready;
  await simplifierReady;
}

async function ensureMeshopt() {
  if (!meshoptReady) {
    meshoptReady = Promise.all([
      MeshoptEncoder.ready,
      MeshoptDecoder.ready,
    ]).then(() => undefined);
  }
  await meshoptReady;
}

async function getIO(): Promise<WebIO> {
  if (!ioPromise) {
    ioPromise = (async () => {
      await ensureMeshopt();
      const draco3d = await loadDraco3d();
      const [encoder, decoder] = await Promise.all([
        draco3d.createEncoderModule({ locateFile: locateDracoFile }),
        draco3d.createDecoderModule({ locateFile: locateDracoFile }),
      ]);
      return new WebIO()
        .registerExtensions(ALL_EXTENSIONS)
        .registerDependencies({
          "draco3d.encoder": encoder,
          "draco3d.decoder": decoder,
          "meshopt.encoder": MeshoptEncoder,
          "meshopt.decoder": MeshoptDecoder,
        });
    })();
  }
  return ioPromise;
}

/** 各 primitive 的顶点数之和（等价于查看器/上传时统计的顶点数） */
function countVertices(document: Document): number {
  let total = 0;
  for (const mesh of document.getRoot().listMeshes()) {
    for (const prim of mesh.listPrimitives()) {
      total += prim.getAttribute("POSITION")?.getCount() ?? 0;
    }
  }
  return total;
}

function isGlbBinary(input: Uint8Array): boolean {
  if (input.byteLength < 4) return false;
  return (
    input[0] === 0x67 &&
    input[1] === 0x6c &&
    input[2] === 0x54 &&
    input[3] === 0x46
  );
}

async function encodeTextureCanvas(
  srcImage: Uint8Array,
  srcMimeType: string,
  targetFormat: "webp" | "jpeg" | "png",
  quality: number,
  maxSize: number,
): Promise<{ bytes: Uint8Array; mime: string }> {
  const dstMime =
    targetFormat === "jpeg"
      ? "image/jpeg"
      : targetFormat === "webp"
        ? "image/webp"
        : "image/png";

  const blob = new Blob([srcImage as BlobPart], { type: srcMimeType });
  const bitmap = await createImageBitmap(blob);
  try {
    let width = bitmap.width;
    let height = bitmap.height;
    if (maxSize > 0) {
      const scale = Math.min(1, maxSize / Math.max(width, height));
      width = Math.max(1, Math.round(width * scale));
      height = Math.max(1, Math.round(height * scale));
    }

    const canvas =
      typeof OffscreenCanvas !== "undefined"
        ? new OffscreenCanvas(width, height)
        : Object.assign(document.createElement("canvas"), { width, height });

    const ctx = canvas.getContext("2d") as
      | OffscreenCanvasRenderingContext2D
      | CanvasRenderingContext2D
      | null;
    if (!ctx) throw new Error("无法创建画布");
    ctx.drawImage(bitmap, 0, 0, width, height);

    const q = Math.min(0.95, Math.max(0.4, quality / 100));
    const outBlob =
      "convertToBlob" in canvas
        ? await (canvas as OffscreenCanvas).convertToBlob({
            type: dstMime,
            quality: q,
          })
        : await new Promise<Blob>((resolve, reject) => {
            (canvas as HTMLCanvasElement).toBlob(
              (b) => (b ? resolve(b) : reject(new Error("贴图编码失败"))),
              dstMime,
              q,
            );
          });

    return {
      bytes: new Uint8Array(await outBlob.arrayBuffer()),
      mime: dstMime,
    };
  } finally {
    bitmap.close();
  }
}

/** Rust libwebp：更慢更小（临时文件 IPC） */
async function encodeTextureRust(
  srcImage: Uint8Array,
  targetFormat: "webp" | "jpeg" | "png",
  quality: number,
  maxSize: number,
): Promise<{ bytes: Uint8Array; mime: string }> {
  const dir = await tempDir();
  const id = `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
  const inputPath = await pathJoin(dir, `ya-tex-in-${id}`);
  const outputPath = await pathJoin(dir, `ya-tex-out-${id}`);

  try {
    await writeFile(inputPath, srcImage);
    const result = await invoke<{ mime: string; outputSize: number }>(
      "compress_texture_file",
      {
        inputPath,
        outputPath,
        targetFormat,
        quality,
        maxSize,
      },
    );
    const bytes = await readFile(outputPath);
    return { bytes, mime: result.mime };
  } finally {
    await Promise.allSettled([remove(inputPath), remove(outputPath)]);
  }
}

async function encodeTexture(
  srcImage: Uint8Array,
  srcMimeType: string,
  targetFormat: "webp" | "jpeg" | "png",
  quality: number,
  maxSize: number,
  encoder: "canvas" | "rust",
): Promise<{ bytes: Uint8Array; mime: string }> {
  if (encoder === "rust") {
    return encodeTextureRust(srcImage, targetFormat, quality, maxSize);
  }
  return encodeTextureCanvas(
    srcImage,
    srcMimeType,
    targetFormat,
    quality,
    maxSize,
  );
}

export type CompressStage =
  | "read"
  | "cleanup"
  | "texture"
  | "geometry"
  | "write";

export type CompressProgress = (info: {
  stage: CompressStage;
  message: string;
  current?: number;
  total?: number;
}) => void;

async function compressTextures(
  document: Document,
  options: GlbCompressOptions,
  onProgress?: CompressProgress,
): Promise<{ done: number; failed: number }> {
  const needPass =
    options.textureFormat !== "keep" || options.textureMaxSize > 0;
  if (!needPass) return { done: 0, failed: 0 };

  const quality = Math.min(95, Math.max(40, options.textureQuality || 70));
  const format = options.textureFormat;
  const maxSize = options.textureMaxSize > 0 ? options.textureMaxSize : 0;
  const encoder = options.textureEncoder === "rust" ? "rust" : "canvas";

  const textures = document.getRoot().listTextures();
  const jobs: {
    texture: (typeof textures)[number];
    srcMime: string;
    srcImage: Uint8Array;
    target: "webp" | "jpeg" | "png";
    q: number;
  }[] = [];

  for (const texture of textures) {
    const srcMime = texture.getMimeType();
    if (!/^image\/(png|jpeg|jpg|webp|bmp|gif)$/i.test(srcMime)) continue;
    const srcImage = texture.getImage();
    if (!srcImage) continue;

    const slots = listTextureSlots(texture);
    const isNormal =
      slots.length > 0 && slots.every((s) => s === "normalTexture");

    let target: "webp" | "jpeg" | "png";
    let q = quality;
    if (format === "keep") {
      if (srcMime.includes("jpeg") || srcMime.includes("jpg")) target = "jpeg";
      else if (srcMime.includes("webp")) target = "webp";
      else target = "png";
    } else if (format === "jpeg") {
      target =
        getTextureChannelMask(texture) & TextureChannel.A ? "png" : "jpeg";
    } else if (format === "webp") {
      target = "webp";
      if (isNormal) q = Math.min(95, Math.max(quality, 80));
    } else {
      target = "png";
    }

    if (format === "keep" && maxSize <= 0) continue;
    jobs.push({ texture, srcMime, srcImage, target, q });
  }

  let done = 0;
  let failed = 0;
  const concurrency = encoder === "canvas" ? 4 : 2;

  for (let i = 0; i < jobs.length; i += concurrency) {
    const batch = jobs.slice(i, i + concurrency);
    onProgress?.({
      stage: "texture",
      message: `压缩贴图 ${Math.min(i + batch.length, jobs.length)}/${jobs.length}（${encoder === "rust" ? "强力" : "快速"}）`,
      current: Math.min(i + batch.length, jobs.length),
      total: jobs.length,
    });
    await new Promise<void>((r) => setTimeout(r, 0));

    await Promise.all(
      batch.map(async ({ texture, srcMime, srcImage, target, q }) => {
        try {
          const { bytes: dstImage, mime: dstMime } = await encodeTexture(
            srcImage,
            srcMime,
            target,
            q,
            maxSize,
            encoder,
          );
          if (
            dstImage.byteLength >= srcImage.byteLength &&
            srcMime === dstMime
          ) {
            return;
          }
          const srcURI = texture.getURI();
          if (srcMime !== dstMime && srcURI) {
            const srcExt = FileUtils.extension(srcURI);
            const dstExt = ImageUtils.mimeTypeToExtension(dstMime);
            texture.setURI(
              srcURI.replace(new RegExp(`\\.${srcExt}$`), `.${dstExt}`),
            );
          }
          texture.setImage(dstImage).setMimeType(dstMime);
          done += 1;
        } catch (err) {
          failed += 1;
          console.warn("[glb] texture compress failed", err);
        }
      }),
    );
  }

  const webpExt = document.createExtension(EXTTextureWebP);
  if (textures.some((t) => t.getMimeType() === "image/webp")) {
    webpExt.setRequired(true);
  } else {
    webpExt.dispose();
  }

  return { done, failed };
}

/**
 * 按 UI 选项原样执行，不做档位覆盖。
 * 预设只负责填充 options，真正生效的永远是传入的字段。
 */
export async function compressGlbBytes(
  input: Uint8Array,
  options: GlbCompressOptions,
  onProgress?: CompressProgress,
): Promise<Uint8Array> {
  if (!isGlbBinary(input)) {
    throw new Error("暂仅支持独立 .glb（资源内嵌）。请先导出为 GLB 再压缩");
  }

  const report: CompressProgress = (info) => onProgress?.(info);

  report({ stage: "read", message: "读取模型…" });
  const io = await getIO();
  const document = await io.readBinary(input);

  const transforms = [];
  // 简化前后统计顶点数，便于确认「缩减顶点」到底起没起作用
  const srcVertexCount = options.simplify ? countVertices(document) : 0;
  if (options.dedup) transforms.push(dedup());
  if (options.weld) transforms.push(weld());
  if (options.resample) transforms.push(resample());

  if (options.prune) transforms.push(prune());
  if (options.joinMeshes) {
    transforms.push(flatten());
    transforms.push(join({ keepNamed: true }));
  }
  if (options.simplify) {
    await ensureSimplifier();
    const ratio = Math.min(1, Math.max(0.05, options.simplifyRatio || 0.5));
    transforms.push(
      simplify({
        simplifier: MeshoptSimplifier,
        ratio,
        error: 0.001,
      }),
    );
  }

  if (transforms.length) {
    report({ stage: "cleanup", message: "清理网格与冗余…" });
    await new Promise<void>((r) => setTimeout(r, 0));
    await document.transform(...transforms);
  }

  if (options.simplify) {
    const dstVertexCount = countVertices(document);
    report({
      stage: "cleanup",
      message: `顶点 ${srcVertexCount.toLocaleString()} → ${dstVertexCount.toLocaleString()}`,
    });
  }

  // 贴图转码是耗时与体积的主因：格式≠保持 或 需要缩放时才会跑
  const needTex =
    options.textureFormat !== "keep" || options.textureMaxSize > 0;
  if (needTex) {
    report({
      stage: "texture",
      message: `转码贴图（${options.textureFormat} · ${options.textureEncoder === "rust" ? "强力libwebp" : "快速浏览器"}）…`,
    });
    const tex = await compressTextures(document, options, report);
    if (tex.failed > 0 && tex.done === 0) {
      throw new Error(
        `贴图压缩全部失败（${tex.failed} 张）。请检查权限后重试`,
      );
    }
  } else {
    report({ stage: "texture", message: "跳过贴图（保持原格式且不缩放）" });
  }

  const post = [];
  const mesh = options.meshCompress;
  const bits = DRACO_PRECISION_BITS[options.dracoPrecision];

  // 独立顶点量化：无网格压缩时，或 Meshopt 前
  if (options.quantize && (mesh === "none" || mesh === "meshopt")) {
    post.push(
      quantize({
        quantizePosition: bits.quantizePosition,
        quantizeNormal: bits.quantizeNormal,
        quantizeTexcoord: bits.quantizeTexcoord,
        quantizeColor: bits.quantizeColor,
        quantizeGeneric: bits.quantizeGeneric,
      }),
    );
  }

  if (mesh === "meshopt") {
    await ensureMeshopt();
    post.push(reorder({ encoder: MeshoptEncoder }));
    post.push(
      meshopt({
        encoder: MeshoptEncoder,
        level: options.meshoptLevel,
      }),
    );
    report({ stage: "geometry", message: "Meshopt 压缩…" });
  } else if (mesh === "draco") {
    const speed = options.dracoSpeed;
    post.push(
      draco({
        method: "edgebreaker",
        encodeSpeed: speed,
        decodeSpeed: Math.min(10, speed + 2),
        ...bits,
      }),
    );
    report({
      stage: "geometry",
      message: `Draco 压缩（速度 ${speed}）…`,
    });
  }

  if (post.length) {
    await new Promise<void>((r) => setTimeout(r, 0));
    await document.transform(...post);
  }

  report({ stage: "write", message: "写入 GLB…" });
  await new Promise<void>((r) => setTimeout(r, 0));
  return io.writeBinary(document);
}
