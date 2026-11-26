<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import CompareModal from "./CompareModal.vue";
import {
  loadImageResizeOptions,
  loadQuality,
  loadGifOptions,
  loadOutputDir,
  loadOutputMode,
  saveGifOptions,
  saveImageResizeOptions,
  saveOutputDir,
  saveOutputMode,
  saveQuality,
  resolveOutputPath,
  type OutputMode,
} from "../lib/settings";
import {
  GIF_PRESETS,
  gifPresetOf,
  resizeValueForMode,
  type GifCompressOptions,
  type GifDither,
  type GifPresetId,
  type ImageOpMode,
  type ResizeMode,
} from "../lib/imageOptions";
import { useI18n } from "../i18n";
import {
  createId,
  formatSize,
  type CompressResult,
  type FileItem,
  type PreviewResult,
} from "../types";

const CONCURRENCY = 2;
const { t } = useI18n();

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
const gifOpts = ref<GifCompressOptions>(loadGifOptions());
const outputMode = ref<OutputMode>(loadOutputMode());
const outputDir = ref<string>(loadOutputDir());

watch(quality, (v) => saveQuality(v));
watch(resizeOpts, (v) => saveImageResizeOptions(v), { deep: true });
watch(gifOpts, (v) => saveGifOptions(v), { deep: true });
watch(outputMode, (v) => saveOutputMode(v));
watch(outputDir, (v) => saveOutputDir(v));

const gifPreset = computed<GifPresetId>(() => gifPresetOf(gifOpts.value));

function onGifPresetChange(e: Event) {
  const id = (e.target as HTMLSelectElement).value as Exclude<
    GifPresetId,
    "custom"
  >;
  const preset = GIF_PRESETS[id];
  if (preset) gifOpts.value = { ...preset };
}

function onGifFpsChange(e: Event) {
  gifOpts.value.maxFps = Number((e.target as HTMLSelectElement).value);
}

function onGifColorsChange(e: Event) {
  gifOpts.value.maxColors = Number((e.target as HTMLSelectElement).value);
}

function onGifDitherChange(e: Event) {
  gifOpts.value.dither = (e.target as HTMLSelectElement).value as GifDither;
}

function onGifMaxWidthChange(e: Event) {
  gifOpts.value.maxWidth = Number((e.target as HTMLSelectElement).value);
}

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
    return t("common.processingNamed", {
      done: progressDone.value,
      total: progressTotal.value,
      name: currentCompressName.value,
    });
  }
  return t("common.processing", {
    done: progressDone.value,
    total: progressTotal.value,
  });
});

const resizeValueLabel = computed(() => {
  if (resizeOpts.value.mode === "percent") return t("image.percentLabel");
  if (resizeOpts.value.mode === "height") return t("image.heightPx");
  return t("image.widthPx");
});

