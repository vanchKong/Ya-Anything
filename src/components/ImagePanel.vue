<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import CompareModal from "./CompareModal.vue";
import {
  loadImageResizeOptions,
  loadQuality,
  saveImageResizeOptions,
  saveQuality,
} from "../lib/settings";
import type { ImageOpMode, ResizeMode } from "../lib/imageOptions";
import {
  createId,
  formatSize,
  type CompressResult,
  type FileItem,
  type PreviewResult,
} from "../types";

const CONCURRENCY = 2;

const props = defineProps<{
  dragActive: boolean;
  dragPoint: { x: number; y: number } | null;
}>();

const panelEl = ref<HTMLElement | null>(null);
const dualDropEl = ref<HTMLElement | null>(null);
const compressZone = ref<HTMLElement | null>(null);
const resizeZone = ref<HTMLElement | null>(null);

const fileList = ref<FileItem[]>([]);
const quality = ref(loadQuality());
const resizeOpts = ref(loadImageResizeOptions());

watch(quality, (v) => saveQuality(v));
watch(resizeOpts, (v) => saveImageResizeOptions(v), { deep: true });

const progressDone = ref(0);
const progressTotal = ref(0);
const anchorIndex = ref(0);
const hoverZone = ref<ImageOpMode | null>(null);
let lastZone: ImageOpMode | null = null;
const currentCompressName = ref("");

const preview = ref<PreviewResult | null>(null);
const previewName = ref("");
const previewOpen = ref(false);

type Job = { path: string; op: ImageOpMode };
const pendingJobs: Job[] = [];
let pumpRunning = false;

const selectedItems = computed(() => fileList.value.filter((f) => f.selected));
const canUndo = computed(() =>
  selectedItems.value.some((f) => f.status === "compressed"),
);
const canClear = computed(() => selectedItems.value.length > 0);
const isEmpty = computed(() => fileList.value.length === 0);
const isWorking = computed(
  () => progressTotal.value > 0 && progressDone.value < progressTotal.value,
);
const progressText = computed(() => {
  if (!isWorking.value) return "";
  if (currentCompressName.value) {
    return `处理中 ${progressDone.value}/${progressTotal.value} · ${currentCompressName.value}`;
  }
  return `处理中 ${progressDone.value}/${progressTotal.value}`;
});

const resizeValueLabel = computed(() => {
  if (resizeOpts.value.mode === "percent") return "比例 %";
  if (resizeOpts.value.mode === "height") return "高度 px";
  return "宽度 px";
});

watch(
  () => [props.dragActive, props.dragPoint] as const,
  async ([active, point]) => {
    if (!active || !point) {
      hoverZone.value = null;
      return;
    }
    await nextTick();
    if (!props.dragActive || !props.dragPoint) return;
    applyHover(props.dragPoint.x, props.dragPoint.y);
  },
);

function setHoverZone(zone: ImageOpMode) {
  hoverZone.value = zone;
  lastZone = zone;
}

function applyHover(x: number, y: number) {
  const zone = hitTest(x, y);
  if (zone) setHoverZone(zone);
  else hoverZone.value = null;
}

function hitTest(x: number, y: number): ImageOpMode | null {
  const c = compressZone.value?.getBoundingClientRect();
  const r = resizeZone.value?.getBoundingClientRect();
  if (c && c.width > 2 && x >= c.left && x <= c.right && y >= c.top && y <= c.bottom) {
    return "compress";
  }
  if (r && r.width > 2 && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
    return "resize";
  }

  const drop = dualDropEl.value?.getBoundingClientRect();
  if (drop && drop.width > 2) {
    const inBand =
      y >= drop.top - 16 &&
      y <= drop.bottom + 16 &&
      x >= drop.left - 16 &&
      x <= drop.right + 16;
    if (inBand) {
      const t = (x - drop.left) / drop.width;
      return t < 0.5 ? "compress" : "resize";
    }
  }

  const panel = panelEl.value?.getBoundingClientRect();
  if (!panel) return null;
  if (
    x < panel.left - 4 ||
    x > panel.right + 4 ||
    y < panel.top - 4 ||
    y > panel.bottom + 4
  ) {
    return null;
  }
  return x < panel.left + panel.width / 2 ? "compress" : "resize";
}

