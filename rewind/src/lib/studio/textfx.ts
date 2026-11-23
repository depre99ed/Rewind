// Rendu « edit » du texte sur canvas : métal 3D (chrome, or, teinté), relief, biseau, éclat (glow) et étincelles.
// Partagé par l'aperçu du Studio et l'export (les textes partent en PNG) : même rendu au pixel près.

export type TextFill = "solid" | "chrome" | "gold" | "metal";

export type TextLook = {
  fill: TextFill;
  /** Couleur du métal teinté (fill = "metal") */
  tint: string;
  /** Épaisseur du relief 3D (0 = plat, 1 = très épais) */
  depth: number;
  /** Biseau : bord intérieur clair en haut, sombre en bas (0-1) */
  bevel: number;
  /** Éclat autour des lettres (null = aucun) ; strength et size de 0 à 1 */
  glow: { color: string; strength: number; size: number } | null;
  /** Petites étincelles sur les arêtes (0 = aucune) */
  sparkle: number;
};

export const FLAT: TextLook = { fill: "solid", tint: "#9fb4ff", depth: 0, bevel: 0, glow: null, sparkle: 0 };

/** Réglages de départ des styles proposés dans le Studio. */
export const LOOKS: { id: string; label: string; look: TextLook }[] = [
  // Les trois chromes des planches d'essais (A, B, C) : chaque police a le sien (Old English en net, Metal Mania en 3D + éclat…)
  { id: "chrome-flat", label: "Chrome net", look: { fill: "chrome", tint: "#9fb4ff", depth: 0, bevel: 0.6, glow: null, sparkle: 0 } },
  { id: "chrome3d", label: "Chrome 3D", look: { fill: "chrome", tint: "#9fb4ff", depth: 0.55, bevel: 0.6, glow: null, sparkle: 0 } },
  { id: "chrome", label: "Chrome 3D + éclat", look: { fill: "chrome", tint: "#9fb4ff", depth: 0.7, bevel: 0.6, glow: { color: "#ffffff", strength: 0.7, size: 0.6 }, sparkle: 0.9 } },
  { id: "gold", label: "Or 3D", look: { fill: "gold", tint: "#9fb4ff", depth: 0.55, bevel: 0.6, glow: { color: "#ffd27a", strength: 0.3, size: 0.45 }, sparkle: 0.4 } },
  { id: "metal", label: "Métal teinté", look: { fill: "metal", tint: "#e0303a", depth: 0.5, bevel: 0.6, glow: { color: "#ff4d57", strength: 0.35, size: 0.5 }, sparkle: 0 } },
  { id: "neon", label: "Néon", look: { fill: "solid", tint: "#9fb4ff", depth: 0, bevel: 0, glow: { color: "#c8f031", strength: 0.75, size: 0.55 }, sparkle: 0 } },
];

// ---------------------------------------------------------------------------
// Couleurs
// ---------------------------------------------------------------------------

function hexRgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h.padEnd(6, "0").slice(0, 6), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
const rgba = (hex: string, a: number) => { const [r, g, b] = hexRgb(hex); return `rgba(${r},${g},${b},${Math.max(0, Math.min(1, a))})`; };
/** Mélange une teinte avec un gris de luminosité l (0-1) : garde le reflet métallique, colore le métal. */
function tintGray(tint: string, l: number, amount = 0.55): string {
  const [r, g, b] = hexRgb(tint);
  const m = (c: number) => Math.round((l * 255) * (1 - amount) + c * l * 1.25 * amount);
  return `rgb(${Math.min(255, m(r))},${Math.min(255, m(g))},${Math.min(255, m(b))})`;
}

/** Dégradé vertical du métal sur la hauteur des lettres : ciel clair, ligne d'horizon sombre, sol brillant. */
function faceGradient(ctx: CanvasRenderingContext2D, look: TextLook, top: number, bottom: number, solid: string): CanvasGradient | string {
  if (look.fill === "solid") return solid;
  const g = ctx.createLinearGradient(0, top, 0, bottom);
  const stops: [number, number][] = [[0, 0.99], [0.22, 0.86], [0.44, 0.97], [0.5, 0.2], [0.58, 0.38], [0.76, 0.74], [0.9, 0.98], [1, 0.66]];
  for (const [o, l] of stops) {
    if (look.fill === "chrome") {
      // léger bleu froid dans les ombres, comme un vrai chrome qui reflète le ciel
      const v = Math.round(l * 255);
      g.addColorStop(o, `rgb(${Math.round(v * 0.96)},${Math.round(v * 0.98)},${Math.min(255, Math.round(v * 1.04 + 6))})`);
    } else if (look.fill === "gold") {
      g.addColorStop(o, `rgb(${Math.min(255, Math.round(255 * Math.min(1, l * 1.08)))},${Math.round(205 * l)},${Math.round(95 * l * l)})`);
    } else {
      g.addColorStop(o, tintGray(look.tint, l));
    }
  }
  return g;
}

