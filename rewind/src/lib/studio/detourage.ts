// Retirer le fond d'une image (2026-11-24) : le modèle tourne dans Rewind (detourage.rs), sur le PC.
// Ici : image ramenée à 1024 × 1024 pour le modèle, puis masque remis à la taille d'origine et appliqué en
// transparence ; l'image détourée est un nouveau PNG (l'original n'est pas modifié).
import { listen } from "@tauri-apps/api/event";
import { api, fileUrl } from "../api";
import { t } from "../i18n.svelte";

const SIDE = 1024;

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((ok, ko) => {
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => ok(img);
    img.onerror = () => ko(new Error(t("image illisible")));
    img.src = src;
  });
}

const b64Of = (c: HTMLCanvasElement) => c.toDataURL("image/png").split(",")[1];

/** Détoure l'image `path` ; renvoie le chemin du PNG détouré. `onStep` : texte d'avancement. */
export async function removeBackground(path: string, onStep: (s: string) => void = () => {}): Promise<string> {
  if (!(await api.bgReady())) {
    onStep(t("Téléchargement de l'outil (178 Mo, une seule fois)…"));
    const stop = await listen<number>("detourage", (e) => onStep(t("Téléchargement de l'outil : {n} %", { n: Math.round(e.payload * 100) })));
    try {
      await api.bgPrepare();
    } finally {
      stop();
    }
  }
  onStep(t("Détourage…"));
  const img = await loadImage(fileUrl(path));
  const W = img.naturalWidth, H = img.naturalHeight;
  // 1) Image pour le modèle (étirée en 1024 × 1024, comme rembg)
  const small = document.createElement("canvas");
  small.width = small.height = SIDE;
  const sc = small.getContext("2d")!;
  sc.imageSmoothingQuality = "high";
  sc.drawImage(img, 0, 0, SIDE, SIDE);
  const mask = Uint8Array.from(atob(await api.bgMask(b64Of(small))), (c) => c.charCodeAt(0));
  // 2) Masque en transparence, agrandi en douceur à la taille de l'image
  const mc = document.createElement("canvas");
  mc.width = mc.height = SIDE;
  const mctx = mc.getContext("2d")!;
  const data = mctx.createImageData(SIDE, SIDE);
  for (let i = 0; i < SIDE * SIDE; i++) {
    data.data[i * 4] = data.data[i * 4 + 1] = data.data[i * 4 + 2] = 255;
    data.data[i * 4 + 3] = mask[i] ?? 0;
  }
  mctx.putImageData(data, 0, 0);
  const out = document.createElement("canvas");
  out.width = W;
  out.height = H;
  const o = out.getContext("2d")!;
  o.drawImage(img, 0, 0);
  o.globalCompositeOperation = "destination-in";
  o.imageSmoothingQuality = "high";
  o.drawImage(mc, 0, 0, W, H);
  const name = (path.split(/[\\/]/).pop() ?? "image").replace(/\.[^.]+$/, "") + " détouré";
  return api.saveImage(b64Of(out), name);
}
