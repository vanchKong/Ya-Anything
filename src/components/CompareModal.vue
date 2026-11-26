<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { parseGIF, decompressFrames } from "gifuct-js";
import type { PreviewResult } from "../types";
import { useI18n } from "../i18n";

const props = defineProps<{
  name: string;
  preview: PreviewResult;
}>();

const emit = defineEmits<{ close: [] }>();
const { t } = useI18n();

const isGif = computed(() => props.preview.mime === "image/gif");
const gifReady = ref(false); // 主路径 gifuct-js 解码完成
// 两种都不行才 side-by-side fallback（极少见：解码抛异常）
const sideBySide = computed(() => isGif.value && !gifReady.value);

const position = ref(0);
const box = ref<HTMLElement | null>(null);
const boxWidth = ref(0);

const originUrl = computed(
  () => `data:${props.preview.mime};base64,${props.preview.originBase64}`,
);
const compressedUrl = computed(
  () => `data:${props.preview.mime};base64,${props.preview.compressedBase64}`,
);
const sameImage = computed(
  () => props.preview.originBase64 === props.preview.compressedBase64,
);

// ---- GIF canvas sync ----
const canvasRef = ref<HTMLCanvasElement | null>(null);
type GifFrame = { canvas: HTMLCanvasElement; delay: number };
let originFrames: GifFrame[] = [];
let compressedFrames: GifFrame[] = [];
let totalDuration = 0;
let rafId = 0;
let startTime = 0;
let canvasCleanup: (() => void) | null = null;

function dataUrlToArrayBuffer(dataUrl: string): ArrayBuffer {
  const comma = dataUrl.indexOf(",");
  if (comma < 0) throw new Error("bad data url");
  const base64 = dataUrl.slice(comma + 1);
  const bin = atob(base64);
  const buf = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);
  return buf.buffer;
}

function decodeGifWithGifuct(dataUrl: string): GifFrame[] {
  const buf = dataUrlToArrayBuffer(dataUrl);
  const gif = parseGIF(buf);
  const frames: any[] = decompressFrames(gif, true); // true = buildPatch 自动合成完整帧
  const out: GifFrame[] = [];
  for (const f of frames) {
    const imgData = f.image as ImageData;
    const c = document.createElement("canvas");
    c.width = imgData.width;
    c.height = imgData.height;
    c.getContext("2d")!.putImageData(imgData, 0, 0);
    // delay 单位：gifuct-js 返回的是 ms（来自 GIF 标准的 delay * 10）
    const delay = Math.max(20, f.delay ?? 40);
    out.push({ canvas: c, delay });
  }
  return out;
}

function setupGifCanvas() {
  gifReady.value = false;
  canvasCleanup?.();
  canvasCleanup = null;
  originFrames = [];
  compressedFrames = [];
  totalDuration = 0;
  if (!isGif.value) return;

  let cancelled = false;
  try {
    originFrames = decodeGifWithGifuct(originUrl.value);
    compressedFrames = decodeGifWithGifuct(compressedUrl.value);
  } catch (e) {
    console.warn("[CompareModal] gifuct-js 解码失败，fallback side-by-side:", e);
    return;
  }

  if (cancelled) return;

  const origTotal = originFrames.reduce((s, f) => s + f.delay, 0) || 1;
  const compTotal = compressedFrames.reduce((s, f) => s + f.delay, 0) || 1;
  totalDuration = Math.max(origTotal, compTotal);

  gifReady.value = true;
  requestAnimationFrame(startLoop);

  canvasCleanup = () => {
    cancelled = true;
    if (rafId) cancelAnimationFrame(rafId);
    originFrames = [];
    compressedFrames = [];
  };
}

function frameAt(frames: GifFrame[], t: number): HTMLCanvasElement | null {
  if (!frames.length) return null;
  let acc = 0;
  for (const f of frames) {
    if (t < acc + f.delay) return f.canvas;
    acc += f.delay;
  }
  return frames[frames.length - 1].canvas;
}

function drawSplit() {
  const canvas = canvasRef.value;
  const boxEl = box.value;
  if (!canvas || !boxEl || originFrames.length === 0 || compressedFrames.length === 0) {
    rafId = requestAnimationFrame(drawSplit);
    return;
  }

  const boxRect = boxEl.getBoundingClientRect();
  const cssW = Math.max(1, Math.floor(boxRect.width));
  const cssH = Math.max(1, Math.floor(boxRect.height));
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  if (canvas.width !== cssW * dpr || canvas.height !== cssH * dpr) {
    canvas.width = cssW * dpr;
    canvas.height = cssH * dpr;
  }
  const ctx = canvas.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, cssW, cssH);

  if (startTime === 0) startTime = performance.now();
  const now = performance.now();
  const elapsed = now - startTime;
  // 防长时间漂移：循环到合理范围
  if (totalDuration > 0 && elapsed > totalDuration * 60) {
    startTime = now - (elapsed % totalDuration);
  }
  const t = totalDuration > 0 ? (now - startTime) % totalDuration : 0;

  const compFrame = frameAt(compressedFrames, t);
  const originFrame = frameAt(originFrames, t);
  if (!compFrame || !originFrame) return;

  const p = position.value / 100;
  const splitX = Math.round(cssW * p);

  // object-fit: contain 绘制辅助
  function drawContain(src: HTMLCanvasElement) {
    const iw = src.width;
    const ih = src.height;
    const scale = Math.min(cssW / iw, cssH / ih);
    const dw = iw * scale;
    const dh = ih * scale;
    const dx = (cssW - dw) / 2;
    const dy = (cssH - dh) / 2;
    ctx.drawImage(src, dx, dy, dw, dh);
  }

  // 底层：压缩后整帧
  drawContain(compFrame);
  // 左半遮罩：原图
  ctx.save();
  ctx.beginPath();
  ctx.rect(0, 0, splitX, cssH);
  ctx.clip();
  drawContain(originFrame);
  ctx.restore();

  // 分割线
  ctx.fillStyle =
    getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() ||
    "#4ade80";
  ctx.fillRect(splitX - 1, 0, 2, cssH);

  rafId = requestAnimationFrame(drawSplit);
}