const activeResizeField = computed({
  get() {
    if (resizeOpts.value.mode === "percent") return resizeOpts.value.percent;
    if (resizeOpts.value.mode === "height") return resizeOpts.value.height;
    return resizeOpts.value.width;
  },
  set(v: number) {
    if (resizeOpts.value.mode === "percent") resizeOpts.value.percent = v;
    else if (resizeOpts.value.mode === "height") resizeOpts.value.height = v;
    else resizeOpts.value.width = v;
  },
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
    const resolvedOutput = resolveOutputPath(
      path,
      outputMode.value,
      outputDir.value,
    );
    const outputPathParam =
      outputMode.value === "overwrite" ? null : resolvedOutput;

    if (op === "resize") {
      result = await invoke<CompressResult>("resize_image", {
        path,
        quality: quality.value,
        mode: resizeOpts.value.mode,
        value: resizeValueForMode(resizeOpts.value),
        lockAspect: resizeOpts.value.lockAspect,
        outputPath: outputPathParam,
      });
    } else {
      result = await invoke<CompressResult>("compress_image", {
        path,
        quality: quality.value,
        outputPath: outputPathParam,
        gifOptions: { ...gifOpts.value },
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
  item.outputPath = result.outputPath;
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
      const isOverwrite = !item.outputPath || item.outputPath === item.path;
      if (isOverwrite) {
        // 原地覆盖：把原图写回原路径
        const result = await invoke<CompressResult>("restore_image", {
          path: item.path,
        });
        applyResult(item, result);
      } else {
        // 新增文件：删除压缩后生成的文件
        await invoke("remove_file", { path: item.outputPath });
        item.outputPath = undefined;
        item.outputSize = item.originSize;
        item.ratio = 0;
        item.status = "restored";
        item.message = t("common.restored");
      }
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
      return item.lastOp === "resize"
        ? t("image.statusResizing")
        : t("image.statusCompressing");
    case "best":
      return item.message || t("common.bestAlready");
    case "compressed":
      if (item.lastOp === "resize") {
        if (item.ratio >= 0) {
          return t("image.statusResizedDown", {
            ratio: item.ratio.toFixed(1),
          });
        }
        return t("image.statusResizedUp", {
          ratio: Math.abs(item.ratio).toFixed(1),
        });
      }
      return t("image.statusSaved", { ratio: item.ratio.toFixed(1) });
    case "restored":
      return t("common.restored");
    case "error":
      return item.message || t("common.failed");
    default:
      return "";
  }
}

function onResizeModeChange(e: Event) {
  resizeOpts.value.mode = (e.target as HTMLSelectElement).value as ResizeMode;
}

async function pickOutputDir() {
  const selected = await open({ directory: true, multiple: false });
  if (!selected || Array.isArray(selected)) return;
  outputDir.value = selected;
}

defineExpose({
  handleDrop,
  progressText,
});
</script>

<template>
  <div ref="panelEl" class="image-panel">
    <section class="options">
      <label class="opt" :title="t('image.qualityTip')">
        <span>{{ t("image.quality") }}</span>
        <select v-model.number="quality">
          <option v-for="n in 9" :key="n" :value="n * 10">{{ n * 10 }}</option>
        </select>
      </label>

      <span class="divider" />

      <label class="opt" :title="t('image.resizeTip')">
        <span>{{ t("image.resize") }}</span>
        <select :value="resizeOpts.mode" @change="onResizeModeChange">
          <option value="width">{{ t("image.byWidth") }}</option>
          <option value="height">{{ t("image.byHeight") }}</option>
          <option value="percent">{{ t("image.byPercent") }}</option>
        </select>
      </label>

      <label class="opt">
        <span>{{ resizeValueLabel }}</span>
        <input
          v-model.number="activeResizeField"
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
        :title="t('image.lockAspectTip')"
      >
        <input v-model="resizeOpts.lockAspect" type="checkbox" />
        <span>{{ t("image.lockAspect") }}</span>
      </label>

      <span class="divider" />

      <label class="opt" :title="t('output.overwriteDesc')">
        <span>{{ t("output.title") }}</span>
        <select v-model="outputMode">
          <option value="overwrite">{{ t("output.overwrite") }}</option>
          <option value="newfile">{{ t("output.newfile") }}</option>
          <option value="custom">{{ t("output.custom") }}</option>
        </select>
      </label>

      <button
        v-if="outputMode === 'custom'"
        type="button"
        class="ghost small"
        @click="pickOutputDir"
      >
        {{ t("output.pickDir") }}
      </button>

      <span
        v-if="outputMode === 'custom'"
        class="opt-hint"
        :title="outputDir || t('output.noDirTip')"
      >
        {{ outputDir || t("output.noDirTip") }}
      </span>
    </section>

    <section class="options gif-options">
      <span class="row-tag">{{ t("image.gifTitle") }}</span>

      <label class="opt" :title="t('image.gifPresetTip')">
        <span>{{ t("image.gifPreset") }}</span>
        <select :value="gifPreset" @change="onGifPresetChange">
          <option value="default">{{ t("image.gifPresetDefault") }}</option>
          <option value="light">{{ t("image.gifPresetLight") }}</option>
          <option value="balanced">{{ t("image.gifPresetBalanced") }}</option>
          <option value="aggressive">{{ t("image.gifPresetAggressive") }}</option>
          <option value="custom" disabled>
            {{ t("image.gifPresetCustom") }}
          </option>
        </select>
      </label>

      <span class="divider" />

      <label class="opt" :title="t('image.gifFpsTip')">
        <span>{{ t("image.gifFps") }}</span>
        <select :value="gifOpts.maxFps" @change="onGifFpsChange">
          <option :value="0">{{ t("image.gifFpsOriginal") }}</option>
          <option v-for="v in [5, 8, 10, 12, 15, 20, 24, 30]" :key="v" :value="v">
            {{ v }}
          </option>
        </select>
      </label>

      <label class="opt" :title="t('image.gifColorsTip')">
        <span>{{ t("image.gifColors") }}</span>
        <select :value="gifOpts.maxColors" @change="onGifColorsChange">
          <option
            v-for="v in [16, 32, 48, 64, 96, 128, 160, 192, 256]"
            :key="v"
            :value="v"
          >
            {{ v }}
          </option>
        </select>
      </label>

      <label class="opt" :title="t('image.gifDitherTip')">
        <span>{{ t("image.gifDither") }}</span>
        <select :value="gifOpts.dither" @change="onGifDitherChange">
          <option value="bayer">{{ t("image.gifDitherBayer") }}</option>
          <option value="floyd_steinberg">{{ t("image.gifDitherFs") }}</option>
          <option value="none">{{ t("image.gifDitherNone") }}</option>
        </select>
      </label>

      <label class="opt" :title="t('image.gifMaxWidthTip')">
        <span>{{ t("image.gifMaxWidth") }}</span>
        <select :value="gifOpts.maxWidth" @change="onGifMaxWidthChange">
          <option :value="0">{{ t("image.gifNoScale") }}</option>
          <option v-for="v in [480, 560, 640, 720, 960, 1280]" :key="v" :value="v">
            {{ v }}
          </option>
        </select>
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
        <strong>{{ t("image.zoneCompress") }}</strong>
        <p v-if="isEmpty">{{ t("image.zoneCompressDesc") }}</p>
        <div v-if="isEmpty" class="actions">
          <button type="button" @click="pickFiles('compress')">
            {{ t("common.pickFiles") }}
          </button>
          <button type="button" class="ghost" @click="pickFolder('compress')">
            {{ t("common.folder") }}
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
        <strong>{{ t("image.zoneResize") }}</strong>
        <p v-if="isEmpty">{{ t("image.zoneResizeDesc") }}</p>
        <div v-if="isEmpty" class="actions">
          <button type="button" @click="pickFiles('resize')">
            {{ t("common.pickFiles") }}
          </button>
          <button type="button" class="ghost" @click="pickFolder('resize')">
            {{ t("common.folder") }}
          </button>
        </div>
      </div>
    </section>

    <section v-if="!isEmpty" class="list-wrap">
      <div class="toolbar">
        <button
          type="button"
          :disabled="!canUndo"
          :title="t('image.undoTip')"
          @click="undoSelected"
        >
          {{ t("common.undo") }}
        </button>
        <button
          type="button"
          :disabled="!canClear"
          :title="t('image.reprocessTip')"
          @click="reprocessSelected"
        >
          {{ t("image.reprocess") }}
        </button>
        <span class="spacer" />
        <button type="button" class="ghost" @click="selectAll">
          {{ t("common.selectAll") }}
        </button>
        <button type="button" class="ghost" @click="pickFiles('compress')">
          {{ t("image.addCompress") }}
        </button>
        <button type="button" class="ghost" @click="pickFiles('resize')">
          {{ t("image.addResize") }}
        </button>
        <button
          type="button"
          :disabled="!canClear"
          class="danger"
          @click="clearSelected"
        >
          {{ t("common.clear") }}
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
      <p class="tip">{{ t("image.tip") }}</p>
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

.gif-options {
  padding-top: 8px;
  padding-bottom: 8px;
  font-weight: 600;
  opacity: 0.92;
}

.row-tag {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(127, 127, 127, 0.16);
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0.02em;
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

.opt-hint {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--muted);
  font-weight: 500;
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

button.small{
  padding: 2px 4px;
  border-radius: 5px;
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
