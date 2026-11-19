import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { applyTranslations, resolveLanguage, translate } from "./i18n";
import type { Language, LanguageSetting } from "./i18n";

const isTauri = "__TAURI_INTERNALS__" in window;

type SettingsView = {
  language: LanguageSetting;
  resolved: Language;
  scale: number;
  scale_min: number;
  scale_max: number;
};

const inputs = Array.from(
  document.querySelectorAll<HTMLInputElement>('input[name="language"]'),
);
const systemHint = document.querySelector<HTMLElement>("#system-hint")!;
const scaleInput = document.querySelector<HTMLInputElement>("#scale")!;
const scaleValue = document.querySelector<HTMLElement>("#scale-value")!;
const doneButton = document.querySelector<HTMLButtonElement>("#done")!;

let current: SettingsView = {
  language: "system",
  resolved: "en",
  scale: 0.75,
  scale_min: 0.5,
  scale_max: 1.5,
};
let previewFrame: number | undefined;
let pendingScale: number | undefined;

function percent(scale: number) {
  return `${Math.round(scale * 100)}%`;
}

function render(view: SettingsView) {
  current = view;
  applyTranslations(document, view.resolved);
  document.title = translate(view.resolved, "settings.title");
  for (const input of inputs) {
    input.checked = input.value === view.language;
  }
  const resolvedName = translate(
    view.resolved,
    view.resolved === "zh" ? "language.zh" : "language.en",
  );
  systemHint.textContent = `${translate(view.resolved, "settings.systemHint")} · ${translate(
    view.resolved,
    "settings.current",
  )}${resolvedName}`;

  scaleInput.min = String(Math.round(view.scale_min * 100));
  scaleInput.max = String(Math.round(view.scale_max * 100));
  scaleInput.value = String(Math.round(view.scale * 100));
  scaleValue.textContent = percent(view.scale);
}

async function changeLanguage(language: LanguageSetting) {
  if (!isTauri) {
    render({ ...current, language, resolved: resolveLanguage(language) });
    return;
  }
  try {
    const view = await invoke<SettingsView>("set_language", { language });
    render(view);
  } catch (error) {
    console.warn("failed to set language", error);
    render(current);
  }
}

function previewScale() {
  const scale = scaleInput.valueAsNumber / 100;
  scaleValue.textContent = percent(scale);
  if (!isTauri) {
    return;
  }
  pendingScale = scale;
  if (previewFrame !== undefined) {
    return;
  }
  previewFrame = requestAnimationFrame(() => {
    previewFrame = undefined;
    const value = pendingScale;
    if (value !== undefined) {
      void invoke("preview_scale", { scale: value });
    }
  });
}

async function saveScale() {
  if (!isTauri) {
    return;
  }
  try {
    render(await invoke<SettingsView>("set_scale", { scale: scaleInput.valueAsNumber / 100 }));
  } catch (error) {
    console.warn("failed to set scale", error);
    render(current);
  }
}

for (const input of inputs) {
  input.addEventListener("change", () => {
    if (input.checked) {
      void changeLanguage(input.value as LanguageSetting);
    }
  });
}

scaleInput.addEventListener("input", previewScale);
scaleInput.addEventListener("change", () => void saveScale());

doneButton.addEventListener("click", () => {
  if (isTauri) {
    void getCurrentWindow().close();
  }
});

async function init() {
  if (!isTauri) {
    render({ ...current, language: "system", resolved: resolveLanguage("system") });
    return;
  }
  try {
    render(await invoke<SettingsView>("get_settings"));
  } catch (error) {
    console.warn("failed to load settings", error);
    render(current);
  }
  await listen<SettingsView>("settings:changed", (event) => render(event.payload));
}

void init();