function startLoop() {
  startTime = 0;
  rafId = requestAnimationFrame(drawSplit);
}

// ---- static image overlay ----
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
}

function measure() {
  boxWidth.value = box.value?.clientWidth ?? 0;
}

function onPointerMove(e: PointerEvent) {
  const el = box.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  if (rect.width <= 0) return;
  const x = e.clientX - rect.left;
  const percent = (x / rect.width) * 100;
  position.value = Math.min(98, Math.max(2, percent));
}

onMounted(() => {
  window.addEventListener("keydown", onKey);
  measure();
  window.addEventListener("resize", measure);
  watch(
    () => [props.preview.mime, props.preview.originBase64, props.preview.compressedBase64],
    () => {
      position.value = 0;
      setupGifCanvas();
    },
    { immediate: true },
  );
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("resize", measure);
  canvasCleanup?.();
});
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="panel" role="dialog" aria-modal="true">
      <header>
        <strong>{{ name }}</strong>
        <button type="button" class="close" @click="emit('close')">
          {{ t("common.close") }}
        </button>
      </header>

      <p v-if="sameImage" class="note">{{ t("common.bestAlready") }}</p>

      <!-- GIF + 静态图：叠层滑块（一起展示） -->
      <div
        v-if="gifReady || !isGif"
        ref="box"
        class="compare"
        @pointermove="onPointerMove"
        @pointerenter="onPointerMove"
        @pointerleave="position = 0"
      >
        <!-- GIF 用单一 canvas 同步画 -->
        <canvas v-if="gifReady" ref="canvasRef" class="gif-canvas" />
        <!-- 静态图用原来的 <img> 叠层 -->
        <template v-else>
          <img class="layer" :src="compressedUrl" :alt="t('compare.after')" />
          <div class="clip" :style="{ width: `${position}%` }">
            <img
              class="layer"
              :src="originUrl"
              :alt="t('compare.before')"
              :style="{ width: boxWidth ? `${boxWidth}px` : '100%' }"
            />
          </div>
        </template>
        <span class="tag left">{{ t("compare.before") }}</span>
        <span class="tag right">{{ t("compare.after") }}</span>
      </div>

      <!-- 极端 fallback：gifuct-js 解码也失败，才 side-by-side -->
      <div v-else-if="sideBySide" class="grid-2 compare">
        <div class="side-wrap">
          <img class="layer" :src="originUrl" :alt="t('compare.before')" />
          <span class="tag">{{ t("compare.before") }}</span>
        </div>
        <div class="side-wrap">
          <img class="layer" :src="compressedUrl" :alt="t('compare.after')" />
          <span class="tag">{{ t("compare.after") }}</span>
        </div>
      </div>

      <p class="hint">{{ t("compare.hint") }}</p>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(20, 24, 22, 0.55);
  display: grid;
  place-items: center;
  padding: 16px;
  z-index: 20;
}

.panel {
  width: min(520px, 100%);
  background: var(--bg);
  color: var(--text);
  border-radius: 14px;
  overflow: hidden;
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.25);
  border: 1px solid var(--line);
}

header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 12px;
  border-bottom: 1px solid var(--line);
  font-size: 13px;
}

.note {
  margin: 0;
  padding: 8px 12px 0;
  font-size: 12px;
  color: var(--best);
  font-weight: 600;
}

.close {
  border: 1px solid var(--line);
  background: var(--control-bg);
  color: var(--text);
  border-radius: 8px;
  padding: 4px 10px;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.compare {
  position: relative;
  aspect-ratio: 4 / 3;
  margin: 12px 12px 6px;
  border-radius: 10px;
  background: #1d2420;
  overflow: hidden;
  user-select: none;
  cursor: ew-resize;
  touch-action: none;
}

.gif-canvas {
  display: block;
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.layer {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  max-width: none;
  pointer-events: none;
}

.clip {
  position: absolute;
  inset: 0 auto 0 0;
  overflow: hidden;
  border-right: 2px solid var(--accent);
  pointer-events: none;
}

.clip .layer {
  height: 100%;
}

.tag {
  position: absolute;
  top: 10px;
  padding: 3px 8px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 700;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  pointer-events: none;
}

.tag.left {
  left: 10px;
}

.tag.right {
  right: 10px;
}

/* 极端 fallback side-by-side */
.grid-2.compare {
  aspect-ratio: auto;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
  padding: 10px 12px 6px;
  background: transparent;
  overflow: visible;
  user-select: auto;
  touch-action: auto;
  margin: 0;
}

.side-wrap {
  position: relative;
  aspect-ratio: 4 / 3;
  border-radius: 10px;
  background: #1d2420;
  overflow: hidden;
}

.hint {
  margin: 0 0 12px;
  padding: 0 12px;
  font-size: 11px;
  color: var(--muted);
  text-align: center;
}
</style>
