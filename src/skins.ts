// Skin packs: a directory under src/assets/skins/ = manifest.json + six frame PNGs.
// The state machine in main.ts only flips #stage[data-hand][data-eyes]; this module is
// the socket between those states and a pack's pixels and display numbers, so adding a
// character never touches blink/tap code.

export type Hand = "up" | "left" | "right";
export type Eyes = "open" | "closed";
export type FrameKey = `${Hand}|${Eyes}`;

type Anchor = { x: number; y: number };

export type SideMount = { x: number; y: number; align: "bottom-left" | "bottom-right" };

export type SkinManifest = {
  format: number;
  id: string;
  name: { zh?: string; en?: string };
  canvas: { width: number; height: number };
  display: {
    width: number;
    drop: number;
    impact: { color: string; left: Anchor; right: Anchor };
  };
  mounts?: {
    side?: {
      left?: SideMount;
      right?: SideMount;
    };
  };
  frames: {
    state: { hand: Hand; eyes: Eyes };
    file: string;
    md5?: string;
    sha256: string;
  }[];
};

export type SkinPack = {
  id: string;
  manifest: SkinManifest;
  frames: Record<FrameKey, string>;
};

export const DEFAULT_SKIN_ID = "xigerika";

const FRAME_SLOTS: Record<FrameKey, string> = {
  "up|open": "pet-art",
  "left|open": "pet-art-left",
  "right|open": "pet-art-right",
  "up|closed": "pet-art-eyes",
  "left|closed": "pet-art-eyes-left",
  "right|closed": "pet-art-eyes-right",
};

const ALL_FRAMES = Object.keys(FRAME_SLOTS) as FrameKey[];

const manifestModules = import.meta.glob<SkinManifest>("./assets/skins/*/manifest.json", {
  eager: true,
  import: "default",
});
const imageUrls = import.meta.glob<string>("./assets/skins/*/*.png", {
  eager: true,
  query: "?url",
  import: "default",
});

function buildPack(dir: string, manifest: SkinManifest): SkinPack | null {
  const frames = {} as Record<FrameKey, string>;
  for (const frame of manifest.frames ?? []) {
    const url = imageUrls[`./assets/skins/${dir}/${frame.file}`];
    if (url) frames[`${frame.state.hand}|${frame.state.eyes}` as FrameKey] = url;
  }
  const missing = ALL_FRAMES.filter((key) => !frames[key]);
  if (!manifest.display || missing.length > 0) {
    console.warn(
      `skin pack "${dir}" 不完整，已跳过：缺 ${[!manifest.display && "display", ...missing].filter(Boolean).join(", ")}`,
    );
    return null;
  }
  return { id: manifest.id, manifest, frames };
}

const packs = new Map<string, SkinPack>();
for (const [path, manifest] of Object.entries(manifestModules)) {
  const [dir] = path.split("/").slice(-2);
  const pack = buildPack(dir, manifest);
  if (pack) packs.set(pack.id, pack);
}

export function listSkins(): SkinPack[] {
  return [...packs.values()];
}

let currentSkinId: string | null = null;

export async function applySkin(id?: string | null): Promise<string | null> {
  const requested = id ?? DEFAULT_SKIN_ID;
  if (requested === currentSkinId) return requested;
  const pack = packs.get(requested) ?? packs.get(DEFAULT_SKIN_ID) ?? listSkins()[0];
  if (!pack) {
    console.warn("没有任何可用的皮肤包");
    return null;
  }
  if (pack.id !== requested) {
    console.warn(`skin "${requested}" 不可用，回退到 "${pack.id}"`);
  }

  // Decode before swapping so the stage never shows half old / half new frames.
  await Promise.allSettled(
    Object.values(pack.frames).map(async (url) => {
      const img = new Image();
      img.src = url;
      await img.decode();
    }),
  );

  for (const key of ALL_FRAMES) {
    const el = document.getElementById(FRAME_SLOTS[key]);
    if (el instanceof HTMLImageElement) el.src = pack.frames[key];
  }

  const pet = document.querySelector<HTMLElement>("#pet");
  if (pet) {
    const { canvas, display } = pack.manifest;
    const set = (name: string, value: string | number) => pet.style.setProperty(name, String(value));
    set("--skin-width", display.width);
    set("--skin-canvas-w", canvas.width);
    set("--skin-canvas-h", canvas.height);
    set("--skin-drop", display.drop);
    set("--impact-left-x", display.impact.left.x);
    set("--impact-left-y", display.impact.left.y);
    set("--impact-right-x", display.impact.right.x);
    set("--impact-right-y", display.impact.right.y);
    set("--impact-color", display.impact.color);
  }

  currentSkinId = pack.id;
  document.documentElement.dataset.skin = pack.id;
  // Accessories hang off the active skin's mounts, so they re-place whenever this fires.
  document.dispatchEvent(new CustomEvent<string>("tappo:skin", { detail: pack.id }));
  return pack.id;
}

export function getActiveSkin(): SkinPack | null {
  return currentSkinId ? packs.get(currentSkinId) ?? null : null;
}
