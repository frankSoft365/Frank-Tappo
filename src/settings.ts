import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { applyTranslations, resolveLanguage, translate } from "./i18n";
import type { Language, LanguageSetting } from "./i18n";
import { DEFAULT_SKIN_ID, listSkins } from "./skins";
import type { SkinPack } from "./skins";

const isTauri = "__TAURI_INTERNALS__" in window;
if (import.meta.env.DEV) {
  document.documentElement.classList.add("dev-build");
}
const DEFAULT_SCALE = 0.4;

// The window gets hidden while the cursor still rests on the close button, and
// WebKit keeps that hover state frozen until the pointer moves again; pin the
// button at rest on every show until a real mousemove re-resolves it.
const root = document.documentElement;
const armPointerIdle = () => root.classList.add("pointer-idle");
root.addEventListener("mousemove", () => root.classList.remove("pointer-idle"), {
  passive: true,
});
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") {
    armPointerIdle();
  }
});
armPointerIdle();

type SettingsView = {
  language: LanguageSetting;
  resolved: Language;
  skin: string;
  scale: number;
  scale_min: number;
  scale_max: number;
};

type PageId = "general" | "characters";

const inputs = Array.from(
  document.querySelectorAll<HTMLInputElement>('input[name="language"]'),
);
const scaleInput = document.querySelector<HTMLInputElement>("#scale")!;
const scaleValue = document.querySelector<HTMLElement>("#scale-value")!;
const closeButton = document.querySelector<HTMLButtonElement>("#close")!;
const licensesModal = document.querySelector<HTMLDivElement>("#licenses-modal")!;
const licensesClose = document.querySelector<HTMLButtonElement>("#licenses-close")!;
const licensesPanel = licensesModal.querySelector<HTMLDivElement>(".modal-panel")!;
const licenseRows = document.querySelector<HTMLTableSectionElement>("#license-rows")!;
const openLicensesLink = document.querySelector<HTMLButtonElement>("#open-licenses")!;
const aboutModal = document.querySelector<HTMLDivElement>("#about-modal")!;
const aboutClose = document.querySelector<HTMLButtonElement>("#about-close")!;
const aboutPanel = aboutModal.querySelector<HTMLDivElement>(".modal-panel")!;
const openAboutLink = document.querySelector<HTMLButtonElement>("#open-about")!;
const tabs = Array.from(document.querySelectorAll<HTMLButtonElement>(".tab"));
const pages: Record<PageId, HTMLElement> = {
  general: document.querySelector<HTMLElement>("#page-general")!,
  characters: document.querySelector<HTMLElement>("#page-characters")!,
};
const characterList = document.querySelector<HTMLElement>("#character-list")!;
const characterPreview = document.querySelector<SVGImageElement>("#character-preview")!;
const characterPreviewSvg = document.querySelector<SVGSVGElement>("#character-preview-svg")!;
const menuItems = Array.from(document.querySelectorAll<HTMLButtonElement>(".menu-item"));
const menuPages: Record<string, HTMLElement> = {
  characters: document.querySelector<HTMLElement>("#menu-page-characters")!,
  accessories: document.querySelector<HTMLElement>("#menu-page-accessories")!,
};

let current: SettingsView = {
  language: "system",
  resolved: "en",
  skin: DEFAULT_SKIN_ID,
  scale: DEFAULT_SCALE,
  scale_min: 0.24,
  scale_max: 0.6,
};
let previewFrame: number | undefined;
let pendingScale: { scale: number; save: boolean } | undefined;
let scaleInFlight = false;
let scaleEditing = false;

function percentage(scale: number) {
  return Math.round((scale / DEFAULT_SCALE) * 100);
}

function percent(scale: number) {
  return `${percentage(scale)}%`;
}

function updateScaleProgress() {
  const min = Number(scaleInput.min);
  const max = Number(scaleInput.max);
  const ratio = (scaleInput.valueAsNumber - min) / (max - min || 1);
  scaleInput.style.setProperty("--scale-progress", `${ratio * 100}%`);
}

function switchPage(page: PageId) {
  for (const tab of tabs) {
    const active = tab.dataset.page === page;
    tab.setAttribute("aria-selected", String(active));
    tab.tabIndex = active ? 0 : -1;
  }
  for (const [id, panel] of Object.entries(pages) as [PageId, HTMLElement][]) {
    panel.hidden = id !== page;
  }
}

function switchMenu(menu: string) {
  for (const item of menuItems) {
    const active = item.dataset.menu === menu;
    item.setAttribute("aria-selected", String(active));
    item.tabIndex = active ? 0 : -1;
  }
  for (const [id, panel] of Object.entries(menuPages)) {
    panel.hidden = id !== menu;
  }
}

