<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import type { PreviewResult } from "../types";

const props = defineProps<{
  name: string;
  preview: PreviewResult;
}>();

const emit = defineEmits<{ close: [] }>();
const position = ref(50);
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
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("resize", measure);
});
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="panel" role="dialog" aria-modal="true">
      <header>
        <strong>{{ name }}</strong>
        <button type="button" class="close" @click="emit('close')">关闭</button>
      </header>

      <p v-if="sameImage" class="note">当前已是最佳效果，前后图一致</p>

      <div
        ref="box"
        class="compare"
        @pointermove="onPointerMove"
        @pointerenter="onPointerMove"
      >
        <img class="layer" :src="compressedUrl" alt="压缩后" />
        <div class="clip" :style="{ width: `${position}%` }">
          <img
            class="layer"
            :src="originUrl"
            alt="原图"
            :style="{ width: boxWidth ? `${boxWidth}px` : '100%' }"
          />
        </div>
        <span class="tag left">原图</span>
        <span class="tag right">压缩后</span>
      </div>
      <p class="hint">移动鼠标对比前后效果</p>
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

.hint {
  margin: 0 0 12px;
  padding: 0 12px;
  font-size: 11px;
  color: var(--muted);
  text-align: center;
}
</style>
