<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import ModelViewerCanvas, {
  type OrbitSyncState,
} from "./ModelViewerCanvas.vue";
import { formatSize } from "../types";
import { useI18n } from "../i18n";

const props = defineProps<{
  name: string;
  originBytes: Uint8Array;
  compressedBytes: Uint8Array;
}>();

const emit = defineEmits<{ close: [] }>();
const { t } = useI18n();
const orbitSync = ref<OrbitSyncState | null>(null);

const originSize = computed(() => props.originBytes.byteLength);
const compressedSize = computed(() => props.compressedBytes.byteLength);
const ratio = computed(() => {
  if (!originSize.value) return 0;
  return ((originSize.value - compressedSize.value) / originSize.value) * 100;
});
const remain = computed(() => {
  if (!originSize.value) return 100;
  return (compressedSize.value / originSize.value) * 100;
});

function onOrbitChange(state: OrbitSyncState) {
  orbitSync.value = state;
}

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
          <span class="stats">
            {{ formatSize(originSize) }} → {{ formatSize(compressedSize) }} ·
            {{ remain.toFixed(1) }}% · {{ ratio.toFixed(1) }}%
          </span>
        </div>
        <button type="button" class="close" @click="emit('close')">
          {{ t("common.close") }}
        </button>
      </header>

      <div class="grid">
        <ModelViewerCanvas
          :bytes="originBytes"
          :label="t('model.origin')"
          :orbit-sync="orbitSync"
          @orbit-change="onOrbitChange"
        />
        <ModelViewerCanvas
          :bytes="compressedBytes"
          :label="t('model.compressed')"
          :orbit-sync="orbitSync"
          @orbit-change="onOrbitChange"
        />
      </div>

      <p class="hint">{{ t("model.compareHint") }}</p>
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
  width: min(720px, 100%);
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
  gap: 12px;
}

header > div {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.stats {
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

.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  padding: 10px;
}

.slider-row {
  display: none;
}

.hint {
  margin: 0;
  padding: 0 12px 12px;
  font-size: 11px;
  color: var(--muted);
  text-align: center;
}
</style>