function characterName(pack: SkinPack, language: Language) {
  const { zh, en } = pack.manifest.name;
  if (language === "zh" && zh) return zh;
  return en || zh || pack.id;
}

// Mirrors applySkin's fallback order so the badge marks the pack the pet actually shows.
function effectiveSkinId(skin: string) {
  const packs = listSkins();
  if (packs.some((pack) => pack.id === skin)) return skin;
  if (packs.some((pack) => pack.id === DEFAULT_SKIN_ID)) return DEFAULT_SKIN_ID;
  return packs[0]?.id;
}

function renderCharacters() {
  const active = effectiveSkinId(current.skin);
  const packs = [...listSkins()].sort(
    (a, b) =>
      Number(b.id === DEFAULT_SKIN_ID) - Number(a.id === DEFAULT_SKIN_ID) ||
      a.id.localeCompare(b.id),
  );
  const activePack = packs.find((pack) => pack.id === active) ?? packs[0];
  if (activePack) {
    characterPreview.setAttribute("href", activePack.frames["up|open"]);
    characterPreviewSvg.setAttribute("aria-label", characterName(activePack, current.resolved));
  } else {
    characterPreview.removeAttribute("href");
    characterPreviewSvg.setAttribute("aria-label", "");
  }
  characterList.replaceChildren(
    ...packs.map((pack) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "character";
      button.setAttribute("aria-pressed", String(pack.id === active));
      button.addEventListener("click", () => void chooseSkin(pack.id));

      const art = document.createElement("img");
      art.className = "character-art";
      art.src = pack.frames["up|open"];
      art.alt = "";

      const name = characterName(pack, current.resolved);
      const label = document.createElement("span");
      label.className = "character-name";
      label.textContent = name;
      label.title = name;

      button.append(art, label);
      return button;
    }),
  );
}

async function chooseSkin(skin: string) {
  if (skin === current.skin) return;
  if (!isTauri) {
    current = { ...current, skin };
    renderCharacters();
    return;
  }
  try {
    render(await invoke<SettingsView>("set_skin", { skin }));
  } catch (error) {
    console.warn("failed to set skin", error);
    renderCharacters();
  }
}

function render(view: SettingsView) {
  current = view;
  applyTranslations(document, view.resolved);
  document.title = translate(view.resolved, "settings.title");
  closeButton.setAttribute("aria-label", translate(view.resolved, "settings.close"));
  closeButton.title = translate(view.resolved, "settings.close");
  licensesClose.setAttribute("aria-label", translate(view.resolved, "settings.close"));
  licensesClose.title = translate(view.resolved, "settings.close");
  aboutClose.setAttribute("aria-label", translate(view.resolved, "settings.close"));
  aboutClose.title = translate(view.resolved, "settings.close");
  for (const input of inputs) {
    input.checked = input.value === view.language;
  }

  scaleInput.min = String(percentage(view.scale_min));
  scaleInput.max = String(percentage(view.scale_max));
  if (!scaleEditing) {
    scaleInput.value = String(percentage(view.scale));
    scaleValue.textContent = percent(view.scale);
    updateScaleProgress();
  }

  renderCharacters();
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

function scheduleScale() {
  if (scaleInFlight || previewFrame !== undefined || !pendingScale) {
    return;
  }
  if (pendingScale.save) {
    void flushScale();
  } else {
    previewFrame = requestAnimationFrame(() => {
      previewFrame = undefined;
      void flushScale();
    });
  }
}

async function flushScale() {
  if (scaleInFlight || !pendingScale) {
    return;
  }
  const request = pendingScale;
  pendingScale = undefined;
  scaleInFlight = true;
  try {
    if (request.save) {
      const view = await invoke<SettingsView>("set_scale", { scale: request.scale });
      if (!pendingScale) {
        scaleEditing = false;
      }
      render(view);
    } else {
      await invoke("preview_scale", { scale: request.scale });
    }
  } catch (error) {
    console.warn("failed to update scale", error);
    if (!pendingScale) {
      scaleEditing = false;
      render(current);
    }
  } finally {
    scaleInFlight = false;
    scheduleScale();
  }
}

function previewScale() {
  const scale = (scaleInput.valueAsNumber * DEFAULT_SCALE) / 100;
  scaleValue.textContent = percent(scale);
  updateScaleProgress();
  if (!isTauri) {
    return;
  }
  scaleEditing = true;
  pendingScale = { scale, save: false };
  scheduleScale();
}

function saveScale() {
  const scale = (scaleInput.valueAsNumber * DEFAULT_SCALE) / 100;
  if (!isTauri) {
    render({ ...current, scale });
    return;
  }
  scaleEditing = true;
  pendingScale = { scale, save: true };
  if (previewFrame !== undefined) {
    cancelAnimationFrame(previewFrame);
    previewFrame = undefined;
  }
  scheduleScale();
}

for (const input of inputs) {
  input.addEventListener("change", () => {
    if (input.checked) {
      void changeLanguage(input.value as LanguageSetting);
    }
  });
}

for (const tab of tabs) {
  tab.addEventListener("click", () => switchPage(tab.dataset.page as PageId));
  tab.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    event.preventDefault();
    const index = tabs.indexOf(tab);
    const next = tabs[(index + (event.key === "ArrowDown" ? 1 : tabs.length - 1)) % tabs.length];
    next.focus();
    switchPage(next.dataset.page as PageId);
  });
}

