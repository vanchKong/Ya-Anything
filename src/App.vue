<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import CompareModal from "./components/CompareModal.vue";
import ModelPanel from "./components/ModelPanel.vue";
import SplashScreen from "./components/SplashScreen.vue";
import { useTheme, type ThemeMode } from "./composables/useTheme";
import logo from "./assets/logo.png";
import {
  loadCompressKind,
  loadQuality,
  saveCompressKind,
  saveQuality,
  type CompressKind,
} from "./lib/settings";
import {
  createId,
  formatSize,
  type CompressResult,
  type FileItem,
  type PreviewResult,
} from "./types";

const CONCURRENCY = 2;

const { mode: themeMode } = useTheme();

const compressKind = ref<CompressKind>(loadCompressKind());
const modelPanelRef = ref<InstanceType<typeof ModelPanel> | null>(null);
const dragPoint = ref<{ x: number; y: number } | null>(null);

const fileList = ref<FileItem[]>([]);
const dragStatus = ref(false);
const quality = ref(loadQuality());
const preview = ref<PreviewResult | null>(null);
const previewName = ref("");
const previewOpen = ref(false);
const showSplash = ref(true);
const progressDone = ref(0);
const progressTotal = ref(0);
const anchorIndex = ref(0);
const currentCompressName = ref("");

let unlistenDrag: (() => void) | null = null;
const pendingPaths: string[] = [];
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
  if (compressKind.value === "model") {
    return modelPanelRef.value?.progressText || "";
  }
  if (!isWorking.value) return "";
  if (currentCompressName.value) {
    return `压缩中 ${progressDone.value}/${progressTotal.value} · ${currentCompressName.value}`;
  }
  return `压缩中 ${progressDone.value}/${progressTotal.value}`;
});

watch(quality, (v) => {
  saveQuality(v);
});

watch(compressKind, (v) => {
  saveCompressKind(v);
  dragStatus.value = false;
  dragPoint.value = null;
});

onMounted(async () => {
  const webview = getCurrentWebview();
  const windowRef = getCurrentWindow();

  /**
   * DragDropEvent.position 为相对 webview 的物理像素。
   * 部分环境下实际已是 CSS 像素；用「是否落在窗口宽高内」判断要不要除 scale。
   * 不再用 cursorPosition（拖文件时易偏移，曾导致左右反了）。
   */
  async function toClientPoint(position: { x: number; y: number }) {
    const scale = await windowRef.scaleFactor();
    const w = window.innerWidth;
    const h = window.innerHeight;
    const raw = { x: position.x, y: position.y };
    const logical = { x: position.x / scale, y: position.y / scale };
    const inView = (p: { x: number; y: number }) =>
      p.x >= -8 && p.y >= -8 && p.x <= w + 8 && p.y <= h + 8;

    // Retina：raw 已在 CSS 视口内 → 已是逻辑像素，不要再除
    if (scale > 1.05 && inView(raw)) return raw;
    if (inView(logical)) return logical;
    if (inView(raw)) return raw;
    return logical;
  }

  unlistenDrag = await webview.onDragDropEvent(async (event) => {
    const payload = event.payload;
    if (payload.type === "enter" || payload.type === "over") {
      dragStatus.value = true;
      dragPoint.value = await toClientPoint(payload.position);
    } else if (payload.type === "leave") {
      dragStatus.value = false;
      dragPoint.value = null;
    } else if (payload.type === "drop") {
      const point = await toClientPoint(payload.position);
      if (compressKind.value === "model") {
        await modelPanelRef.value?.handleDrop(payload.paths, point);
      } else {
        void handlePaths(payload.paths);
      }
      dragStatus.value = false;
      dragPoint.value = null;
    }
  });
  window.addEventListener("keydown", onGlobalKeydown);
});

onUnmounted(() => {
  unlistenDrag?.();
  window.removeEventListener("keydown", onGlobalKeydown);
});

function onGlobalKeydown(e: KeyboardEvent) {
  if (compressKind.value === "model") return;
  const meta = e.metaKey || e.ctrlKey;
  if (meta && e.key.toLowerCase() === "a") {
    e.preventDefault();
    selectAll();
  }
}

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
  if (!fileList.value.length) return;
  fileList.value.forEach((f) => {
    f.selected = true;
  });
}

async function pickFiles() {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [
      {
        name: "图片",
        extensions: ["jpg", "jpeg", "png", "gif", "webp"],
      },
    ],
  });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  await handlePaths(paths);
}

