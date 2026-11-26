export type LocaleCode = "zh" | "en" | "ja" | "ko" | "ru";
export type LocalePreference = "system" | LocaleCode;

/** 国际品牌名（安装包 / GitHub 用） */
export const BRAND_LATIN = "YaYa";

export type MessageTree = {
  brand: string;
  taglineImage: string;
  taglineModel: string;
  openRepo: string;
  kindImage: string;
  kindModel: string;
  footerOutput: string;
  theme: string;
  themeSystem: string;
  themeLight: string;
  themeDark: string;
  language: string;
  langSystem: string;
  langZh: string;
  langEn: string;
  langJa: string;
  langKo: string;
  langRu: string;

  common: {
    undo: string;
    selectAll: string;
    clear: string;
    close: string;
    pickFiles: string;
    pickFolder: string;
    folder: string;
    failed: string;
    restored: string;
    bestAlready: string;
    processing: string;
    processingNamed: string;
  };

  image: {
    quality: string;
    qualityTip: string;
    resize: string;
    resizeTip: string;
    byWidth: string;
    byHeight: string;
    byPercent: string;
    widthPx: string;
    heightPx: string;
    percentLabel: string;
    lockAspect: string;
    lockAspectTip: string;
    zoneCompress: string;
    zoneCompressDesc: string;
    zoneResize: string;
    zoneResizeDesc: string;
    reprocess: string;
    reprocessTip: string;
    addCompress: string;
    addResize: string;
    tip: string;
    undoTip: string;
    statusCompressing: string;
    statusResizing: string;
    statusResizedDown: string;
    statusResizedUp: string;
    statusSaved: string;
    gifTitle: string;
    gifPreset: string;
    gifPresetTip: string;
    gifPresetDefault: string;
    gifPresetLight: string;
    gifPresetBalanced: string;
    gifPresetAggressive: string;
    gifPresetCustom: string;
    gifFps: string;
    gifFpsTip: string;
    gifFpsOriginal: string;
    gifColors: string;
    gifColorsTip: string;
    gifDither: string;
    gifDitherTip: string;
    gifDitherBayer: string;
    gifDitherFs: string;
    gifDitherNone: string;
    gifMaxWidth: string;
    gifMaxWidthTip: string;
    gifNoScale: string;
  };

  model: {
    preset: string;
    presetTip: string;
    presetMax: string;
    presetCompact: string;
    presetBalanced: string;
    presetFast: string;
    presetCustom: string;
    meshCompress: string;
    meshCompressTip: string;
    meshNone: string;
    dracoSpeed: string;
    dracoSpeedTip: string;
    dracoPrecision: string;
    dracoPrecisionTip: string;
    precisionLow: string;
    precisionMedium: string;
    precisionHigh: string;
    meshoptLevel: string;
    levelMedium: string;
    levelHigh: string;
    quantize: string;
    quantizeTip: string;
    dedup: string;
    dedupTip: string;
    weld: string;
    weldTip: string;
    resample: string;
    resampleTip: string;
    prune: string;
    pruneTip: string;
    joinMeshes: string;
    joinMeshesTip: string;
    simplify: string;
    simplifyTip: string;
    simplifyRatio: string;
    textureMaxSize: string;
    textureMaxSizeTip: string;
    noScale: string;
    textureFormat: string;
    textureFormatTip: string;
    formatKeep: string;
    textureEncoder: string;
    textureEncoderTip: string;
    encoderFast: string;
    encoderStrong: string;
    textureQuality: string;
    textureQualityTip: string;
    zonePreview: string;
    zonePreviewDesc: string;
    zoneCompress: string;
    zoneCompressDesc: string;
    compare: string;
    compareTip: string;
    recompress: string;
    add: string;
    tip: string;
    undoTip: string;
    statusCompressing: string;
    statusSaved: string;
    compressingNamed: string;
    origin: string;
    compressed: string;
    compareHint: string;
    previewHint: string;
  };

  compare: {
    after: string;
    before: string;
    hint: string;
  };

  output: {
    title: string;
    overwrite: string;
    overwriteDesc: string;
    newfile: string;
    newfileDesc: string;
    custom: string;
    customDesc: string;
    pickDir: string;
    noDirTip: string;
  };
};

export const LOCALE_CODES: LocaleCode[] = ["zh", "en", "ja", "ko", "ru"];

export function detectSystemLocale(): LocaleCode {
  const lang = (navigator.language || "en").toLowerCase();
  if (lang.startsWith("zh")) return "zh";
  if (lang.startsWith("ja")) return "ja";
  if (lang.startsWith("ko")) return "ko";
  if (lang.startsWith("ru")) return "ru";
  return "en";
}

export function resolveLocale(pref: LocalePreference): LocaleCode {
  return pref === "system" ? detectSystemLocale() : pref;
}
