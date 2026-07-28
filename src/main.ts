import { createApp } from "vue";
import App from "./App.vue";
import "./styles.css";

type ThemeMode = "system" | "light" | "dark";

function resolveTheme(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  }
  return mode;
}

const saved = (localStorage.getItem("ya-theme") as ThemeMode) || "system";
const resolved = resolveTheme(saved);
document.documentElement.dataset.theme = resolved;
document.documentElement.dataset.themeMode = saved;

createApp(App).mount("#app");
