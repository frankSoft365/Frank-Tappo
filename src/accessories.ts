// Side accessories: a directory under src/assets/accessories/ = manifest.json + one static PNG.
// The accessory itself never animates; it only follows the active skin's mount, so a skin
// without a matching mount simply hides it. The equipped id comes from settings.

import { getActiveSkin } from "./skins";
import type { SideMount } from "./skins";

export type SideAccessoryManifest = {
  format: number;
  id: string;
  name: { zh?: string; en?: string };
  kind: "side";
  side: "left" | "right";
  file: string;
  width: number;
  layer: "front" | "behind";
  sha256: string;
};

type SideAccessoryPack = {
  manifest: SideAccessoryManifest;
  url: string;
};

export type AccessoryEntry = {
  id: string;
  name: SideAccessoryManifest["name"];
  url: string;
  width: number;
  side: SideAccessoryManifest["side"];
  layer: SideAccessoryManifest["layer"];
};

const manifestModules = import.meta.glob<SideAccessoryManifest>(
  "./assets/accessories/*/manifest.json",
  { eager: true, import: "default" },
);
const imageUrls = import.meta.glob<string>("./assets/accessories/*/*.png", {
  eager: true,
  query: "?url",
  import: "default",
});

function isUsable(manifest: SideAccessoryManifest | undefined): manifest is SideAccessoryManifest {
  return (
    !!manifest &&
    manifest.format === 0 &&
    manifest.kind === "side" &&
    (manifest.side === "left" || manifest.side === "right") &&
    (manifest.layer === "front" || manifest.layer === "behind") &&
    Number.isFinite(manifest.width) &&
    manifest.width > 0 &&
    typeof manifest.id === "string" &&
    typeof manifest.file === "string"
  );
}

const packs = new Map<string, SideAccessoryPack>();
for (const [path, manifest] of Object.entries(manifestModules)) {
  const [dir] = path.split("/").slice(-2);
  if (!isUsable(manifest)) {
    console.warn(`配件包 "${dir}" 的 manifest 不完整，已跳过`);
    continue;
  }
  const url = imageUrls[`./assets/accessories/${dir}/${manifest.file}`];
  if (!url) {
    console.warn(`配件包 "${dir}" 缺少文件 ${manifest.file}，已跳过`);
    continue;
  }
  if (manifest.id !== dir) {
    console.warn(`配件包目录 "${dir}" 与 manifest.id "${manifest.id}" 不一致`);
  }
  packs.set(manifest.id, { manifest, url });
}

export function listAccessories(): AccessoryEntry[] {
  return [...packs.values()].map(({ manifest, url }) => ({
    id: manifest.id,
    name: manifest.name,
    url,
    width: manifest.width,
    side: manifest.side,
    layer: manifest.layer,
  }));
}

let equippedId: string | null = null;

function apply() {
  const img = document.querySelector<HTMLImageElement>("#side-accessory");
  if (!img) return;
  const pack = equippedId ? packs.get(equippedId) : undefined;
  const mount: SideMount | undefined = pack
    ? getActiveSkin()?.manifest.mounts?.side?.[pack.manifest.side]
    : undefined;
  if (!pack || !mount) {
    img.hidden = true;
    return;
  }
  if (img.getAttribute("src") !== pack.url) img.src = pack.url;
  img.hidden = false;
  img.style.setProperty("--acc-width", String(pack.manifest.width));
  img.style.setProperty("--acc-x", String(mount.x));
  img.style.setProperty("--acc-y", String(mount.y));
  img.dataset.align = mount.align;
  img.dataset.layer = pack.manifest.layer;
  img.alt = pack.manifest.name.zh ?? pack.manifest.name.en ?? pack.manifest.id;
}

export function initAccessories() {
  document.addEventListener("tappo:skin", apply);
  apply();
}

export function equipAccessory(id?: string | null) {
  const wanted = id?.trim();
  if (wanted && wanted !== "none") {
    if (!packs.has(wanted)) {
      console.warn(`配件 "${wanted}" 不可用，已保持不装备`);
      equippedId = null;
      apply();
      return;
    }
    equippedId = wanted;
  } else {
    equippedId = null;
  }
  apply();
}