<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { readFile, writeFile } from "@tauri-apps/plugin-fs";
import { open } from "@tauri-apps/plugin-dialog";
import ModelPreview from "./ModelPreview.vue";
import ModelCompare from "./ModelCompare.vue";
import {
  optionsFromPreset,
  type CompressPresetId,
  type DracoPrecision,
  type DracoSpeed,
  type GlbCompressOptions,
  type MeshCompress,
  type MeshoptLevel,
  type TextureEncoder,
  type TextureFormat,
  type TextureMaxSize,
} from "../lib/glbOptions";
import { loadGlbOptions, saveGlbOptions } from "../lib/settings";
import { createId, formatSize, type FileItem } from "../types";

const CONCURRENCY = 1;

const props = defineProps<{
  dragActive: boolean;
  dragPoint: { x: number; y: number } | null;
}>();

const panelEl = ref<HTMLElement | null>(null);
const dualDropEl = ref<HTMLElement | null>(null);
const previewZone = ref<HTMLElement | null>(null);
const compressZone = ref<HTMLElement | null>(null);
const fileList = ref<FileItem[]>([]);
const options = ref<GlbCompressOptions>(loadGlbOptions());
let applyingPreset = false;

watch(
  options,
  (v) => saveGlbOptions(v),
  { deep: true },
);

/** 改任意非 preset 字段 → 标为自定义 */
watch(
  () => {
    const { preset: _p, ...rest } = options.value;
    return rest;
  },
  () => {
    if (applyingPreset) return;
    if (options.value.preset !== "custom") {
      options.value.preset = "custom";
    }
  },
  { deep: true },
);

function applyPreset(id: Exclude<CompressPresetId, "custom">) {
  applyingPreset = true;
  options.value = optionsFromPreset(id);
  void nextTick(() => {
    applyingPreset = false;
  });
}

function onPresetChange(e: Event) {
  const id = (e.target as HTMLSelectElement).value as CompressPresetId;
  if (id === "custom") {
    options.value.preset = "custom";
    return;
  }
  applyPreset(id);
}

function onMeshCompressChange(e: Event) {
  options.value.meshCompress = (e.target as HTMLSelectElement)
    .value as MeshCompress;
}

function onDracoSpeedChange(e: Event) {
  options.value.dracoSpeed = Number(
    (e.target as HTMLSelectElement).value,
  ) as DracoSpeed;
}

function onDracoPrecisionChange(e: Event) {
  options.value.dracoPrecision = (e.target as HTMLSelectElement)
    .value as DracoPrecision;
}

function onMeshoptLevelChange(e: Event) {
  options.value.meshoptLevel = (e.target as HTMLSelectElement)
    .value as MeshoptLevel;
}
const progressDone = ref(0);
const progressTotal = ref(0);
const anchorIndex = ref(0);
const hoverZone = ref<"preview" | "compress" | null>(null);
let lastZone: "preview" | "compress" | null = null;

const previewOpen = ref(false);
const compareOpen = ref(false);
const dialogName = ref("");
const previewBytes = ref<Uint8Array | null>(null);
const compareOrigin = ref<Uint8Array | null>(null);
const compareCompressed = ref<Uint8Array | null>(null);

const originals = new Map<string, Uint8Array>();
const compressedMap = new Map<string, Uint8Array>();
const pendingPaths: string[] = [];
let pumpRunning = false;

const stageHint = ref("");
const currentName = ref("");

const selectedItems = computed(() => fileList.value.filter((f) => f.selected));
const canUndo = computed(() =>
  selectedItems.value.some((f) => f.status === "compressed"),
);
const canCompare = computed(() => {
  const item = selectedItems.value[0];
  return (
    selectedItems.value.length === 1 &&
    !!item &&
    (item.status === "compressed" || item.status === "best") &&
    originals.has(item.path)
  );
});
const canClear = computed(() => selectedItems.value.length > 0);
const isEmpty = computed(() => fileList.value.length === 0);
const isWorking = computed(
  () => progressTotal.value > 0 && progressDone.value < progressTotal.value,
);
const progressText = computed(() => {
  if (!isWorking.value) return "";
  const base = `压缩中 ${progressDone.value}/${progressTotal.value}`;
  if (currentName.value && stageHint.value) {
    return `${base} · ${currentName.value} · ${stageHint.value}`;
  }
  if (stageHint.value) return `${base} · ${stageHint.value}`;
  if (currentName.value) return `${base} · ${currentName.value}`;
  return base;
});

