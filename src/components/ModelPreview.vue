<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import ModelViewerCanvas from "./ModelViewerCanvas.vue";
import { formatSize } from "../types";
import { useI18n } from "../i18n";

const props = defineProps<{
  name: string;
  bytes: Uint8Array;
}>();

const emit = defineEmits<{ close: [] }>();
const { t } = useI18n();

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
}

onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="panel" role="dialog" aria-modal="true">
      <header>
        <div>
          <strong>{{ name }}</strong>
          <span class="size">{{ formatSize(bytes.byteLength) }}</span>
        </div>
        <button type="button" class="close" @click="emit('close')">
          {{ t("common.close") }}
        </button>
      </header>
      <ModelViewerCanvas :bytes="bytes" />
      <p class="hint">{{ t("model.previewHint") }}</p>
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
  width: min(560px, 100%);
  background: var(--bg);
  color: var(--text);
  border-radius: 14px;
  overflow: hidden;
  border: 1px solid var(--line);
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.25);
}

header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 12px;
  border-bottom: 1px solid var(--line);
  font-size: 13px;
  gap: 12px;
}

header > div {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.size {
  font-size: 11px;
  color: var(--muted);
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

.hint {
  margin: 0;
  padding: 8px 12px 12px;
  font-size: 11px;
  color: var(--muted);
  text-align: center;
}

:deep(.canvas-host) {
  height: 360px;
}
</style>
