export type Language = "zh" | "en";
export type LanguageSetting = "system" | "zh" | "en";

export function resolveLanguage(setting: LanguageSetting): Language {
  if (setting === "zh" || setting === "en") {
    return setting;
  }
  return navigator.language.toLowerCase().startsWith("zh") ? "zh" : "en";
}

const messages: Record<Language, Record<string, string>> = {
  zh: {
    "hint.main": "敲敲键盘，Tappo 陪你敲",
    "banner.message": "需要在「输入监控」中允许 Tappo，才能感知键盘。",
    "banner.button": "去开启",
    "badge.clickThrough": "点击穿透",
    "settings.title": "设置",
    "settings.language": "语言",
    "settings.system": "跟随系统",
    "settings.systemHint": "使用系统语言",
    "settings.current": "当前：",
    "settings.languageNote": "默认跟随系统语言，可随时切换；切换后立即生效。",
    "settings.size": "大小",
    "settings.sizeHint": "拖动滑块即时预览，松手后保存；默认 75%。",
    "settings.done": "完成",
    "language.zh": "中文",
    "language.en": "English",
  },
  en: {
    "hint.main": "Type anything — Tappo taps along",
    "banner.message": "Allow Tappo in Input Monitoring so it can feel your typing.",
    "banner.button": "Open Settings",
    "badge.clickThrough": "Click-through",
    "settings.title": "Settings",
    "settings.language": "Language",
    "settings.system": "Use system language",
    "settings.systemHint": "Use system language",
    "settings.current": "Current: ",
    "settings.languageNote": "Defaults to your system language. Changes apply immediately.",
    "settings.size": "Size",
    "settings.sizeHint": "Drag to preview, release to save. Default 75%.",
    "settings.done": "Done",
    "language.zh": "中文",
    "language.en": "English",
  },
};

export function translate(language: Language, key: string): string {
  return messages[language][key] ?? key;
}

export function applyTranslations(root: ParentNode, language: Language) {
  document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((element) => {
    element.textContent = translate(language, element.dataset.i18n ?? "");
  });
}