async function pickFolder() {
  const selected = await open({ directory: true, multiple: false });
  if (!selected || Array.isArray(selected)) return;
  await handlePaths([selected]);
}

async function handlePaths(paths: string[]) {
  if (!paths.length) return;

  // 先让出一帧，保证拖入反馈立刻结束，避免同步卡顿感
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

  const { paths: images } = await invoke<{ paths: string[] }>("collect_images", {
    paths,
  });
  if (!images.length) return;

  const existing = new Set(fileList.value.map((f) => f.path));
  const newcomers = images.filter((p) => !existing.has(p));
  if (!newcomers.length) return;

  const items: FileItem[] = newcomers.map((path) => ({
    id: createId(),
    path,
    name: path.split(/[/\\]/).pop() || path,
    originSize: 0,
    outputSize: 0,
    ratio: 0,
    status: "pending",
    message: "",
    selected: false,
  }));

  fileList.value = fileList.value.concat(items);
  await nextTick();

  enqueueCompress(newcomers);
}

function enqueueCompress(paths: string[]) {
  pendingPaths.push(...paths);
  progressTotal.value += paths.length;
  void pumpCompress();
}

async function pumpCompress() {
  if (pumpRunning) return;
  pumpRunning = true;

  try {
    const workers = Array.from({ length: CONCURRENCY }, async () => {
      while (pendingPaths.length) {
        const path = pendingPaths.shift();
        if (!path) break;
        await compressOne(path);
        progressDone.value += 1;
        // 每张图后让出事件循环，保持列表滚动/选择流畅
        await new Promise<void>((resolve) => setTimeout(resolve, 0));
      }
    });
    await Promise.all(workers);
  } finally {
    pumpRunning = false;
    if (pendingPaths.length) {
      void pumpCompress();
      return;
    }
    if (progressDone.value >= progressTotal.value) {
      progressDone.value = 0;
      progressTotal.value = 0;
    }
  }
}

async function compressOne(path: string) {
  const item = fileList.value.find((f) => f.path === path);
  if (!item) return;
  item.status = "loading";
  item.message = "";
  currentCompressName.value = item.name;
  try {
    const result = await invoke<CompressResult>("compress_image", {
      path,
      quality: quality.value,
    });
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

async function recompressSelected() {
  const paths = selectedItems.value
    .filter((item) => item.status !== "loading")
    .map((item) => item.path);
  if (!paths.length) return;
  enqueueCompress(paths);
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
      return "压缩中…";
    case "best":
      return "已是最佳效果";
    case "compressed":
      return `↓ ${item.ratio.toFixed(1)}%`;
    case "restored":
      return "已回退";
    case "error":
      return item.message || "失败";
    default:
      return "";
  }
}

function onThemeChange(event: Event) {
  themeMode.value = (event.target as HTMLSelectElement).value as ThemeMode;
}
</script>

<template>
  <div class="app">
    <SplashScreen v-if="showSplash" @done="showSplash = false" />

    <header class="brand">
      <img :src="logo" class="brand-logo" alt="压鸭" />
      <div class="brand-text">
        <h1>压鸭</h1>
        <p>
          {{
            compressKind === "model"
              ? "本地 GLB 压缩 · 原路径替换"
              : "本地图片压缩 · 原路径替换"
          }}
        </p>
      </div>
      <span v-if="progressText" class="progress">{{ progressText }}</span>
    </header>

    <div class="kind-switch">
      <button
        type="button"
        :class="{ active: compressKind === 'image' }"
        @click="compressKind = 'image'"
      >
        图片
      </button>
      <button
        type="button"
        :class="{ active: compressKind === 'model' }"
        @click="compressKind = 'model'"
      >
        模型 GLB
      </button>
    </div>

    <ModelPanel
      v-if="compressKind === 'model'"
      ref="modelPanelRef"
      :drag-active="dragStatus"
      :drag-point="dragPoint"
    />

    <template v-else>
    <section v-if="isEmpty" class="dropzone" :class="{ active: dragStatus }">
      <div class="formats">
        <span>JPG</span>
        <span>PNG</span>
        <span>GIF</span>
        <span>WebP</span>
      </div>
      <p class="hint">拖入图片或文件夹开始压缩</p>
      <p class="sub">动图会尽量保留动画 · 为你的图片减减肥</p>
      <div class="actions">
        <button type="button" @click="pickFiles">选择文件</button>
        <button type="button" class="ghost" @click="pickFolder">选择文件夹</button>
      </div>
    </section>

    <section v-else class="list-wrap">
      <div class="toolbar">
        <button type="button" :disabled="!canUndo" title="回退选中" @click="undoSelected">
          回退
        </button>
        <button
          type="button"
          :disabled="!canClear"
          title="按当前质量重新压缩"
          @click="recompressSelected"
        >
          再压缩
        </button>
        <span class="spacer" />
        <button type="button" class="ghost" title="全选" @click="selectAll">全选</button>
        <button type="button" class="ghost" @click="pickFiles">添加</button>
        <button type="button" class="ghost" @click="pickFolder">文件夹</button>
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
      <p class="tip">双击对比 · Shift 连选 · ⌘/Ctrl 点选 · ⌘/Ctrl+A 全选</p>

      <div v-if="dragStatus" class="drag-overlay">松开以添加更多图片</div>
    </section>
    </template>

    <footer class="footer">
      <span>输出：原始路径替换</span>
      <div class="footer-controls">
        <label>
          主题
          <select :value="themeMode" @change="onThemeChange">
            <option value="system">跟随系统</option>
            <option value="light">浅色</option>
            <option value="dark">深色</option>
          </select>
        </label>
        <label v-if="compressKind === 'image'">
          质量
          <select v-model.number="quality">
            <option v-for="n in 9" :key="n" :value="n * 10">{{ n * 10 }}</option>
          </select>
        </label>
      </div>
    </footer>

    <CompareModal
      v-if="previewOpen && preview"
      :name="previewName"
      :preview="preview"
      @close="previewOpen = false"
    />
  </div>
</template>

<style scoped>
.app {
  height: 100vh;
  display: flex;
  flex-direction: column;
  user-select: none;
  -webkit-user-select: none;
  background:
    radial-gradient(120% 80% at 0% 0%, var(--brand-soft), transparent 55%),
    radial-gradient(90% 60% at 100% 100%, var(--ok-soft), transparent 50%),
    var(--bg);
  color: var(--text);
}

.brand {
  display: flex;
  gap: 12px;
  align-items: center;
  padding: 14px 16px 10px;
  border-bottom: 1px solid var(--line);
}

.brand-logo {
  width: 40px;
  height: 40px;
  border-radius: 12px;
  object-fit: cover;
  flex-shrink: 0;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.18);
}

