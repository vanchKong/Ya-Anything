<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { DRACOLoader } from "three/examples/jsm/loaders/DRACOLoader.js";
import { RoomEnvironment } from "three/examples/jsm/environments/RoomEnvironment.js";
import { MeshoptDecoder } from "meshoptimizer";

/** 对比同步用：球坐标方位 + 相对初始取景的缩放 */
export interface OrbitSyncState {
  theta: number;
  phi: number;
  radiusScale: number;
}

const props = defineProps<{
  bytes: Uint8Array;
  label?: string;
  /** 来自对侧的同步方位；本侧交互驱动时不要回写同值 */
  orbitSync?: OrbitSyncState | null;
}>();

const emit = defineEmits<{
  "orbit-change": [state: OrbitSyncState];
}>();

const canvasHost = ref<HTMLDivElement | null>(null);
const error = ref("");
const animInfo = ref("");

let renderer: THREE.WebGLRenderer | null = null;
let animationId = 0;
let controls: OrbitControls | null = null;
let camera: THREE.PerspectiveCamera | null = null;
let resizeHandler: (() => void) | null = null;
let mixer: THREE.AnimationMixer | null = null;
let clock: THREE.Clock | null = null;
let baseRadius = 1;
let applyingSync = false;
let isOrbitDriver = false;
let clearDriverTimer = 0;

function readOrbitState(): OrbitSyncState | null {
  if (!camera || !controls) return null;
  const offset = camera.position.clone().sub(controls.target);
  const spherical = new THREE.Spherical().setFromVector3(offset);
  return {
    theta: spherical.theta,
    phi: spherical.phi,
    radiusScale: spherical.radius / (baseRadius || 1),
  };
}

function applyOrbitState(state: OrbitSyncState) {
  if (!camera || !controls) return;
  applyingSync = true;
  const spherical = new THREE.Spherical(
    Math.max(1e-4, baseRadius * state.radiusScale),
    state.phi,
    state.theta,
  );
  const offset = new THREE.Vector3().setFromSpherical(spherical);
  camera.position.copy(controls.target).add(offset);
  controls.update();
  applyingSync = false;
}

function markOrbitDriver() {
  isOrbitDriver = true;
  if (clearDriverTimer) {
    window.clearTimeout(clearDriverTimer);
    clearDriverTimer = 0;
  }
}

function onControlsStart() {
  markOrbitDriver();
}

function onControlsEnd() {
  // 阻尼还会继续几帧，稍后再放开 driver，保证对侧跟完
  if (clearDriverTimer) window.clearTimeout(clearDriverTimer);
  clearDriverTimer = window.setTimeout(() => {
    isOrbitDriver = false;
    clearDriverTimer = 0;
  }, 320);
}

function onDomWheel() {
  // 滚轮缩放不触发 controls start/end，需单独占位 driver
  markOrbitDriver();
  onControlsEnd();
}

function onControlsChange() {
  if (applyingSync) return;
  // 仅交互侧向外同步，避免初始取景/对侧回写互相抢占
  if (!isOrbitDriver) return;
  const state = readOrbitState();
  if (state) emit("orbit-change", state);
}

function dispose() {
  cancelAnimationFrame(animationId);
  animationId = 0;
  if (clearDriverTimer) {
    window.clearTimeout(clearDriverTimer);
    clearDriverTimer = 0;
  }
  if (resizeHandler) {
    window.removeEventListener("resize", resizeHandler);
    resizeHandler = null;
  }
  if (controls) {
    controls.removeEventListener("start", onControlsStart);
    controls.removeEventListener("end", onControlsEnd);
    controls.removeEventListener("change", onControlsChange);
    controls.dispose();
    controls = null;
  }
  mixer?.stopAllAction();
  mixer = null;
  clock = null;
  camera = null;
  if (renderer) {
    renderer.domElement.removeEventListener("wheel", onDomWheel);
    renderer.dispose();
    renderer = null;
  }
  if (canvasHost.value) canvasHost.value.innerHTML = "";
}