function onZoneDragOver(zone: ImageOpMode, e: DragEvent) {
  e.preventDefault();
  e.stopPropagation();
  setHoverZone(zone);
}

function onGlobalKeydown(e: KeyboardEvent) {
  const meta = e.metaKey || e.ctrlKey;
  if (meta && e.key.toLowerCase() === "a") {
    e.preventDefault();
    selectAll();
  }
}

onMounted(() => window.addEventListener("keydown", onGlobalKeydown));
onUnmounted(() => window.removeEventListener("keydown", onGlobalKeydown));

function selectItem(item: FileItem, event: MouseEvent) {
  event.preventDefault();
  const index = fileList.value.findIndex((f) => f.id === item.id);
  if (index < 0) return;
  const meta = event.metaKey || event.ctrlKey;
  if (meta) {
    item.selected = !item.selected;
    anchorIndex.value = index;
    return;
  }
  if (event.shiftKey) {
    const [from, to] =
      anchorIndex.value < index
        ? [anchorIndex.value, index]
        : [index, anchorIndex.value];
    fileList.value.forEach((f, i) => {
      f.selected = i >= from && i <= to;
    });
    return;
  }
  fileList.value.forEach((f) => {
    f.selected = f.id === item.id;
  });
  anchorIndex.value = index;
}

function selectAll() {
  fileList.value.forEach((f) => {
    f.selected = true;
  });
}

async function pickFiles(mode: ImageOpMode) {
  const selected = await open({
    multiple: true,
    filters: [
      {
        name: "图片",
        extensions: ["jpg", "jpeg", "png", "gif", "webp"],
      },
    ],
  });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  await handlePaths(paths, mode);
}

async function pickFolder(mode: ImageOpMode) {
  const selected = await open({ directory: true, multiple: false });
  if (!selected || Array.isArray(selected)) return;
  await handlePaths([selected], mode);
}

async function handleDrop(
  paths: string[],
  point: { x: number; y: number } | null,
) {
  const zone =
    (point ? hitTest(point.x, point.y) : null) ?? lastZone ?? "compress";
  lastZone = null;
  hoverZone.value = null;
  await handlePaths(paths, zone);
}

async function handlePaths(paths: string[], op: ImageOpMode) {
  if (!paths.length) return;
  await new Promise<void>((r) => requestAnimationFrame(() => r()));

  const { paths: images } = await invoke<{ paths: string[] }>("collect_images", {
    paths,
  });
  if (!images.length) return;

  const existing = new Set(fileList.value.map((f) => f.path));
  const newcomers = images.filter((p) => !existing.has(p));

  if (newcomers.length) {
    fileList.value = fileList.value.concat(
      newcomers.map((path) => ({
        id: createId(),
        path,
        name: path.split(/[/\\]/).pop() || path,
        originSize: 0,
        outputSize: 0,
        ratio: 0,
        status: "pending" as const,
        message: "",
        selected: false,
      })),
    );
    await nextTick();
  }

  const toRun = newcomers.length ? newcomers : images;
  enqueueJobs(toRun.map((path) => ({ path, op })));
}

function enqueueJobs(jobs: Job[]) {
  pendingJobs.push(...jobs);
  progressTotal.value += jobs.length;
  void pumpJobs();
}

async function pumpJobs() {
  if (pumpRunning) return;
  pumpRunning = true;
  try {
    const workers = Array.from({ length: CONCURRENCY }, async () => {
      while (pendingJobs.length) {
        const job = pendingJobs.shift();
        if (!job) break;
        await processOne(job.path, job.op);
        progressDone.value += 1;
        await new Promise<void>((r) => setTimeout(r, 0));
      }
    });
    await Promise.all(workers);
  } finally {
    pumpRunning = false;
    if (pendingJobs.length) {
      void pumpJobs();
      return;
    }
    if (progressDone.value >= progressTotal.value) {
      progressDone.value = 0;
      progressTotal.value = 0;
    }
  }
}

