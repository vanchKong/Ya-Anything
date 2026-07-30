<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import ImagePanel from "./components/ImagePanel.vue";
import ModelPanel from "./components/ModelPanel.vue";
import SplashScreen from "./components/SplashScreen.vue";
import { useTheme, type ThemeMode } from "./composables/useTheme";
import logo from "./assets/logo.png";
import { loadCompressKind, saveCompressKind, type CompressKind } from "./lib/settings";

const REPO_URL = "https://github.com/vanchKong/Ya-Anything";

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

async function openRepo() {
  try {
    await openUrl(REPO_URL);
  } catch {
    window.open(REPO_URL, "_blank", "noopener,noreferrer");
  }
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
      <button
        type="button"
        class="github-btn"
        title="开源仓库"
        aria-label="打开 GitHub 开源仓库"
        @click="openRepo"
      >
        <svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true">
          <path
            fill="currentColor"
            d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8"
          />
        </svg>
      </button>
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
  max-width: 42%;
  overflow: hidden;
  text-overflow: ellipsis;
}

.github-btn {
  flex-shrink: 0;
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: var(--ghost-bg);
  color: var(--text);
  cursor: pointer;
  padding: 0;
}

.github-btn:hover {
  color: var(--accent);
  border-color: var(--accent);
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