async function mountScene() {
  const host = canvasHost.value;
  if (!host) return;
  dispose();
  error.value = "";
  animInfo.value = "";
  isOrbitDriver = false;

  const width = host.clientWidth || 260;
  const height = host.clientHeight || 260;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x1d2420);

  camera = new THREE.PerspectiveCamera(45, width / height, 0.01, 2000);
  camera.position.set(1.6, 1.2, 2.2);

  renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.setSize(width, height);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  host.appendChild(renderer.domElement);

  const pmrem = new THREE.PMREMGenerator(renderer);
  scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;

  controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  controls.addEventListener("start", onControlsStart);
  controls.addEventListener("end", onControlsEnd);
  controls.addEventListener("change", onControlsChange);
  renderer.domElement.addEventListener("wheel", onDomWheel, { passive: true });
  scene.add(new THREE.HemisphereLight(0xffffff, 0x444444, 1.1));

  const dracoLoader = new DRACOLoader();
  dracoLoader.setDecoderPath("/draco-three/");
  const loader = new GLTFLoader();
  loader.setDRACOLoader(dracoLoader);
  await MeshoptDecoder.ready;
  loader.setMeshoptDecoder(MeshoptDecoder);

  try {
    const copy = props.bytes.slice();
    const gltf = await loader.parseAsync(copy.buffer, "");
    const root = gltf.scene;
    scene.add(root);

    const box = new THREE.Box3().setFromObject(root);
    const size = box.getSize(new THREE.Vector3());
    const center = box.getCenter(new THREE.Vector3());
    root.position.sub(center);

    const maxDim = Math.max(size.x, size.y, size.z) || 1;
    const dist = maxDim * 2.2;
    camera.position.set(dist, dist * 0.7, dist);
    camera.near = maxDim / 100;
    camera.far = maxDim * 100;
    camera.updateProjectionMatrix();
    controls.target.set(0, 0, 0);
    controls.update();
    baseRadius = camera.position.distanceTo(controls.target) || dist;

    if (props.orbitSync) {
      applyOrbitState(props.orbitSync);
    }

    if (gltf.animations?.length) {
      mixer = new THREE.AnimationMixer(root);
      clock = new THREE.Clock();
      for (const clip of gltf.animations) {
        mixer.clipAction(clip).play();
      }
      animInfo.value = `动画 ${gltf.animations.length} 条 · 播放中`;
    } else {
      animInfo.value = "无动画";
    }
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  }

  resizeHandler = () => {
    if (!renderer || !host || !camera) return;
    const w = host.clientWidth;
    const h = host.clientHeight;
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
    renderer.setSize(w, h);
  };
  window.addEventListener("resize", resizeHandler);

  const tick = () => {
    animationId = requestAnimationFrame(tick);
    const dt = clock?.getDelta() ?? 0;
    mixer?.update(dt);
    controls?.update();
    if (renderer && camera) renderer.render(scene, camera);
  };
  tick();
}

onMounted(() => {
  void mountScene();
});

onUnmounted(() => {
  dispose();
});

watch(
  () => props.bytes,
  () => {
    void mountScene();
  },
);

watch(
  () => props.orbitSync,
  (state) => {
    if (!state || isOrbitDriver) return;
    applyOrbitState(state);
  },
);
</script>

<template>
  <div class="viewer">
    <div v-if="label" class="label">{{ label }}</div>
    <div ref="canvasHost" class="canvas-host" />
    <p v-if="error" class="error">{{ error }}</p>
    <p v-else class="meta">{{ animInfo }}</p>
  </div>
</template>

<style scoped>
.viewer {
  position: relative;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: #1d2420;
  border-radius: 10px;
  overflow: hidden;
}

.label {
  position: absolute;
  top: 8px;
  left: 8px;
  z-index: 2;
  padding: 3px 8px;
  border-radius: 999px;
  font-size: 11px;
  font-weight: 700;
  color: #fff;
  background: rgba(0, 0, 0, 0.55);
}

.canvas-host {
  width: 100%;
  height: 260px;
}

.error,
.meta {
  margin: 0;
  padding: 6px 10px;
  font-size: 11px;
  color: #c9d2cb;
}

.error {
  color: #f0a0a0;
}
</style>
