import { onMounted, onUnmounted, ref, watch } from "vue";

export type ThemeMode = "system" | "light" | "dark";

const THEME_KEY = "ya-theme";

function resolveTheme(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  }
  return mode;
}

function applyTheme(mode: ThemeMode) {
  const resolved = resolveTheme(mode);
  document.documentElement.dataset.theme = resolved;
  document.documentElement.dataset.themeMode = mode;
}

export function useTheme() {
  const mode = ref<ThemeMode>(
    (localStorage.getItem(THEME_KEY) as ThemeMode) || "system",
  );

  let media: MediaQueryList | null = null;

  function onSystemChange() {
    if (mode.value === "system") applyTheme("system");
  }

  onMounted(() => {
    applyTheme(mode.value);
    media = window.matchMedia("(prefers-color-scheme: dark)");
    media.addEventListener("change", onSystemChange);
  });

  onUnmounted(() => {
    media?.removeEventListener("change", onSystemChange);
  });

  watch(
    mode,
    (v) => {
      localStorage.setItem(THEME_KEY, v);
      applyTheme(v);
    },
    { immediate: true },
  );

  return { mode };
}
