<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import ImagePanel from "./components/ImagePanel.vue";
import ModelPanel from "./components/ModelPanel.vue";
import SplashScreen from "./components/SplashScreen.vue";
import { useTheme, type ThemeMode } from "./composables/useTheme";
import logo from "./assets/logo.png";
import { loadCompressKind, saveCompressKind, type CompressKind } from "./lib/settings";

const { mode: themeMode } = useTheme();

const compressKind = ref<CompressKind>(loadCompressKind());
const modelPanelRef = ref<InstanceType<typeof ModelPanel> | null>(null);
const imagePanelRef = ref<InstanceType<typeof ImagePanel> | null>(null);
const dragPoint = ref<{ x: number; y: number } | null>(null);
const dragStatus = ref(false);
const showSplash = ref(true);

let unlistenDrag: (() => void) | null = null;

const progressText = computed(() => {
  if (compressKind.value === "model") {
    return modelPanelRef.value?.progressText || "";
  }
  return imagePanelRef.value?.progressText || "";
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
   */
  async function toClientPoint(position: { x: number; y: number }) {
    const scale = await windowRef.scaleFactor();
    const w = window.innerWidth;
    const h = window.innerHeight;
    const raw = { x: position.x, y: position.y };
    const logical = { x: position.x / scale, y: position.y / scale };
    const inView = (p: { x: number; y: number }) =>
      p.x >= -8 && p.y >= -8 && p.x <= w + 8 && p.y <= h + 8;

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
        await imagePanelRef.value?.handleDrop(payload.paths, point);
      }
      dragStatus.value = false;
      dragPoint.value = null;
    }
  });
});

onUnmounted(() => {
  unlistenDrag?.();
});

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
              : "本地图片压缩 / 改尺寸 · 原路径替换"
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

    <ImagePanel
      v-else
      ref="imagePanelRef"
      :drag-active="dragStatus"
      :drag-point="dragPoint"
    />

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
      </div>
    </footer>
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
  border-radius: 9px;
  padding: 7px 12px;
  font: inherit;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
}

.kind-switch button.active {
  color: var(--accent-text);
  background: linear-gradient(180deg, #f2c13a, #e8a820);
  border-color: transparent;
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
  color: var(--text);
  font: inherit;
}
</style>
