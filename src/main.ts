import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { applyTranslations, resolveLanguage, translate } from "./i18n";
import type { Language, LanguageSetting } from "./i18n";
import { initAccessories, equipAccessory } from "./accessories";
import { applySkin, DEFAULT_SKIN_ID } from "./skins";

const isTauri = "__TAURI_INTERNALS__" in window;
if (import.meta.env.DEV) {
  document.documentElement.classList.add("dev-build");
}
// Dev override, e.g. /?skin=moning — wins over the saved setting for this page load.
const skinOverride = new URLSearchParams(location.search).get("skin");
// Dev override, e.g. /?accessory=dragon — accessories are off by default.
const accessoryOverride = new URLSearchParams(location.search).get("accessory");

const banner = document.querySelector<HTMLElement>("#banner")!;
const bannerBtn = document.querySelector<HTMLButtonElement>("#banner-btn")!;
const badge = document.querySelector<HTMLElement>("#badge")!;
const settingsToggle = document.querySelector<HTMLButtonElement>("#settings-toggle")!;

let permissionTimer: number | undefined;
let settingsPreview: Window | null = null;
let focusRequest: Promise<void> | undefined;

const stage = document.querySelector<HTMLElement>("#stage")!;
const impact = document.querySelector<SVGSVGElement>("#tap-impact")!;
const counter = document.querySelector<HTMLElement>("#tap-count")!;

// Never show an empty stage: apply the fallback pack while the saved setting loads.
initAccessories();
void applySkin(skinOverride ?? DEFAULT_SKIN_ID);

let impactAnimation: Animation | undefined;
let armTimer: number | undefined;
let armPending = false;
let total = 0n;
let counterLanguage = resolveLanguage("system");
let countFormatter = new Intl.NumberFormat(counterLanguage, { useGrouping: false });

function renderCount() {
  const text = countFormatter.format(total);
  counter.textContent = text;
  counter.style.setProperty("--counter-font-size", String(Math.min(32, 400 / text.length)));
  const label = `${translate(counterLanguage, "counter.total")}: ${text}`;
  counter.title = label;
  counter.setAttribute("aria-label", label);
}

function updateCount(value: string) {
  const next = BigInt(value);
  if (next < total) return;
  total = next;
  renderCount();
}

const ARM_DOWN_MS = 200;
const ARM_UP_MS = 70;

type Hand = "left" | "right";

// Each launch starts on the left hand; a completed strike flips to the other one.
let nextHand: Hand = "left";

function raiseArm() {
  if (armTimer !== undefined) window.clearTimeout(armTimer);
  armTimer = undefined;
  armPending = false;
  stage.dataset.hand = "up";
}

function strikeArm() {
  stage.dataset.hand = nextHand;
  nextHand = nextHand === "left" ? "right" : "left";
  armTimer = window.setTimeout(() => {
    armTimer = undefined;
    stage.dataset.hand = "up";
    if (armPending) {
      armPending = false;
      armTimer = window.setTimeout(strikeArm, ARM_UP_MS);
    }
  }, ARM_DOWN_MS);
}

const BLINK_CLOSED_MS = 280;
const BLINK_GAP_MIN_MS = 2600;
const BLINK_GAP_MAX_MS = 6200;

// Blinking runs on its own clock: it never waits for the arm and the arm never waits for it.
function blink() {
  if (!document.hidden) {
    stage.dataset.eyes = "closed";
    window.setTimeout(() => {
      stage.dataset.eyes = "open";
    }, BLINK_CLOSED_MS);
  }
  window.setTimeout(blink, BLINK_GAP_MIN_MS + Math.random() * (BLINK_GAP_MAX_MS - BLINK_GAP_MIN_MS));
}

window.setTimeout(blink, BLINK_GAP_MIN_MS);

function tapHand() {
  if (document.hidden) return;
  // Faster inputs coalesce into one owed strike so the arm keeps a visible down-up rhythm.
  // The spark reads #stage[data-hand], so a coalesced tap flashes on the fist that is still down.
  if (armTimer === undefined) strikeArm();
  else armPending = true;
  impactAnimation?.cancel();
  const burst = impact.animate(
    [
      { opacity: 1, transform: "scale(0.65)", offset: 0, easing: "ease-out" },
      { opacity: 1, transform: "scale(1)", offset: 0.2 },
      { opacity: 0.9, transform: "scale(1.04)", offset: 0.45, easing: "ease-out" },
      { opacity: 0, transform: "scale(1.22)", offset: 1 },
    ],
    { duration: 180 },
  );
  impactAnimation = burst;
  burst.onfinish = () => {
    if (impactAnimation === burst) impactAnimation = undefined;
  };
}

async function initInput() {
  await listen<{ ts: number; total: string }>("input:activity", (event) => {
    updateCount(event.payload.total);
    tapHand();
  });
  updateCount(await invoke<string>("get_input_count"));
}