watch(
  () => [props.dragActive, props.dragPoint] as const,
  async ([active, point]) => {
    if (!active || !point) {
      // 保留 lastZone 供 drop；仅清高亮
      hoverZone.value = null;
      return;
    }
    // 列表态拖入时迷你投放区刚显示，等布局完成再测
    await nextTick();
    if (!props.dragActive || !props.dragPoint) return;
    applyHover(props.dragPoint.x, props.dragPoint.y);
  },
);

function setHoverZone(zone: "preview" | "compress") {
  hoverZone.value = zone;
  lastZone = zone;
}

function applyHover(x: number, y: number) {
  const zone = hitTest(x, y);
  if (zone) setHoverZone(zone);
  else hoverZone.value = null;
}

/**
 * 系统文件拖入时 DOM mouseover 通常无效，用坐标命中。
 * 先测单框 contains；再按 dual-drop 水平比例（避免坐标轻微偏移时左右错乱）。
 */
function hitTest(x: number, y: number): "preview" | "compress" | null {
  const p = previewZone.value?.getBoundingClientRect();
  const c = compressZone.value?.getBoundingClientRect();
  if (p && p.width > 2 && x >= p.left && x <= p.right && y >= p.top && y <= p.bottom) {
    return "preview";
  }
  if (c && c.width > 2 && x >= c.left && x <= c.right && y >= c.top && y <= c.bottom) {
    return "compress";
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
      return t < 0.5 ? "preview" : "compress";
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
  return x < panel.left + panel.width / 2 ? "preview" : "compress";
}

function onZoneDragOver(zone: "preview" | "compress", e: DragEvent) {
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

async function pickFiles(mode: "preview" | "compress") {
  const selected = await open({
    multiple: true,
    filters: [{ name: "GLB/GLTF", extensions: ["glb", "gltf"] }],
  });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  if (mode === "preview") await previewPaths(paths);
  else await compressPaths(paths);
}

async function handleDrop(
  paths: string[],
  point: { x: number; y: number } | null,
) {
  const zone = (point ? hitTest(point.x, point.y) : null) ?? lastZone ?? "compress";
  lastZone = null;
  hoverZone.value = null;
  if (zone === "preview") {
    await previewPaths(paths);
    return;
  }
  await compressPaths(paths);
}

async function collectModels(paths: string[]) {
  const { paths: models } = await invoke<{ paths: string[] }>("collect_models", {
    paths,
  });
  return models;
}

async function previewPaths(paths: string[]) {
  const models = await collectModels(paths);
  if (!models.length) return;
  const path = models[0];
  previewBytes.value = await readFile(path);
  dialogName.value = path.split(/[/\\]/).pop() || path;
  previewOpen.value = true;
}

async function compressPaths(paths: string[]) {
  await new Promise<void>((r) => requestAnimationFrame(() => r()));
  const models = await collectModels(paths);
  if (!models.length) return;

  const existing = new Set(fileList.value.map((f) => f.path));
  const newcomers = models.filter((p) => !existing.has(p));
  if (!newcomers.length) {
    enqueueCompress(models);
    return;
  }

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
        await new Promise<void>((r) => setTimeout(r, 0));
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
  currentName.value = item.name;
  stageHint.value = "准备中…";
  try {
    let origin = originals.get(path);
    if (!origin) {
      origin = await readFile(path);
      originals.set(path, origin);
    }
    item.originSize = origin.byteLength;

    const { compressGlbBytes } = await import("../lib/glbCompress");
    const output = await compressGlbBytes(origin, options.value, (info) => {
      stageHint.value = info.message;
      item.message = info.message;
    });
    if (output.byteLength === 0 || output.byteLength >= origin.byteLength) {
      compressedMap.set(path, origin.slice());
      item.outputSize = origin.byteLength;
      item.ratio = 0;
      item.status = "best";
      item.message = "已是最佳效果";
      return;
    }

    await writeFile(path, output);
    compressedMap.set(path, output);
    item.outputSize = output.byteLength;
    item.ratio =
      ((origin.byteLength - output.byteLength) / origin.byteLength) * 100;
    item.status = "compressed";
    item.message = "";
  } catch (err) {
    item.status = "error";
    item.message = err instanceof Error ? err.message : String(err);
  } finally {
    if (currentName.value === item.name) {
      currentName.value = "";
      stageHint.value = "";
    }
  }
}

async function undoSelected() {
  for (const item of selectedItems.value) {
    if (item.status !== "compressed") continue;
    const origin = originals.get(item.path);
    if (!origin) {
      item.status = "error";
      item.message = "无可回退原文件（仅本会话）";
      continue;
    }
    try {
      await writeFile(item.path, origin);
      compressedMap.delete(item.path);
      item.outputSize = origin.byteLength;
      item.ratio = 0;
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
    .filter((i) => i.status !== "loading")
    .map((i) => i.path);
  if (paths.length) enqueueCompress(paths);
}

function clearSelected() {
  const paths = new Set(selectedItems.value.map((f) => f.path));
  for (const p of paths) {
    originals.delete(p);
    compressedMap.delete(p);
  }
  fileList.value = fileList.value.filter((f) => !paths.has(f.path));
}

async function openPreview(item: FileItem) {
  try {
    previewBytes.value = await readFile(item.path);
    dialogName.value = item.name;
    previewOpen.value = true;
  } catch (err) {
    item.message = err instanceof Error ? err.message : String(err);
  }
}

async function openCompare(item?: FileItem) {
  const target = item ?? selectedItems.value[0];
  if (!target) return;
  const origin = originals.get(target.path);
  if (!origin) {
    await openPreview(target);
    return;
  }
  let compressed = compressedMap.get(target.path);
  if (!compressed) {
    try {
      compressed = await readFile(target.path);
    } catch {
      await openPreview(target);
      return;
    }
  }
  compareOrigin.value = origin;
  compareCompressed.value = compressed;
  dialogName.value = target.name;
  compareOpen.value = true;
}

async function onItemActivate(item: FileItem) {
  if (
    (item.status === "compressed" || item.status === "best") &&
    originals.has(item.path)
  ) {
    await openCompare(item);
  } else {
    await openPreview(item);
  }
}

function statusText(item: FileItem): string {
  switch (item.status) {
    case "loading":
      return item.message || "压缩中…";
    case "best":
      return "已是最佳效果";
    case "compressed":
      return `剩余 ${(100 - item.ratio).toFixed(1)}%`;
    case "restored":
      return "已回退";
    case "error":
      return item.message || "失败";
    default:
      return "";
  }
}

function onTextureEncoderChange(e: Event) {
  options.value.textureEncoder = (e.target as HTMLSelectElement)
    .value as TextureEncoder;
}

function onTextureChange(e: Event) {
  options.value.textureFormat = (e.target as HTMLSelectElement)
    .value as TextureFormat;
}

function onMaxSizeChange(e: Event) {
  options.value.textureMaxSize = Number(
    (e.target as HTMLSelectElement).value,
  ) as TextureMaxSize;
}

defineExpose({
  handleDrop,
  progressText,
});
</script>

<template>
  <div ref="panelEl" class="model-panel">
    <section class="options">
      <label
        class="opt"
        title="选档位会自动填充下方全部选项；改任意项后变为自定义"
      >
        <span>预设</span>
        <select :value="options.preset" @change="onPresetChange">
          <option value="max">极致小体积</option>
          <option value="compact">轻量</option>
          <option value="balanced">均衡</option>
          <option value="fast">快速保真</option>
          <option value="custom">自定义</option>
        </select>
        <i class="tip-dot">?</i>
      </label>

      <label
        class="opt"
        title="无=不压网格；Draco 兼容广但可能较慢；Meshopt 接近 gltfpack"
      >
        <span>网格压缩</span>
        <select :value="options.meshCompress" @change="onMeshCompressChange">
          <option value="none">无</option>
          <option value="draco">Draco</option>
          <option value="meshopt">Meshopt</option>
        </select>
        <i class="tip-dot">?</i>
      </label>

      <label
        v-if="options.meshCompress === 'draco'"
        class="opt"
        title="0 最慢通常最小，10 最快文件更大"
      >
        <span>Draco速度</span>
        <select :value="options.dracoSpeed" @change="onDracoSpeedChange">
          <option :value="0">0 极慢</option>
          <option :value="3">3 慢</option>
          <option :value="5">5 中</option>
          <option :value="8">8 快</option>
          <option :value="10">10 最快</option>
        </select>
      </label>

      <label
        v-if="options.meshCompress === 'draco'"
        class="opt"
        title="量化位数：低=更小更糊，高=更大更保真"
      >
        <span>Draco精度</span>
        <select
          :value="options.dracoPrecision"
          @change="onDracoPrecisionChange"
        >
          <option value="low">低（更小）</option>
          <option value="medium">中</option>
          <option value="high">高（更保真）</option>
        </select>
      </label>

      <label v-if="options.meshCompress === 'meshopt'" class="opt">
        <span>Meshopt</span>
        <select :value="options.meshoptLevel" @change="onMeshoptLevelChange">
          <option value="medium">medium</option>
          <option value="high">high</option>
        </select>
      </label>

      <label
        v-if="options.meshCompress === 'none' || options.meshCompress === 'meshopt'"
        class="opt"
        title="降低顶点精度。选 Draco 时由「Draco精度」控制，此项不参与"
      >
        <input v-model="options.quantize" type="checkbox" />
        <span>顶点量化</span>
        <i class="tip-dot">?</i>
      </label>

      <label
        class="opt"
        title="去重 accessor/纹理，几乎无损"
      >
        <input v-model="options.dedup" type="checkbox" />
        <span>去重</span>
      </label>

      <label
        class="opt"
        title="焊接重合顶点。大模型可能非常慢；要快请关掉"
      >
        <input v-model="options.weld" type="checkbox" />
        <span>焊接顶点</span>
        <i class="tip-dot">?</i>
      </label>

      <label class="opt" title="动画关键帧重采样">
        <input v-model="options.resample" type="checkbox" />
        <span>动画重采样</span>
      </label>

      <label
        class="opt"
        title="移除未使用资源。若压缩后丢节点/材质请关闭"
      >
        <input v-model="options.prune" type="checkbox" />
        <span>清除冗余</span>
        <i class="tip-dot">?</i>
      </label>

      <label class="opt" title="合并兼容网格，减少 draw call">
        <input v-model="options.joinMeshes" type="checkbox" />
        <span>合并网格</span>
      </label>

      <label
        class="opt"
        title="有损减面，体积可再降；动画/角色请谨慎"
      >
        <input v-model="options.simplify" type="checkbox" />
        <span>网格简化</span>
        <i class="tip-dot">?</i>
      </label>

      <label v-if="options.simplify" class="opt">
        <span>简化率</span>
        <select v-model.number="options.simplifyRatio">
          <option :value="0.75">75%</option>
          <option :value="0.5">50%</option>
          <option :value="0.35">35%</option>
          <option :value="0.2">20%</option>
        </select>
      </label>

      <label
        class="opt"
        title="贴图边长上限。不缩放=保持原分辨率（体积关键杠杆）"
      >
        <span>贴图边长</span>
        <select :value="options.textureMaxSize" @change="onMaxSizeChange">
          <option :value="0">不缩放</option>
          <option :value="2048">2048</option>
          <option :value="1024">1024</option>
          <option :value="512">512</option>
        </select>
        <i class="tip-dot">?</i>
      </label>

      <label
        class="opt"
        title="⚠️ 体积与耗时的最大头。只要不是「保持」，就会转码贴图。初版体感请选「快速」编码器"
      >
        <span>贴图格式</span>
        <select :value="options.textureFormat" @change="onTextureChange">
          <option value="keep">保持</option>
          <option value="webp">WebP</option>
          <option value="jpeg">JPG</option>
          <option value="png">PNG</option>
        </select>
        <i class="tip-dot">?</i>
      </label>

      <label
        v-if="options.textureFormat !== 'keep' || options.textureMaxSize > 0"
        class="opt"
        title="快速=浏览器编码（接近初版：快、约十数MB）；强力=Rust libwebp（慢、可压到数MB）"
      >
        <span>贴图编码</span>
        <select
          :value="options.textureEncoder"
          @change="onTextureEncoderChange"
        >
          <option value="canvas">快速</option>
          <option value="rust">强力</option>
        </select>
        <i class="tip-dot">?</i>
      </label>

      <label
        v-if="options.textureFormat === 'jpeg' || options.textureFormat === 'webp'"
        class="opt"
        title="仅 WebP/JPG 有损质量，越低越小越糊"
      >
        <span>贴图质量</span>
        <select v-model.number="options.textureQuality">
          <option :value="40">40</option>
          <option :value="50">50</option>
          <option :value="60">60</option>
          <option :value="70">70</option>
          <option :value="80">80</option>
          <option :value="88">88</option>
          <option :value="90">90</option>
        </select>
      </label>

      <span v-if="progressText" class="progress">{{ progressText }}</span>
    </section>

    <!-- 投放区始终挂载：空态大卡片 / 列表态拖入时显示迷你条 -->
    <section
      ref="dualDropEl"
      v-show="isEmpty || dragActive"
      class="dual-drop"
      :class="{ mini: !isEmpty }"
    >
      <div
        ref="previewZone"
        data-drop-zone="preview"
        class="drop-card"
        :class="{ active: dragActive && hoverZone === 'preview' }"
        @dragenter.prevent="onZoneDragOver('preview', $event)"
        @dragover.prevent="onZoneDragOver('preview', $event)"
      >
        <strong>预览</strong>
        <p v-if="isEmpty">拖入查看模型（含动画）</p>
        <button
          v-if="isEmpty"
          type="button"
          class="ghost"
          @click="pickFiles('preview')"
        >
          选择文件
        </button>
      </div>
      <div
        ref="compressZone"
        data-drop-zone="compress"
        class="drop-card"
        :class="{ active: dragActive && hoverZone === 'compress' }"
        @dragenter.prevent="onZoneDragOver('compress', $event)"
        @dragover.prevent="onZoneDragOver('compress', $event)"
      >
        <strong>压缩</strong>
        <p v-if="isEmpty">拖入压缩并覆盖原文件</p>
        <button v-if="isEmpty" type="button" @click="pickFiles('compress')">
          选择文件
        </button>
      </div>
    </section>

    <section v-if="!isEmpty" class="list-wrap">
      <div class="toolbar">
        <button type="button" :disabled="!canUndo" title="回退选中到原文件" @click="undoSelected">
          回退
        </button>
        <button type="button" :disabled="!canCompare" title="原模型 / 压缩后比对" @click="openCompare()">
          比对
        </button>
        <button type="button" :disabled="!canClear" @click="recompressSelected">
          再压缩
        </button>
        <span class="spacer" />
        <button type="button" class="ghost" @click="selectAll">全选</button>
        <button type="button" class="ghost" @click="pickFiles('compress')">添加</button>
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
          @dblclick="onItemActivate(item)"
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
        双击比对或预览 · 预设会填充全部选项，改任意项即变自定义 · 回退仅本会话有效
      </p>
    </section>

    <ModelPreview
      v-if="previewOpen && previewBytes"
      :name="dialogName"
      :bytes="previewBytes"
      @close="previewOpen = false"
    />
    <ModelCompare
      v-if="compareOpen && compareOrigin && compareCompressed"
      :name="dialogName"
      :origin-bytes="compareOrigin"
      :compressed-bytes="compareCompressed"
      @close="compareOpen = false"
    />
  </div>
</template>

<style scoped>
.model-panel {
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
}

.opt {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  cursor: pointer;
}

.tip-dot {
  display: inline-grid;
  place-items: center;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  font-style: normal;
  font-size: 10px;
  color: var(--accent-text);
  background: var(--accent);
}

.progress {
  width: 100%;
  color: var(--accent);
  font-size: 12px;
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
  transition: border-color 0.15s, background 0.15s;
  background: rgba(255, 255, 255, 0.04);
}

.dual-drop.mini .drop-card {
  min-height: 52px;
  padding: 8px;
  border-radius: 10px;
  flex-direction: row;
  gap: 6px;
}

.drop-card strong {
  font-size: 16px;
}

.dual-drop.mini .drop-card strong {
  font-size: 13px;
}

.drop-card p {
  margin: 0;
  font-size: 12px;
  color: var(--muted);
  font-weight: 600;
}

.drop-card.active {
  border-color: var(--accent);
  background: var(--drop-active);
}

.list-wrap {
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
  padding: 3px 6px;
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
</style>