for (const item of menuItems) {
  item.addEventListener("click", () => switchMenu(item.dataset.menu ?? "characters"));
  item.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const index = menuItems.indexOf(item);
    const next =
      menuItems[
        (index + (event.key === "ArrowRight" ? 1 : menuItems.length - 1)) % menuItems.length
      ];
    next.focus();
    switchMenu(next.dataset.menu ?? "characters");
  });
}

scaleInput.addEventListener("input", previewScale);
scaleInput.addEventListener("change", () => void saveScale());

closeButton.addEventListener("click", () => {
  if (isTauri) {
    void getCurrentWindow().close();
  }
});

const logsButton = document.querySelector<HTMLButtonElement>("#view-logs")!;
logsButton.addEventListener("click", () => {
  if (!isTauri) return;
  void invoke("reveal_log_file")
    .catch((error) => console.warn("failed to reveal logs", error))
    .finally(() => void getCurrentWindow().close());
});

const quitButton = document.querySelector<HTMLButtonElement>("#quit-app")!;
quitButton.addEventListener("click", () => {
  if (!isTauri) return;
  void invoke("quit_app").catch((error) => console.warn("failed to quit app", error));
});

const emailLink = document.querySelector<HTMLAnchorElement>(".legal-email a")!;
emailLink.addEventListener("click", (event) => {
  if (!isTauri) return;
  event.preventDefault();
  void invoke("open_author_email").catch((error) => console.warn("failed to open email", error));
});

const LICENSES: ReadonlyArray<readonly [string, string]> = [
  ["@tauri-apps/api", "(Apache-2.0 OR MIT)"],
  ["tauri", "(Apache-2.0 OR MIT)"],
  ["serde", "(MIT OR Apache-2.0)"],
  ["serde_json", "(MIT OR Apache-2.0)"],
  ["sys-locale", "(MIT OR Apache-2.0)"],
  ["gilrs", "(Apache-2.0 OR MIT)"],
  ["rdev", "MIT"],
  ["objc2-app-kit", "(Zlib OR Apache-2.0 OR MIT)"],
  ["objc2-core-graphics", "(Zlib OR Apache-2.0 OR MIT)"],
  ["objc2-foundation", "MIT"],
  ["windows-sys", "(MIT OR Apache-2.0)"],
];

licenseRows.replaceChildren(
  ...LICENSES.map(([name, license]) => {
    const row = document.createElement("tr");
    const nameCell = document.createElement("td");
    nameCell.className = "license-name";
    nameCell.textContent = name;
    const colonCell = document.createElement("td");
    colonCell.className = "license-colon";
    colonCell.textContent = ":";
    const licenseCell = document.createElement("td");
    licenseCell.className = "license-text";
    licenseCell.textContent = license;
    row.append(nameCell, colonCell, licenseCell);
    return row;
  }),
);

function openLicenses() {
  licensesModal.hidden = false;
  licensesPanel.focus();
}

function closeLicenses() {
  licensesModal.hidden = true;
}

function openAbout() {
  aboutModal.hidden = false;
  aboutPanel.focus();
}

function closeAbout() {
  aboutModal.hidden = true;
}

openLicensesLink.addEventListener("click", openLicenses);
licensesClose.addEventListener("click", closeLicenses);
licensesModal.addEventListener("click", (event) => {
  if (event.target === licensesModal) {
    closeLicenses();
  }
});
openAboutLink.addEventListener("click", openAbout);
aboutClose.addEventListener("click", closeAbout);
aboutModal.addEventListener("click", (event) => {
  if (event.target === aboutModal) {
    closeAbout();
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!licensesModal.hidden) closeLicenses();
  if (!aboutModal.hidden) closeAbout();
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
  await listen<string>("settings:navigate", (event) => {
    const page = event.payload;
    if (page === "general" || page === "characters") switchPage(page);
  });
  void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (focused) {
      armPointerIdle();
    }
  });
}

void init();
