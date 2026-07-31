<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import logo from "../assets/logo.png";
import { useI18n } from "../i18n";

const emit = defineEmits<{ done: [] }>();
const phase = ref<"in" | "out">("in");
const { t } = useI18n();

let zoomTimer = 0;
let doneTimer = 0;

onMounted(() => {
  zoomTimer = window.setTimeout(() => {
    phase.value = "out";
  }, 700);
  doneTimer = window.setTimeout(() => {
    emit("done");
  }, 1250);
});

onUnmounted(() => {
  window.clearTimeout(zoomTimer);
  window.clearTimeout(doneTimer);
});
</script>

<template>
  <div class="splash" :class="phase" aria-hidden="true">
    <img :src="logo" alt="" class="logo" />
    <p class="title">{{ t("brand") }}</p>
  </div>
</template>

<style scoped>
.splash {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: grid;
  place-content: center;
  justify-items: center;
  gap: 14px;
  background:
    radial-gradient(120% 80% at 0% 0%, var(--brand-soft), transparent 55%),
    radial-gradient(90% 60% at 100% 100%, var(--ok-soft), transparent 50%),
    var(--bg);
  color: var(--text);
  transition: opacity 0.45s ease, visibility 0.45s ease;
}

.splash.out {
  opacity: 0;
  visibility: hidden;
  pointer-events: none;
}

.logo {
  width: 112px;
  height: 112px;
  border-radius: 28px;
  object-fit: cover;
  transform: scale(0.88);
  filter: blur(0);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
  animation: splash-zoom 1.15s cubic-bezier(0.22, 1, 0.36, 1) forwards;
}

.title {
  margin: 0;
  color: var(--accent);
  font-size: 18px;
  font-weight: 800;
  letter-spacing: 0.18em;
  opacity: 0.95;
  animation: splash-fade 0.9s ease forwards;
}

@keyframes splash-zoom {
  0% {
    transform: scale(0.82);
    filter: blur(0);
    opacity: 1;
  }
  55% {
    transform: scale(1.08);
    filter: blur(0);
    opacity: 1;
  }
  100% {
    transform: scale(1.55);
    filter: blur(14px);
    opacity: 0;
  }
}

@keyframes splash-fade {
  0% {
    opacity: 0;
    transform: translateY(6px);
  }
  35% {
    opacity: 1;
    transform: translateY(0);
  }
  100% {
    opacity: 0;
    transform: translateY(-4px);
  }
}
</style>
