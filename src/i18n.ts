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
    "counter.total": "累计点击",
    "settings.title": "更多",
    "settings.toggle": "更多",
    "settings.tab.general": "设置",
    "settings.tab.characters": "角色",
    "settings.menu.characters": "角色",
    "settings.menu.accessories": "配件",
    "settings.accessoriesEmpty": "配件列表暂无",
    "settings.language": "语言",
    "settings.system": "跟随系统",
    "settings.size": "大小",
    "settings.close": "关闭",
    "settings.viewLogs": "查看日志",
    "settings.quitApp": "退出应用",
    "settings.openSourceLicenses": "开源组件声明",
    "settings.about": "关于 Frank Tappo",
    "settings.authorEmail": "作者邮箱",
    "about.lead": "用与 Wuwa Tappo 相同的技术路线，做了一个 macOS 版本的简单实现。",
    "about.p1": "你敲键盘的时候，它会在角落里轻轻跟着敲；安静下来时，就眨眨眼，等你回来。",
    "about.p2": "这是一个粉丝作品，并非官方版本。我们和你一样，期待 Wuwa Tappo 官方 macOS 版本上线。",
    "about.p3": "作者借助 Qoder 一起探索了桌宠的实现方式——像素帧怎么对齐、眨眼和挥手怎么接上键盘、窗口怎么悄悄跟随，还有系统权限、打包和签名这些看不见的细节。",
    "about.made": "Made with Qoder",
    "language.zh": "中文",
    "language.en": "English",
  },
  en: {
    "hint.main": "Type anything — Tappo taps along",
    "banner.message": "Allow Tappo in Input Monitoring so it can feel your typing.",
    "banner.button": "Open Settings",
    "badge.clickThrough": "Click-through",
    "counter.total": "Total taps",
    "settings.title": "More",
    "settings.toggle": "More",
    "settings.tab.general": "Settings",
    "settings.tab.characters": "Characters",
    "settings.menu.characters": "Characters",
    "settings.menu.accessories": "Accessories",
    "settings.accessoriesEmpty": "No accessories yet",
    "settings.language": "Language",
    "settings.system": "Use system language",
    "settings.size": "Size",
    "settings.close": "Close",
    "settings.viewLogs": "View logs",
    "settings.quitApp": "Quit app",
    "settings.openSourceLicenses": "Open-source licenses",
    "settings.about": "About Frank Tappo",
    "settings.authorEmail": "Author email",
    "about.lead": "A simple macOS take on the same technical path as Wuwa Tappo.",
    "about.p1": "While you type, it taps along softly in the corner; when things go quiet, it blinks and waits for you to come back.",
    "about.p2": "This is a fan-made project, not an official release. Like you, we are looking forward to Wuwa Tappo's official macOS version.",
    "about.p3": "The author explored how desktop pets are made together with Qoder — aligning pixel frames, wiring blinks and waves to the keyboard, following the window quietly, plus the invisible details of permissions, packaging and code signing.",
    "about.made": "Made with Qoder",
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