async function processOne(path: string, op: ImageOpMode) {
  const item = fileList.value.find((f) => f.path === path);
  if (!item) return;
  item.status = "loading";
  item.message = "";
  item.lastOp = op;
  currentCompressName.value = item.name;
  try {
    let result: CompressResult;
    if (op === "resize") {
      result = await invoke<CompressResult>("resize_image", {
        path,
        quality: quality.value,
        mode: resizeOpts.value.mode,
        value: resizeOpts.value.value,
        lockAspect: resizeOpts.value.lockAspect,
      });
    } else {
      result = await invoke<CompressResult>("compress_image", {
        path,
        quality: quality.value,
      });
    }
    applyResult(item, result);
  } catch (err) {
    item.status = "error";
    item.message = err instanceof Error ? err.message : String(err);
  } finally {
    if (currentCompressName.value === item.name) {
      currentCompressName.value = "";
    }
  }
}

function applyResult(item: FileItem, result: CompressResult) {
  item.originSize = result.originSize;
  item.outputSize = result.outputSize;
  item.ratio = result.ratio;
  item.message = result.message;
  if (result.status === "best") {
    item.status = "best";
  } else if (result.status === "restored") {
    item.status = "restored";
  } else {
    item.status = "compressed";
  }
}

async function undoSelected() {
  for (const item of selectedItems.value) {
    if (item.status !== "compressed") continue;
    try {
      const result = await invoke<CompressResult>("restore_image", {
        path: item.path,
      });
      applyResult(item, result);
      item.status = "restored";
      item.message = "已回退";
    } catch (err) {
      item.status = "error";
      item.message = err instanceof Error ? err.message : String(err);
    }
  }
}

async function reprocessSelected() {
  const jobs = selectedItems.value
    .filter((item) => item.status !== "loading")
    .map((item) => ({
      path: item.path,
      op: (item.lastOp ?? "compress") as ImageOpMode,
    }));
  if (!jobs.length) return;
  enqueueJobs(jobs);
}

async function clearSelected() {
  const paths = selectedItems.value.map((f) => f.path);
  await invoke("clear_session", { paths });
  fileList.value = fileList.value.filter((f) => !paths.includes(f.path));
}

async function openCompare(item: FileItem) {
  if (item.status !== "compressed" && item.status !== "best") return;
  try {
    const data = await invoke<PreviewResult>("get_preview", { path: item.path });
    preview.value = data;
    previewName.value = item.name;
    previewOpen.value = true;
  } catch (err) {
    item.message = err instanceof Error ? err.message : String(err);
  }
}

function statusText(item: FileItem): string {
  switch (item.status) {
    case "loading":
      return item.lastOp === "resize" ? "改尺寸中…" : "压缩中…";
    case "best":
      return item.message || "已是最佳效果";
    case "compressed":
      if (item.lastOp === "resize") {
        if (item.ratio >= 0) {
          return `改尺寸 · ↓ ${item.ratio.toFixed(1)}%`;
        }
        return `改尺寸 · ↑ ${Math.abs(item.ratio).toFixed(1)}%`;
      }
      return `↓ ${item.ratio.toFixed(1)}%`;
    case "restored":
      return "已回退";
    case "error":
      return item.message || "失败";
    default:
      return "";
  }
}

function onResizeModeChange(e: Event) {
  resizeOpts.value.mode = (e.target as HTMLSelectElement).value as ResizeMode;
}

defineExpose({
  handleDrop,
  progressText,
});
</script>

