// Dessin sur canvas partagé par l'aperçu et l'export : texte stylé et clavier / souris.
// L'export envoie ces dessins en PNG au moteur, qui les incruste : le rendu est identique au pixel près.
import type { TextLayer } from "./model";
import { fontOf, cssFont } from "./fonts";

// ---------------------------------------------------------------------------
// Texte
// ---------------------------------------------------------------------------

export type TextBox = { w: number; h: number; lines: string[]; lineH: number; pad: number; px: number };

/** Mesure le texte pour une échelle k (pixels par « pixel de référence 1080p »). */
export function measureText(ctx: CanvasRenderingContext2D, l: TextLayer, k: number): TextBox {
  const f = fontOf(l.font);
  const px = Math.max(1, l.size * k);
  ctx.font = cssFont(f, px);
  const lines = (l.text || " ").split("\n");
  const widths = lines.map((s) => ctx.measureText(s || " ").width);
  const lineH = px * (f.id === "pixel" ? 1.45 : 1.18);
  const stroke = l.stroke ? l.stroke.width * k : 0;
  const pad = (l.bg ? px * 0.32 : 0) + stroke + (l.shadow ? px * 0.12 : 0) + 2;
  return { w: Math.ceil(Math.max(...widths) + pad * 2), h: Math.ceil(lineH * lines.length + pad * 2), lines, lineH, pad, px };
}

/** Dessine le texte dans un canvas déjà dimensionné (box). `reveal` : part des caractères visibles (machine à écrire). */
export function drawText(ctx: CanvasRenderingContext2D, l: TextLayer, k: number, box: TextBox, reveal = 1) {
  const f = fontOf(l.font);
  ctx.save();
  ctx.clearRect(0, 0, box.w, box.h);
  ctx.font = cssFont(f, box.px);
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  ctx.miterLimit = 2;
  ctx.textAlign = "left";
  const total = box.lines.reduce((a, s) => a + s.length, 0);
  let budget = reveal >= 1 ? Infinity : Math.round(total * reveal);
  const padBg = l.bg ? box.px * 0.32 : 0;
  // Fond : un rectangle arrondi par ligne (style sous-titre), toujours à la taille du texte complet
  if (l.bg) {
    ctx.fillStyle = hexA(l.bg.color, l.bg.opacity);
    box.lines.forEach((s, i) => {
      const w = ctx.measureText(s || " ").width;
      const x = alignX(l.align, box, w);
      const cy = box.pad + box.lineH * (i + 0.5);
      roundRect(ctx, x - padBg, cy - box.lineH / 2 - padBg * 0.4, w + padBg * 2, box.lineH + padBg * 0.8, box.px * 0.2);
      ctx.fill();
    });
  }
  box.lines.forEach((full, i) => {
    const s = budget >= full.length ? full : full.slice(0, Math.max(0, budget));
    budget -= full.length;
    if (!s) return;
    const fullW = ctx.measureText(full || " ").width;
    const x = alignX(l.align, box, fullW);
    const y = box.pad + box.lineH * (i + 0.5);
    if (l.shadow) {
      ctx.shadowColor = "rgba(0,0,0,0.55)";
      ctx.shadowBlur = box.px * 0.14;
      ctx.shadowOffsetY = box.px * 0.05;
    }
    if (l.stroke && l.stroke.width > 0) {
      ctx.strokeStyle = l.stroke.color;
      ctx.lineWidth = l.stroke.width * 2 * (box.px / l.size);
      ctx.strokeText(s, x, y);
      ctx.shadowColor = "transparent";
    }
    ctx.fillStyle = l.color;
    ctx.fillText(s, x, y);
    ctx.shadowColor = "transparent";
  });
  ctx.restore();
}

/** Bord gauche d'une ligne de largeur w dans la boîte. */
function alignX(align: TextLayer["align"], box: TextBox, w: number): number {
  const inner = box.w - box.pad * 2;
  if (align === "left") return box.pad;
  if (align === "right") return box.pad + inner - w;
  return box.pad + (inner - w) / 2;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

function hexA(hex: string, a: number): string {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h, 16);
  return `rgba(${(n >> 16) & 255},${(n >> 8) & 255},${n & 255},${a})`;
}

/** Texte en PNG (base64 sans en-tête) à l'échelle k. */
export function textPng(l: TextLayer, k: number, reveal = 1): { b64: string; w: number; h: number } {
  const c = document.createElement("canvas");
  const ctx = c.getContext("2d")!;
  const box = measureText(ctx, l, k);
  c.width = box.w;
  c.height = box.h;
  drawText(ctx, l, k, box, reveal);
  return { b64: c.toDataURL("image/png").split(",")[1], w: box.w, h: box.h };
}

// ---------------------------------------------------------------------------
// Clavier et souris
// ---------------------------------------------------------------------------

/** Une touche ; k = 0 : case vide (décalage). */
export type KeyDef = { k: number; label: string; w?: number };
export type KeysPreset = "compact" | "standard" | "complet";
export type KeysLayout = "auto" | "qwerty" | "azerty";

