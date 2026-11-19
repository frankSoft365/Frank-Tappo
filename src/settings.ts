import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { applyTranslations, resolveLanguage, translate } from "./i18n";
import type { Language, LanguageSetting } from "./i18n";

const isTauri = "__TAURI_INTERNALS__" in window;

type SettingsView = {
  language: LanguageSetting;
  resolved: Language;
};

const inputs = Array.from(
  document.querySelectorAll<HTMLInputElement>('input[name="language"]'),
);
const systemHint = document.querySelector<HTMLElement>("#system-hint")!;
const doneButton = document.querySelector<HTMLButtonElement>("#done")!;

let current: SettingsView = { language: "system", resolved: "en" };

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
}

async function changeLanguage(language: LanguageSetting) {
  if (!isTauri) {
    render({ language, resolved: resolveLanguage(language) });
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

for (const input of inputs) {
  input.addEventListener("change", () => {
    if (input.checked) {
      void changeLanguage(input.value as LanguageSetting);
    }
  });
}

doneButton.addEventListener("click", () => {
  if (isTauri) {
    void getCurrentWindow().close();
  }
});

async function init() {
  if (!isTauri) {
    render({ language: "system", resolved: resolveLanguage("system") });
    return;
  }
  try {
    render(await invoke<SettingsView>("get_settings"));
  } catch (error) {
    console.warn("failed to load settings", error);
    render({ language: "system", resolved: "en" });
  }
  await listen<SettingsView>("settings:changed", (event) => render(event.payload));
}

void init();