/** Couleur des flancs du relief (plus sombre que la face). */
function sideColor(look: TextLook, p: number, solid: string): string {
  // p : 0 = juste sous la face, 1 = fond du relief
  const l = 0.42 - 0.3 * p;
  if (look.fill === "chrome") { const v = Math.round(l * 255); return `rgb(${v},${v + 2},${v + 8})`; }
  if (look.fill === "gold") return `rgb(${Math.round(200 * l)},${Math.round(140 * l)},${Math.round(40 * l)})`;
  if (look.fill === "metal") return tintGray(look.tint, l, 0.7);
  const [r, g, b] = hexRgb(solid);
  return `rgb(${Math.round(r * l)},${Math.round(g * l)},${Math.round(b * l)})`;
}

// ---------------------------------------------------------------------------
// Dessin
// ---------------------------------------------------------------------------

/** Marge à prévoir autour du texte pour le relief et l'éclat (en pixels pour une police de taille px). */
export function lookPad(look: TextLook | null | undefined, px: number): number {
  if (!look) return 0;
  const depth = look.depth > 0 ? px * 0.12 * look.depth : 0;
  const glow = look.glow ? px * (0.15 + 0.5 * look.glow.size) : 0;
  return Math.ceil(Math.max(depth * 1.2, glow) + (look.sparkle > 0 ? px * 0.08 : 0));
}

/** Direction du relief : vers le bas, légèrement vers la droite (lumière en haut à gauche). */
const DIR = { x: 0.35, y: 1 };

/**
 * Dessine une ligne de texte stylée. Le contexte doit déjà avoir sa police ; textBaseline au choix de l'appelant
 * (top et bottom servent seulement au dégradé du métal : haut et bas des lettres).
 */