.brand-text {
  min-width: 0;
  flex: 1;
}

.brand h1 {
  margin: 0;
  font-size: 20px;
  line-height: 1.2;
  letter-spacing: 0.04em;
}

.brand p {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--muted);
}

.progress {
  font-size: 12px;
  font-weight: 700;
  color: var(--accent);
  white-space: nowrap;
}

.kind-switch {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  padding: 10px 12px 0;
}

.kind-switch button {
  background: var(--ghost-bg);
  border: 1px solid var(--line);
  color: var(--text);
}

.kind-switch button.active {
  color: var(--accent-text);
  background: linear-gradient(180deg, #f2c13a, #e8a820);
  border-color: transparent;
}

.dropzone {
  flex: 1;
  margin: 16px;
  border: 1.5px dashed transparent;
  border-radius: 16px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  transition: border-color 0.2s, background 0.2s;
}

.dropzone.active {
  border-color: var(--accent);
  background: var(--drop-active);
}

.formats {
  display: flex;
  gap: 10px;
  margin-bottom: 8px;
}

.formats span {
  min-width: 52px;
  text-align: center;
  padding: 8px 0;
  border-radius: 10px;
  font-size: 12px;
  font-weight: 700;
  color: var(--ok-text);
  background: var(--ok-soft);
}

.hint {
  margin: 0;
  font-size: 15px;
  font-weight: 700;
  animation: pulse 1.2s ease-in-out infinite alternate;
}

.sub {
  margin: 0;
  font-size: 12px;
  color: var(--muted);
  animation: pulse 1.2s ease-in-out infinite alternate;
}

.actions {
  display: flex;
  gap: 8px;
  margin-top: 14px;
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

.drag-overlay {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  font-size: 14px;
  font-weight: 800;
  color: var(--accent-text);
  background: var(--overlay-drag);
  border: 2px dashed var(--accent);
  border-radius: 12px;
  margin: 8px;
  pointer-events: none;
  z-index: 5;
}

.footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  font-size: 12px;
  font-weight: 700;
  border-top: 1px solid var(--line);
  background: var(--bg-elevated);
}

.footer-controls {
  display: flex;
  gap: 12px;
  align-items: center;
}

.footer label {
  display: flex;
  align-items: center;
  gap: 6px;
}

select {
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 4px 8px;
  background: var(--control-bg);
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
