import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const isTauri = "__TAURI_INTERNALS__" in window;

const pet = document.querySelector<HTMLElement>("#pet")!;
const wrap = document.querySelector<HTMLElement>("#bunny-wrap")!;
const pawLeft = document.querySelector<SVGGElement>("#paw-left")!;
const pawRight = document.querySelector<SVGGElement>("#paw-right")!;
const hint = document.querySelector<HTMLElement>("#hint")!;
const banner = document.querySelector<HTMLElement>("#banner")!;
const bannerBtn = document.querySelector<HTMLButtonElement>("#banner-btn")!;
const badge = document.querySelector<HTMLElement>("#badge")!;

const SLEEP_AFTER_MS = 5 * 60 * 1000;
const HINT_FADE_MS = 1200;

type Reaction = "left" | "right" | "mouse" | "wheel";

let lastInput = Date.now();
let sleeping = false;
let hintTimer: number | undefined;
let permissionTimer: number | undefined;

function pulse(element: Element) {
  element.classList.remove("tap");
  void element.getBoundingClientRect();
  element.classList.add("tap");
}

function bounce() {
  wrap.classList.remove("bounce");
  void wrap.offsetWidth;
  wrap.classList.add("bounce");
}

function react(reaction: Reaction) {
  lastInput = Date.now();

  if (sleeping) {
    wake();
  }

  switch (reaction) {
    case "left":
      pulse(pawLeft);
      bounce();
      break;
    case "right":
      pulse(pawRight);
      bounce();
      break;
    case "mouse":
      pulse(pawLeft);
      pulse(pawRight);
      bounce();
      break;
    case "wheel":
      bounce();
      break;
  }

  window.clearTimeout(hintTimer);
  hintTimer = window.setTimeout(() => hint.classList.add("faded"), HINT_FADE_MS);
}

function sleep() {
  sleeping = true;
  pet.classList.add("sleeping");
}

function wake() {
  sleeping = false;
  pet.classList.remove("sleeping");
  pet.classList.add("waking");
  window.setTimeout(() => pet.classList.remove("waking"), 700);
}

for (const element of [pawLeft, pawRight]) {
  element.addEventListener("animationend", () => element.classList.remove("tap"));
}
wrap.addEventListener("animationend", () => wrap.classList.remove("bounce"));

window.setInterval(() => {
  if (!sleeping && Date.now() - lastInput >= SLEEP_AFTER_MS) {
    sleep();
  }
}, 1000);

async function refreshPermission() {
  if (!isTauri) {
    return;
  }
  try {
    const trusted = await invoke<boolean>("check_accessibility");
    banner.hidden = trusted;
    if (trusted && permissionTimer !== undefined) {
      window.clearInterval(permissionTimer);
      permissionTimer = undefined;
    }
  } catch (error) {
    console.warn("accessibility check failed", error);
  }
}

if (isTauri) {
  listen<{ side: "left" | "right"; ts: number }>("input:key", (event) => {
    if (import.meta.env.DEV && event.payload.ts) {
      const ms = Date.now() - event.payload.ts;
      if (ms >= 0 && ms < 1000) {
        void invoke("report_latency", { ms });
      }
    }
    react(event.payload.side === "left" ? "left" : "right");
  });

  listen("input:mouse", () => react("mouse"));
  listen("input:wheel", () => react("wheel"));

  listen<boolean>("ui:click-through", (event) => {
    badge.hidden = !event.payload;
  });

  bannerBtn.addEventListener("click", () => {
    void invoke("open_accessibility_settings");
  });

  void refreshPermission();
  permissionTimer = window.setInterval(refreshPermission, 3000);
  window.addEventListener("focus", () => void refreshPermission());
} else {
  // Browser preview: approximate the left/right split without reading key text.
  const isLeftKey = (code: string) =>
    /^Key[QWERTASDFGZXCVB]$/.test(code) ||
    /^Digit[1-5]$/.test(code) ||
    ["Backquote", "Tab", "CapsLock", "ShiftLeft", "ControlLeft", "MetaLeft", "AltLeft", "Escape"].includes(code) ||
    /^F[1-4]$/.test(code);

  window.addEventListener("keydown", (event) => {
    if (event.repeat) {
      return;
    }
    react(isLeftKey(event.code) ? "left" : "right");
  });
  window.addEventListener("mousedown", () => react("mouse"));
}