<template>
  <div ref="panelEl" class="image-panel">
    <section class="options">
      <label class="opt" title="压缩与改尺寸共用；压缩时不改宽高">
        <span>质量</span>
        <select v-model.number="quality">
          <option v-for="n in 9" :key="n" :value="n * 10">{{ n * 10 }}</option>
        </select>
      </label>

      <span class="divider" />

      <label class="opt" title="按宽或按高时默认锁定比例">
        <span>改尺寸</span>
        <select :value="resizeOpts.mode" @change="onResizeModeChange">
          <option value="width">按宽</option>
          <option value="height">按高</option>
          <option value="percent">百分比</option>
        </select>
      </label>

      <label class="opt">
        <span>{{ resizeValueLabel }}</span>
        <input
          v-model.number="resizeOpts.value"
          class="num"
          type="number"
          :min="1"
          :max="resizeOpts.mode === 'percent' ? 400 : 16384"
          step="1"
        />
      </label>

      <label
        v-if="resizeOpts.mode !== 'percent'"
        class="opt check"
        title="关闭后只改选定边，另一边保持原像素（会变形）"
      >
        <input v-model="resizeOpts.lockAspect" type="checkbox" />
        <span>锁比例</span>
      </label>
    </section>

    <section
      ref="dualDropEl"
      v-show="isEmpty || dragActive"
      class="dual-drop"
      :class="{ mini: !isEmpty }"
    >
      <div
        ref="compressZone"
        data-drop-zone="compress"
        class="drop-card"
        :class="{ active: dragActive && hoverZone === 'compress' }"
        @dragenter.prevent="onZoneDragOver('compress', $event)"
        @dragover.prevent="onZoneDragOver('compress', $event)"
      >
        <strong>压缩</strong>
        <p v-if="isEmpty">不改宽高 · 仅压质量</p>
        <div v-if="isEmpty" class="actions">
          <button type="button" @click="pickFiles('compress')">选择文件</button>
          <button type="button" class="ghost" @click="pickFolder('compress')">
            文件夹
          </button>
        </div>
      </div>
      <div
        ref="resizeZone"
        data-drop-zone="resize"
        class="drop-card"
        :class="{ active: dragActive && hoverZone === 'resize' }"
        @dragenter.prevent="onZoneDragOver('resize', $event)"
        @dragover.prevent="onZoneDragOver('resize', $event)"
      >
        <strong>改尺寸</strong>
        <p v-if="isEmpty">按上方规则缩放并编码</p>
        <div v-if="isEmpty" class="actions">
          <button type="button" @click="pickFiles('resize')">选择文件</button>
          <button type="button" class="ghost" @click="pickFolder('resize')">
            文件夹
          </button>
        </div>
      </div>
    </section>

    <section v-if="!isEmpty" class="list-wrap">
      <div class="toolbar">
        <button type="button" :disabled="!canUndo" title="回退选中" @click="undoSelected">
          回退
        </button>
        <button
          type="button"
          :disabled="!canClear"
          title="按各文件上次方式，用当前参数再处理"
          @click="reprocessSelected"
        >
          再处理
        </button>
        <span class="spacer" />
        <button type="button" class="ghost" @click="selectAll">全选</button>
        <button type="button" class="ghost" @click="pickFiles('compress')">添加压缩</button>
        <button type="button" class="ghost" @click="pickFiles('resize')">添加改尺寸</button>
        <button type="button" :disabled="!canClear" class="danger" @click="clearSelected">
          清除
        </button>
      </div>

      <ul class="file-list" @mousedown.prevent>
        <li
          v-for="item in fileList"
          :key="item.id"
          class="file-item"
          :class="{ selected: item.selected, best: item.status === 'best' }"
          @click="selectItem(item, $event)"
          @dblclick="openCompare(item)"
        >
          <div class="name-row">
            <span class="dot" :data-status="item.status" />
            <span class="name" :title="item.path">{{ item.name }}</span>
          </div>
          <span class="size">{{ item.originSize ? formatSize(item.originSize) : "…" }}</span>
          <span class="size">
            {{
              item.status === "compressed" || item.status === "best"
                ? formatSize(item.outputSize)
                : "-"
            }}
          </span>
          <span class="meta">{{ statusText(item) }}</span>
        </li>
      </ul>
      <p class="tip">
        左压缩不改尺寸 · 右改尺寸可批量 · 双击对比 · 动图请用压缩区
      </p>
    </section>

    <CompareModal
      v-if="previewOpen && preview"
      :name="previewName"
      :preview="preview"
      @close="previewOpen = false"
    />
  </div>