const K = (k: number, label: string, w?: number): KeyDef => ({ k, label, w });
/** Dispositions en position QWERTY physique (les lettres affichées suivent le clavier choisi). */
export const KEY_LAYOUTS: Record<KeysPreset, KeyDef[][]> = {
  compact: [
    // Case vide de la largeur de Maj + A : le W tombe au-dessus du S
    [K(0, "", 2.49), K(0x57, "W")],
    [K(0xa0, "Maj", 1.35), K(0x41, "A"), K(0x53, "S"), K(0x44, "D")],
    [K(0xa2, "Ctrl", 1.35), K(0x20, "Espace", 3.28)],
  ],
  standard: [
    [K(0x09, "Tab", 1.4), K(0x51, "Q"), K(0x57, "W"), K(0x45, "E"), K(0x52, "R")],
    [K(0xa0, "Maj", 1.4), K(0x41, "A"), K(0x53, "S"), K(0x44, "D"), K(0x46, "F")],
    [K(0xa2, "Ctrl", 1.4), K(0x20, "Espace", 3.1), K(0x43, "C")],
  ],
  complet: [
    [K(0, "", 1.4), K(0x31, "1"), K(0x32, "2"), K(0x33, "3"), K(0x34, "4"), K(0x35, "5")],
    [K(0x09, "Tab", 1.4), K(0x51, "Q"), K(0x57, "W"), K(0x45, "E"), K(0x52, "R"), K(0x54, "T")],
    [K(0xa0, "Maj", 1.4), K(0x41, "A"), K(0x53, "S"), K(0x44, "D"), K(0x46, "F"), K(0x47, "G")],
    [K(0xa2, "Ctrl", 1.4), K(0x5a, "Z"), K(0x58, "X"), K(0x43, "C"), K(0x56, "V"), K(0x42, "B")],
    [K(0xa4, "Alt", 1.4), K(0x20, "Espace", 5.56)],
  ],
};
/** Ancienne liste (standard), gardée pour compatibilité. */
export const KEY_ROWS = KEY_LAYOUTS.standard;
export const MOUSE_KEYS = [0x01, 0x02, 0x04];

/** Lettres affichées : celles du clavier de Windows (auto), ou une disposition forcée. */
export function layoutLabels(layout: KeysLayout | string | undefined, windows: Record<string, string>): Record<string, string> {
  if (layout === "qwerty") return {};
  // AZERTY : A et Q, Z et W échangés (codes de position QWERTY)
  if (layout === "azerty") return { "81": "A", "87": "Z", "65": "Q", "90": "W" };
  return windows;
}

export const presetOf = (p: string | undefined): KeysPreset => (p === "compact" || p === "complet" ? p : "standard");

/** Taille du panneau pour une unité u (taille d'une touche). */
export function keysSize(u: number, preset: KeysPreset = "standard") {
  const rows = KEY_LAYOUTS[preset];
  const gap = u * 0.14, pad = u * 0.34;
  const rowW = (r: KeyDef[]) => r.reduce((a, k) => a + (k.w ?? 1) * u, 0) + gap * (r.length - 1);
  const boardW = Math.max(...rows.map(rowW));
  const mouseW = u * 1.5, mouseH = u * 2.25;
  const boardH = rows.length * u + (rows.length - 1) * gap;
  return { w: Math.ceil(pad + boardW + gap * 3 + mouseW + pad), h: Math.ceil(pad + Math.max(boardH, mouseH) + pad), gap, pad, mouseW, mouseH, boardW, rows };
}

/**
 * Dessine le clavier. `only` : ne dessine QUE cette touche, enfoncée, sur fond transparent (calques de l'export,
 * superposés au fond quand la touche est appuyée). `labels` : lettres de la disposition du clavier ; `name` : traduction.
 */