if (isTauri) {
  void initInput().catch((error) => console.warn("failed to initialize pet input", error));
} else {
  function previewTap() {
    total += 1n;
    renderCount();
    tapHand();
  }
  const heldMouseButtons = new Set<number>();
  window.addEventListener("keydown", (event) => {
    if (!event.repeat) previewTap();
  });
  window.addEventListener("mousedown", (event) => {
    if (heldMouseButtons.has(event.button)) return;
    heldMouseButtons.add(event.button);
    previewTap();
  }, true);
  window.addEventListener("mouseup", (event) => heldMouseButtons.delete(event.button), true);
  window.addEventListener("mousemove", (event) => {
    for (const button of heldMouseButtons) {
      const mask = button === 1 ? 4 : button === 2 ? 2 : 1 << button;
      if ((event.buttons & mask) === 0) heldMouseButtons.delete(button);
    }
  }, true);
  window.addEventListener("blur", () => heldMouseButtons.clear());
}

document.addEventListener("visibilitychange", () => {
  if (document.hidden) {
    raiseArm();
    // A blink interrupted by hiding would otherwise stay closed until the throttled restore timer runs.
    stage.dataset.eyes = "open";
    impactAnimation?.cancel();
    impactAnimation = undefined;
  } else if (isTauri) {
    void invoke<string>("get_input_count").then(updateCount)
      .catch((error) => console.warn("failed to refresh input count", error));
  }
});

settingsToggle.addEventListener("mousedown", (event) => event.stopPropagation());
settingsToggle.addEventListener("click", async () => {
  if (!isTauri) {
    if (settingsPreview && !settingsPreview.closed) {
      settingsPreview.close();
      settingsPreview = null;
    } else {
      settingsPreview = window.open("/settings.html", "tappo-settings", "popup,width=880,height=640");
    }
    return;
  }
  settingsToggle.disabled = true;
  try {
    await focusRequest;
    await invoke("toggle_settings_window");
  } catch (error) {
    console.warn("failed to toggle settings window", error);
  } finally {
    settingsToggle.disabled = false;
  }
});

type PermissionStatus = {
  accessibility: boolean;
  input_monitoring: boolean;
};

type SettingsView = {
  language: LanguageSetting;
  resolved: Language;
  skin: string;
  accessory: string;
  drag_disabled: boolean;
  hidden: boolean;
  opacity: number;
};

let dragDisabled = false;

function setPetOpacity(opacity: number) {
  document.documentElement.style.setProperty("--pet-opacity", String(opacity));
}

function setDragDisabled(disabled: boolean) {
  dragDisabled = disabled;
  document.documentElement.classList.toggle("drag-disabled", disabled);
}

function applyLanguage(view: Pick<SettingsView, "language" | "resolved">) {
  applyTranslations(document, view.resolved);
  counterLanguage = view.resolved;
  countFormatter = new Intl.NumberFormat(counterLanguage, { useGrouping: false });
  renderCount();
  const label = translate(view.resolved, "settings.toggle");
  settingsToggle.title = label;
  settingsToggle.setAttribute("aria-label", label);
}

function applySettingsView(view: SettingsView) {
  applyLanguage(view);
  setDragDisabled(view.drag_disabled);
  setPetOpacity(view.opacity);
  equipAccessory(accessoryOverride ?? view.accessory);
  void applySkin(skinOverride ?? view.skin);
}

async function refreshSettings() {
  if (!isTauri) {
    applyLanguage({ language: "system", resolved: resolveLanguage("system") });
    return;
  }
  try {
    applySettingsView(await invoke<SettingsView>("get_settings"));
  } catch (error) {
    console.warn("failed to load settings", error);
  }
}

async function refreshPermission() {
  if (!isTauri) {
    return;
  }
  try {
    const status = await invoke<PermissionStatus>("check_permissions");
    const missing = !status.input_monitoring;
    banner.hidden = !missing;
    if (!missing && permissionTimer !== undefined) {
      window.clearInterval(permissionTimer);
      permissionTimer = undefined;
    }
  } catch (error) {
    console.warn("permission check failed", error);
  }
}

async function initCanvasScale() {
  function applyCanvasScale(scale: number | null) {
    if (scale === null) return;
    document.documentElement.style.setProperty("--overlay-zoom", String(1 / scale));
    document.documentElement.classList.toggle("compact-canvas", 392 * scale <= 200);
  }
  type WindowsCanvas = { scale: number; offset: { x: number; y: number }; revision: number };
  let revision = 0;
  function applyWindowsCanvas(canvas: WindowsCanvas | null) {
    if (!canvas || canvas.revision < revision) return;
    revision = canvas.revision;
    const root = document.documentElement;
    root.classList.add("windows-canvas");
    root.style.setProperty("--canvas-scale", String(canvas.scale));
    root.style.setProperty("--canvas-x", `${canvas.offset.x}px`);
    root.style.setProperty("--canvas-y", `${canvas.offset.y}px`);
    applyCanvasScale(canvas.scale);
  }
  try {
    await Promise.all([
      listen<number>("pet:canvas-scale", (event) => applyCanvasScale(event.payload)),
      listen<WindowsCanvas>("pet:windows-canvas", (event) => applyWindowsCanvas(event.payload)),
    ]);
    applyCanvasScale(await invoke<number | null>("get_pet_canvas_scale"));
    applyWindowsCanvas(await invoke<WindowsCanvas | null>("get_windows_pet_canvas"));
  } catch (error) {
    console.warn("failed to initialize pet canvas", error);
  }
}