</template>

<style scoped>
.image-panel {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.options {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 10px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--line);
  font-size: 12px;
  font-weight: 700;
  align-items: center;
}

.opt {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  cursor: pointer;
}

.opt.check {
  gap: 4px;
}

.divider {
  width: 1px;
  height: 18px;
  background: var(--line);
  margin: 0 2px;
}

.num {
  width: 72px;
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 4px 6px;
  background: var(--control-bg);
  color: var(--text);
  font: inherit;
}

.dual-drop {
  flex: 1;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
  padding: 16px;
  min-height: 0;
}

.dual-drop.mini {
  flex: 0 0 auto;
  gap: 8px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--line);
}

.drop-card {
  border: 1.5px dashed var(--line);
  border-radius: 14px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 16px;
  text-align: center;
  transition:
    border-color 0.15s,
    background 0.15s;
  background: rgba(255, 255, 255, 0.04);
}

.dual-drop.mini .drop-card {
  min-height: 52px;
  padding: 8px;
  border-radius: 10px;
  flex-direction: row;
  gap: 6px;
}

.dual-drop.mini .drop-card p,
.dual-drop.mini .drop-card .actions {
  display: none;
}

.drop-card.active {
  border-color: var(--accent);
  background: var(--drop-active);
}

.drop-card strong {
  font-size: 15px;
}

.drop-card p {
  margin: 0;
  font-size: 12px;
  color: var(--muted);
  font-weight: 600;
}

.actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.list-wrap {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.toolbar {
  display: flex;
  gap: 8px;
  align-items: center;
  padding: 10px 12px;
  border-bottom: 1px solid var(--line);
  flex-wrap: wrap;
}

.spacer {
  flex: 1;
}

.file-list {
  list-style: none;
  margin: 0;
  padding: 0;
  overflow: auto;
  flex: 1;
}

.file-item {
  display: grid;
  grid-template-columns: minmax(0, 1.6fr) 0.8fr 0.8fr 1fr;
  gap: 6px;
  align-items: center;
  padding: 8px 12px;
  font-size: 12px;
  font-weight: 600;
  cursor: default;
  border-bottom: 1px solid var(--line);
}

.file-item:nth-child(odd) {
  background: var(--row-odd);
}

.file-item.selected {
  background: var(--selected);
}

.file-item.best .meta {
  color: var(--best);
}

.name-row {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #b8b8b8;
  flex-shrink: 0;
}

.dot[data-status="loading"] {
  background: var(--accent);
  animation: pulse 0.8s ease-in-out infinite alternate;
}

.dot[data-status="compressed"] {
  background: #2f8f63;
}

.dot[data-status="best"] {
  background: #d4a017;
}

.dot[data-status="error"] {
  background: #c44949;
}

.dot[data-status="restored"] {
  background: #6b7c93;
}

.name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.size,
.meta {
  color: var(--muted);
}

.tip {
  margin: 0;
  padding: 8px 12px;
  font-size: 11px;
  color: var(--muted);
  border-top: 1px solid var(--line);
}

select {
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 4px 8px;
  background: var(--control-bg);
  color: var(--text);
  font: inherit;
}

button {
  border: none;
  border-radius: 9px;
  padding: 7px 12px;
  font: inherit;
  font-size: 12px;
  font-weight: 700;
  color: var(--accent-text);
  background: linear-gradient(180deg, #f2c13a, #e8a820);
  cursor: pointer;
}

button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

button.ghost {
  background: var(--ghost-bg);
  border: 1px solid var(--line);
  color: var(--text);
}

button.danger {
  background: var(--danger-bg);
  color: var(--danger-text);
}

@keyframes pulse {
  from {
    opacity: 1;
  }
  to {
    opacity: 0.55;
  }
}
</style>
