import { computed, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { MESSAGES } from "./messages";
import {
  detectSystemLocale,
  resolveLocale,
  type LocaleCode,
  type LocalePreference,
  type MessageTree,
} from "./types";
import { loadLocalePreference, saveLocalePreference } from "../lib/settings";

export type { LocaleCode, LocalePreference, MessageTree };
export { BRAND_LATIN, detectSystemLocale, resolveLocale } from "./types";

const preference = ref<LocalePreference>(loadLocalePreference());
const locale = ref<LocaleCode>(resolveLocale(preference.value));

function applyDocumentLang(code: LocaleCode) {
  document.documentElement.lang = code === "zh" ? "zh-CN" : code;
}

async function applyWindowTitle(brand: string) {
  try {
    await getCurrentWindow().setTitle(brand);
  } catch {
    // web preview / non-tauri
    document.title = brand;
  }
}

applyDocumentLang(locale.value);
void applyWindowTitle(MESSAGES[locale.value].brand);

watch(preference, (pref) => {
  saveLocalePreference(pref);
  locale.value = resolveLocale(pref);
});

watch(
  locale,
  (code) => {
    applyDocumentLang(code);
    void applyWindowTitle(MESSAGES[code].brand);
  },
  { immediate: false },
);

if (typeof window !== "undefined") {
  window.matchMedia("(prefers-color-scheme: dark)"); // keep parity with theme boot
  // 系统语言变化时，仅在「跟随系统」下刷新
  const onLangChange = () => {
    if (preference.value === "system") {
      locale.value = detectSystemLocale();
    }
  };
  window.addEventListener("languagechange", onLangChange);
}

type Vars = Record<string, string | number>;

function format(template: string, vars?: Vars): string {
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (_, key: string) =>
    vars[key] === undefined ? `{${key}}` : String(vars[key]),
  );
}

function getByPath(tree: MessageTree, path: string): string {
  const parts = path.split(".");
  let cur: unknown = tree;
  for (const p of parts) {
    if (cur && typeof cur === "object" && p in (cur as object)) {
      cur = (cur as Record<string, unknown>)[p];
    } else {
      return path;
    }
  }
  return typeof cur === "string" ? cur : path;
}

export function t(path: string, vars?: Vars): string {
  const primary = getByPath(MESSAGES[locale.value], path);
  const fallback =
    primary === path ? getByPath(MESSAGES.en, path) : primary;
  return format(fallback === path ? getByPath(MESSAGES.zh, path) : fallback, vars);
}

export function useI18n() {
  const messages = computed(() => MESSAGES[locale.value]);

  function tt(path: string, vars?: Vars) {
    // 依赖 locale，保证模板响应式
    void locale.value;
    return t(path, vars);
  }

  function setPreference(pref: LocalePreference) {
    preference.value = pref;
  }

  return {
    preference,
    locale,
    messages,
    t: tt,
    setPreference,
  };
}