if (isTauri) {
  void initCanvasScale();
  const pet = document.querySelector<HTMLElement>("#pet")!;
  type Point = { x: number; y: number };
  type Drag = {
    pointer: number;
    pending?: Point;
    frame?: number;
    moving: boolean;
    anchor: Point;
    started: boolean;
  };
  // Screen points the pointer must travel before the window may move; a click never repositions.
  const DRAG_THRESHOLD = 3;
  let drag: Drag | undefined;
  let dragQueue = Promise.resolve();
  let nativeDrag: boolean | undefined;
  let nativeDragPending = false;
  void invoke<boolean>("uses_native_pet_drag")
    .then((value) => { nativeDrag = value; })
    .catch((error) => console.warn("failed to initialize pet dragging", error));

  function queueDrag(
    command: "start_pet_drag" | "drag_pet" | "end_pet_drag",
    args: { offset?: Point; cursor?: Point | null },
  ) {
    dragQueue = dragQueue
      .then(() => invoke<void>(command, args))
      .catch((error) => console.warn("failed to drag pet", error));
    return dragQueue;
  }

  function scheduleDrag(current: Drag) {
    if (current.frame !== undefined || current.moving) return;
    current.frame = requestAnimationFrame(() => {
      current.frame = undefined;
      const cursor = current.pending;
      current.pending = undefined;
      if (drag !== current || !cursor) return;
      current.moving = true;
      void queueDrag("drag_pet", { cursor }).then(() => {
        current.moving = false;
        if (drag === current && current.pending) scheduleDrag(current);
      });
    });
  }

  pet.addEventListener("pointerdown", (event) => {
    if (
      event.button !== 0 ||
      dragDisabled ||
      drag ||
      nativeDrag === undefined ||
      nativeDragPending ||
      !badge.hidden ||
      !(event.target instanceof HTMLElement) ||
      !event.target.hasAttribute("data-pet-drag-region")
    ) return;
    event.preventDefault();
    const offset = { x: event.clientX, y: event.clientY };
    if (nativeDrag) {
      nativeDragPending = true;
      void invoke("start_pet_drag", { offset })
        .catch((error) => console.warn("failed to drag pet", error))
        .finally(() => { nativeDragPending = false; });
      return;
    }
    pet.setPointerCapture(event.pointerId);
    drag = {
      pointer: event.pointerId,
      moving: false,
      anchor: { x: event.screenX, y: event.screenY },
      started: false,
    };
  });

  pet.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.pointer) return;
    if ((event.buttons & 1) === 0) {
      finishDrag(null);
      return;
    }
    if (!drag.started) {
      const travel =
        Math.abs(event.screenX - drag.anchor.x) + Math.abs(event.screenY - drag.anchor.y);
      if (travel < DRAG_THRESHOLD) return;
      drag.started = true;
      void queueDrag("start_pet_drag", { offset: { x: event.clientX, y: event.clientY } });
    }
    drag.pending = { x: event.screenX, y: event.screenY };
    scheduleDrag(drag);
  });

  function finishDrag(cursor: Point | null) {
    if (!drag) return;
    const pointer = drag.pointer;
    const final = drag.started ? cursor : null;
    if (drag.frame !== undefined) cancelAnimationFrame(drag.frame);
    drag = undefined;
    if (pet.hasPointerCapture(pointer)) pet.releasePointerCapture(pointer);
    void queueDrag("end_pet_drag", { cursor: final });
  }

  function endDrag(event: PointerEvent) {
    if (event.pointerId === drag?.pointer) {
      finishDrag(event.type === "pointerup" ? { x: event.screenX, y: event.screenY } : null);
    }
  }

  pet.addEventListener("pointerup", endDrag);
  pet.addEventListener("pointercancel", endDrag);
  pet.addEventListener("lostpointercapture", endDrag);
  window.addEventListener("blur", () => finishDrag(null));

  document.documentElement.addEventListener("pointerenter", () => {
    if (!badge.hidden || focusRequest) return;
    focusRequest = invoke<void>("focus_pet")
      .catch((error) => {
        console.warn("failed to focus pet window", error);
      })
      .finally(() => {
        focusRequest = undefined;
      });
  });

  listen<boolean>("ui:click-through", (event) => {
    badge.hidden = !event.payload;
    if (event.payload) finishDrag(null);
  });

  bannerBtn.addEventListener("click", () => {
    void invoke("request_input_monitoring");
  });

  listen<SettingsView>("settings:changed", (event) => applySettingsView(event.payload));
  listen<number>("pet:opacity", (event) => setPetOpacity(event.payload));

  void refreshSettings();
  void refreshPermission();
  permissionTimer = window.setInterval(refreshPermission, 3000);
  window.addEventListener("focus", () => void refreshPermission());
} else {
  applyLanguage({ language: "system", resolved: resolveLanguage("system") });
  equipAccessory(accessoryOverride);
}