export function paintLine(
  ctx: CanvasRenderingContext2D,
  s: string,
  x: number,
  y: number,
  px: number,
  look: TextLook,
  opt: { color: string; stroke?: { color: string; width: number } | null; top: number; bottom: number },
) {
  if (!s) return;
  const depth = look.depth > 0 ? px * 0.12 * look.depth : 0;
  // Tout est dessiné aligné à gauche sur la ligne de base : face, relief et contour tombent pile l'un sur l'autre
  const prevAlign = ctx.textAlign, prevBase = ctx.textBaseline;
  const w0 = ctx.measureText(s).width;
  if (prevAlign === "center") x -= w0 / 2;
  else if (prevAlign === "right" || prevAlign === "end") x -= w0;
  if (prevBase !== "alphabetic") {
    const aB = ctx.measureText(s).actualBoundingBoxAscent;
    ctx.textBaseline = "alphabetic";
    y = y - aB + ctx.measureText(s).actualBoundingBoxAscent;
  }
  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";

  // 1) Éclat : deux passes floues (large et douce, puis serrée) derrière tout le reste
  if (look.glow && look.glow.strength > 0) {
    const g = look.glow;
    ctx.save();
    ctx.fillStyle = rgba(g.color, 1);
    for (const [blur, a] of [[px * (0.2 + 0.7 * g.size), 0.55 * g.strength], [px * (0.06 + 0.18 * g.size), 0.9 * g.strength]] as const) {
      ctx.shadowColor = rgba(g.color, a);
      ctx.shadowBlur = blur;
      ctx.globalAlpha = 1;
      // le texte lui-même sert d'émetteur ; il sera recouvert par la face
      ctx.fillText(s, x + DIR.x * depth * 0.5, y + DIR.y * depth * 0.5);
    }
    ctx.restore();
  }

  // 2) Relief : la forme répétée vers le bas, du fond vers la face, de plus en plus claire
  if (depth > 0) {
    const steps = Math.max(2, Math.ceil(depth));
    ctx.save();
    for (let i = steps; i >= 1; i--) {
      const p = i / steps;
      ctx.fillStyle = sideColor(look, p, opt.color);
      ctx.fillText(s, x + DIR.x * depth * p, y + DIR.y * depth * p);
    }
    // arête sombre au fond du relief, pour décoller du décor
    ctx.lineJoin = "round";
    ctx.strokeStyle = "rgba(0,0,0,0.55)";
    ctx.lineWidth = Math.max(1, px * 0.025);
    ctx.strokeText(s, x + DIR.x * depth, y + DIR.y * depth);
    ctx.restore();
  }

  // 3) Contour choisi par l'utilisateur (sous la face)
  if (opt.stroke && opt.stroke.width > 0) {
    ctx.save();
    ctx.lineJoin = "round";
    ctx.strokeStyle = opt.stroke.color;
    ctx.lineWidth = opt.stroke.width;
    ctx.strokeText(s, x, y);
    ctx.restore();
  } else if (look.fill !== "solid") {
    // fin liseré sombre : sépare le métal clair d'un fond clair
    ctx.save();
    ctx.lineJoin = "round";
    ctx.strokeStyle = "rgba(10,12,16,0.85)";
    ctx.lineWidth = Math.max(1, px * 0.03);
    ctx.strokeText(s, x, y);
    ctx.restore();
  }

  // 4) Face (+ biseau dessiné sur un calque à part, limité à la forme des lettres)
  const face = faceGradient(ctx, look, opt.top, opt.bottom, opt.color);
  if (look.bevel <= 0 || typeof document === "undefined") {
    ctx.fillStyle = face;
    ctx.fillText(s, x, y);
  } else {
    const pad = Math.ceil(px * 0.1);
    const c = document.createElement("canvas");
    const o = c.getContext("2d")!;
    o.font = ctx.font;
    o.textAlign = "left";
    o.textBaseline = "alphabetic";
    // Mesure dans le calque lui-même (aligné à gauche, ligne de base) : la mesure du contexte principal
    // dépend de son alignement (centré : la moitié droite du texte sortait du calque)
    const lm = o.measureText(s);
    const asc = Math.ceil(lm.actualBoundingBoxAscent || px), desc = Math.ceil(lm.actualBoundingBoxDescent || px * 0.3);
    const left = Math.ceil(Math.max(0, lm.actualBoundingBoxLeft || 0)), right = Math.ceil(Math.max(lm.width, lm.actualBoundingBoxRight || 0));
    c.width = Math.max(1, left + right + pad * 2);
    c.height = Math.max(1, asc + desc + pad * 2);
    // (redimensionner remet le contexte à zéro)
    o.font = ctx.font;
    o.textAlign = "left";
    o.textBaseline = "alphabetic";
    const ox = pad + left, oy = pad + asc;
    // repère local : même dégradé qu'à l'écran (décalé de la position de la ligne)
    const dy = oy - y;
    const top = opt.top + dy, bottom = opt.bottom + dy;
    o.fillStyle = faceGradient(o, look, top, bottom, opt.color);
    o.fillText(s, ox, oy);
    // Biseau : bord clair en haut à gauche, sombre en bas à droite, seulement à l'intérieur des lettres
    o.globalCompositeOperation = "source-atop";
    const b = Math.max(1, px * 0.035 * look.bevel);
    o.lineJoin = "round";
    o.lineWidth = b * 2;
    o.strokeStyle = "rgba(255,255,255,0.9)";
    o.strokeText(s, ox + b * 0.5, oy + b * 0.7);
    o.strokeStyle = "rgba(0,0,0,0.55)";
    o.strokeText(s, ox - b * 0.6, oy - b * 0.8);
    // reflet fin sur le haut des lettres
    o.lineWidth = Math.max(1, b * 0.6);
    o.strokeStyle = "rgba(255,255,255,0.85)";
    o.strokeText(s, ox + b * 0.9, oy + b * 1.2);
    o.globalCompositeOperation = "source-over";
    // replace le calque : l'origine du texte (ox, oy) tombe sur (x, y)
    ctx.drawImage(c, x - ox, y - oy);
  }

  // 5) Étincelles : petites croix lumineuses sur quelques arêtes hautes (positions stables, liées au texte)
  if (look.sparkle > 0) {
    const n = Math.max(1, Math.round(s.length * 0.35 * look.sparkle));
    let seed = 0;
    for (const ch of s) seed = (seed * 31 + ch.charCodeAt(0)) % 9973;
    const rnd = () => ((seed = (seed * 9301 + 49297) % 233280) / 233280);
    for (let i = 0; i < n; i++) {
      const sx = x + w0 * (0.08 + 0.84 * rnd());
      const sy = opt.top + (opt.bottom - opt.top) * (0.05 + 0.35 * rnd());
      star(ctx, sx, sy, px * (0.07 + 0.08 * rnd()) * (0.6 + 0.4 * look.sparkle));
    }
  }
  ctx.textAlign = prevAlign;
  ctx.textBaseline = prevBase;
}

function star(ctx: CanvasRenderingContext2D, x: number, y: number, r: number) {
  ctx.save();
  const g = ctx.createRadialGradient(x, y, 0, x, y, r);
  g.addColorStop(0, "rgba(255,255,255,1)");
  g.addColorStop(0.25, "rgba(255,255,255,0.65)");
  g.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = g;
  ctx.beginPath();
  // croix fine à 4 branches + petit halo
  const t = r * 0.09;
  ctx.moveTo(x - r, y); ctx.lineTo(x - t, y - t); ctx.lineTo(x, y - r); ctx.lineTo(x + t, y - t);
  ctx.lineTo(x + r, y); ctx.lineTo(x + t, y + t); ctx.lineTo(x, y + r); ctx.lineTo(x - t, y + t);
  ctx.closePath();
  ctx.fill();
  ctx.beginPath();
  ctx.arc(x, y, r * 0.35, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}