export function drawKeys(ctx: CanvasRenderingContext2D, u: number, isDown: (k: number) => boolean, labels: Record<string, string>, name: (s: string) => string, only?: number, preset: KeysPreset = "standard") {
  const S = keysSize(u, preset);
  ctx.save();
  ctx.clearRect(0, 0, S.w, S.h);
  const r = u * 0.22;
  if (only === undefined) {
    ctx.fillStyle = "rgba(8,9,12,0.5)";
    roundRect(ctx, 0, 0, S.w, S.h, u * 0.42);
    ctx.fill();
  }
  // Clavier calé en bas du panneau (à côté de la souris)
  const boardH = S.rows.length * u + (S.rows.length - 1) * S.gap;
  const top = S.h - S.pad - boardH;
  S.rows.forEach((row, ri) => {
    let x = S.pad;
    const y = top + ri * (u + S.gap);
    for (const key of row) {
      const w = (key.w ?? 1) * u;
      const down = only !== undefined ? only === key.k : isDown(key.k);
      if (key.k !== 0 && (only === undefined || only === key.k)) {
        roundRect(ctx, x, y, w, u, r);
        ctx.fillStyle = down ? "#ffffff" : "rgba(255,255,255,0.1)";
        ctx.fill();
        if (!down) {
          ctx.strokeStyle = "rgba(255,255,255,0.18)";
          ctx.lineWidth = Math.max(1, u * 0.035);
          ctx.stroke();
        }
        const label = labels[String(key.k)] ?? name(key.label);
        ctx.fillStyle = down ? "#0b0c0f" : "rgba(255,255,255,0.88)";
        ctx.font = `700 ${Math.round(label.length > 2 ? u * 0.3 : u * 0.38)}px "Inter", "Segoe UI", sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(label, x + w / 2, y + u / 2 + u * 0.02);
      }
      x += w + S.gap;
    }
  });
  // Souris : corps arrondi, deux boutons, molette
  const mx = S.pad + S.boardW + S.gap * 3;
  const my = S.h - S.pad - S.mouseH;
  const mw = S.mouseW, mh = S.mouseH, rr = mw / 2;
  const body = () => {
    ctx.beginPath();
    ctx.moveTo(mx, my + rr);
    ctx.arc(mx + rr, my + rr, rr, Math.PI, 0);
    ctx.lineTo(mx + mw, my + mh - rr);
    ctx.arc(mx + rr, my + mh - rr, rr, 0, Math.PI);
    ctx.closePath();
  };
  if (only === undefined) {
    body();
    ctx.fillStyle = "rgba(255,255,255,0.1)";
    ctx.fill();
    ctx.strokeStyle = "rgba(255,255,255,0.28)";
    ctx.lineWidth = Math.max(1, u * 0.04);
    ctx.stroke();
  }
  const split = my + mh * 0.42;
  const button = (left: boolean) => {
    ctx.beginPath();
    if (left) {
      ctx.moveTo(mx + rr, my);
      ctx.arc(mx + rr, my + rr, rr, -Math.PI / 2, Math.PI, true);
      ctx.lineTo(mx, split);
      ctx.lineTo(mx + rr, split);
    } else {
      ctx.moveTo(mx + rr, my);
      ctx.arc(mx + rr, my + rr, rr, -Math.PI / 2, 0);
      ctx.lineTo(mx + mw, split);
      ctx.lineTo(mx + rr, split);
    }
    ctx.closePath();
  };
  for (const [k, left] of [[0x01, true], [0x02, false]] as const) {
    const down = only !== undefined ? only === k : isDown(k);
    if (only !== undefined && only !== k) continue;
    button(left);
    if (down) {
      ctx.fillStyle = "#ffffff";
      ctx.fill();
    } else if (only === undefined) {
      ctx.strokeStyle = "rgba(255,255,255,0.28)";
      ctx.lineWidth = Math.max(1, u * 0.035);
      ctx.stroke();
    }
  }
  // Molette
  if (only === undefined || only === 0x04) {
    const down = only !== undefined ? true : isDown(0x04);
    const ww = u * 0.2, wh = u * 0.42;
    roundRect(ctx, mx + rr - ww / 2, my + u * 0.32, ww, wh, ww / 2);
    ctx.fillStyle = down ? "#ffffff" : only === undefined ? "rgba(255,255,255,0.35)" : "#ffffff";
    ctx.fill();
  }
  ctx.restore();
}

/** Touches dessinées pour l'export : le fond (rien d'appuyé) puis une image par touche enfoncée. */
export function keysPngs(u: number, labels: Record<string, string>, name: (s: string) => string, preset: KeysPreset = "standard"): { base: string; keys: { key: number; b64: string }[]; w: number; h: number } {
  const S = keysSize(u, preset);
  const c = document.createElement("canvas");
  c.width = S.w;
  c.height = S.h;
  const ctx = c.getContext("2d")!;
  drawKeys(ctx, u, () => false, labels, name, undefined, preset);
  const base = c.toDataURL("image/png").split(",")[1];
  const keys: { key: number; b64: string }[] = [];
  for (const k of [...S.rows.flat().map((x) => x.k).filter((x) => x !== 0), ...MOUSE_KEYS]) {
    drawKeys(ctx, u, () => false, labels, name, k, preset);
    keys.push({ key: k, b64: c.toDataURL("image/png").split(",")[1] });
  }
  return { base, keys, w: S.w, h: S.h };
}

/** Pour un clip : touche enfoncée à l'instant t ? (recherche dichotomique dans les événements triés par touche). */
export function keyIndex(events: { t: number; key: number; down: boolean }[]) {
  const m = new Map<number, { t: number; down: boolean }[]>();
  for (const e of [...events].sort((a, b) => a.t - b.t)) {
    if (!m.has(e.key)) m.set(e.key, []);
    m.get(e.key)!.push(e);
  }
  return (k: number, t: number) => {
    const list = m.get(k);
    if (!list) return false;
    let lo = 0, hi = list.length - 1, ans = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (list[mid].t <= t) { ans = mid; lo = mid + 1; } else hi = mid - 1;
    }
    return ans >= 0 && list[ans].down;
  };
}
