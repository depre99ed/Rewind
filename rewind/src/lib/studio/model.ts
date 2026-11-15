// Studio v2 : modèle d'un projet de montage et calculs partagés par l'aperçu ET l'export
// (même formule des deux côtés : ce qu'on voit dans le Studio est ce qui sort dans la vidéo).
//
// Repères de temps :
//  - temps du MONTAGE : de 0 à la durée totale (tête de lecture) ;
//  - temps LOCAL d'un élément : 0 = son début dans le montage (points clés des calques, du son) ;
//  - temps SOURCE : position dans le fichier d'origine (début/fin d'un morceau, rampe de ralenti).
import type { Marker } from "../api";

export type Transition = "none" | "fade" | "black" | "white" | "zoom" | "slide" | "wipe";
export type Format = "16:9" | "9:16" | "1:1";
export type Fit = "fit" | "blur" | "crop";

/** Point clé : `t` en temps local + les valeurs à cet instant. */
export type Kf = { t: number; [prop: string]: number };

export type Color = { b: number; c: number; s: number };
export const NEUTRAL: Color = { b: 1, c: 1, s: 1 };

/** Ralenti progressif : la vitesse descend en douceur jusqu'à `speed`, tient `dur` s (temps source), puis remonte. */
export type Ramp = { at: number; dur: number; speed: number; ease: number };

/** Morceau de la piste principale (un bout de clip, ou une image figée). */
export type Piece = {
  uid: string;
  kind: "clip" | "freeze";
  clipId: string;
  path: string;
  title: string;
  game: string;
  thumb: string;
  srcDur: number;
  start: number;
  end: number;
  /** Arrêt sur image : durée tenue (l'image est celle de `start`) */
  hold: number;
  speed: number;
  ramp: Ramp | null;
  volume: number;
  trackVolumes: number[];
  transition: Transition;
  transitionDur: number;
  color: Color;
  /** Zoom animé : {t, z (1 = normal), x, y (centre du zoom, 0-1)} en temps local de sortie */
  zoom: Kf[];
  markers: Marker[];
};

export type TextAnimIn = "none" | "fade" | "pop" | "slide" | "type";
export type TextAnimOut = "none" | "fade" | "pop" | "slide";

type LayerBase = {
  uid: string;
  at: number;
  dur: number;
  /** Centre (fraction du cadre) */
  x: number;
  y: number;
  /** Largeur / largeur du cadre (texte : facteur de taille, 1 = taille choisie) */
  scale: number;
  rot: number;
  opacity: number;
  kf: Kf[];
  fadeIn: number;
  fadeOut: number;
};

export type TextLayer = LayerBase & {
  kind: "text";
  text: string;
  font: string;
  /** Taille en pixels pour une image de 1080 px de haut */
  size: number;
  color: string;
  stroke: { color: string; width: number } | null;
  shadow: boolean;
  bg: { color: string; opacity: number } | null;
  align: "left" | "center" | "right";
  animIn: TextAnimIn;
  animOut: TextAnimOut;
  animDur: number;
};
export type ImageLayer = LayerBase & { kind: "image" | "gif"; path: string; name: string; iw: number; ih: number };
export type VideoLayer = LayerBase & { kind: "video"; path: string; name: string; iw: number; ih: number; srcStart: number; srcDur: number; speed: number; volume: number };
export type BlurLayer = LayerBase & { kind: "blur"; w: number; h: number; mode: "blur" | "pixel"; strength: number };
export type Layer = TextLayer | ImageLayer | VideoLayer | BlurLayer;

export type AudioItem = {
  uid: string;
  kind: "music" | "sfx" | "voice";
  path: string;
  name: string;
  at: number;
  srcStart: number;
  dur: number;
  srcDur: number;
  volume: number;
  /** Volume dans le temps : {t, v} en temps local */
  kf: Kf[];
  fadeIn: number;
  fadeOut: number;
  /** Baisse automatiquement quand le jeu parle fort (musique) */
  duck: boolean;
};

export type Track =
  | { id: string; kind: "layer"; name: string; hidden: boolean; locked: boolean; items: Layer[] }
  | { id: string; kind: "audio"; name: string; muted: boolean; locked: boolean; items: AudioItem[] };

export type Project = {
  id: string;
  name: string;
  created: number;
  updated: number;
  format: Format;
  /** Comment un clip 16:9 remplit un cadre vertical ou carré */
  fit: Fit;
  pieces: Piece[];
  /** Pistes de calques et de son ; index 0 = tout en haut (affiché par-dessus) */
  tracks: Track[];
  /** Touches clavier / souris incrustées */
  keys: { on: boolean; x: number; y: number; scale: number };
};

export const uid = () => Math.random().toString(36).slice(2, 10);

// ---------------------------------------------------------------------------
// Créations
// ---------------------------------------------------------------------------

export function newProject(name = "Montage"): Project {
  const now = Date.now() / 1000;
  return { id: uid() + uid(), name, created: now, updated: now, format: "16:9", fit: "blur", pieces: [], tracks: [], keys: defaultKeys("16:9") };
}

/** Place des touches par défaut : en bas à gauche en 16:9 ; en vertical, centrées sous l'image. */
export function defaultKeys(format: Format): Project["keys"] {
  if (format === "16:9") return { on: true, x: 0.14, y: 0.86, scale: 0.2 };
  if (format === "1:1") return { on: true, x: 0.5, y: 0.88, scale: 0.36 };
  return { on: true, x: 0.5, y: 0.8, scale: 0.5 };
}

export function normalizePiece(p: Partial<Piece> & { path: string }): Piece {
  return {
    uid: p.uid ?? uid(),
    kind: p.kind ?? "clip",
    clipId: p.clipId ?? "",
    path: p.path,
    title: p.title ?? "Clip",
    game: p.game ?? "",
    thumb: p.thumb ?? "",
    srcDur: p.srcDur ?? Math.max(p.end ?? 0, 0.1),
    start: p.start ?? 0,
    end: p.end ?? p.srcDur ?? 1,
    hold: p.hold ?? 2,
    speed: p.speed ?? 1,
    ramp: p.ramp ?? null,
    volume: p.volume ?? 1,
    trackVolumes: p.trackVolumes ?? [],
    transition: p.transition ?? "none",
    transitionDur: p.transitionDur ?? 0.5,
    color: p.color ?? { ...NEUTRAL },
    zoom: p.zoom ?? [],
    markers: p.markers ?? [],
  };
}

function base(at: number, dur: number): LayerBase {
  return { uid: uid(), at, dur, x: 0.5, y: 0.5, scale: 1, rot: 0, opacity: 1, kf: [], fadeIn: 0, fadeOut: 0 };
}

export type TextStyle = "title" | "subtitle" | "caption" | "gothic";

export function newText(at: number, style: TextStyle = "title"): TextLayer {
  const b = base(at, 3);
  if (style === "gothic") return { ...b, kind: "text", text: "Edit", font: "fraktur", size: 150, color: "#ffffff", stroke: null, shadow: true, bg: null, align: "center", animIn: "fade", animOut: "fade", animDur: 0.5, y: 0.45 };
  if (style === "title") return { ...b, kind: "text", text: "TITRE", font: "anton", size: 120, color: "#ffffff", stroke: { color: "#000000", width: 8 }, shadow: true, bg: null, align: "center", animIn: "pop", animOut: "fade", animDur: 0.35, y: 0.22 };
  if (style === "subtitle") return { ...b, kind: "text", text: "Sous-titre", font: "montserrat", size: 64, color: "#ffffff", stroke: null, shadow: true, bg: { color: "#000000", opacity: 0.55 }, align: "center", animIn: "fade", animOut: "fade", animDur: 0.25, y: 0.8 };
  return { ...b, kind: "text", text: "Légende", font: "inter", size: 48, color: "#ffffff", stroke: { color: "#000000", width: 4 }, shadow: false, bg: null, align: "left", animIn: "slide", animOut: "fade", animDur: 0.3, x: 0.3, y: 0.9 };
}

export function newMediaLayer(kind: "image" | "gif" | "video", path: string, name: string, iw: number, ih: number, at: number, dur: number, frame: { w: number; h: number }): ImageLayer | VideoLayer {
  // Taille de départ : 40 % de la largeur du cadre, sans dépasser 50 % de sa hauteur
  const aspect = ih > 0 ? iw / ih : 16 / 9;
  let scale = 0.4;
  const hFrac = (scale * frame.w) / aspect / frame.h;
  if (hFrac > 0.5) scale *= 0.5 / hFrac;
  const b = { ...base(at, dur), scale, x: 0.7, y: 0.3 };
  if (kind === "video") return { ...b, kind, path, name, iw, ih, srcStart: 0, srcDur: dur, speed: 1, volume: 1 };
  return { ...b, kind, path, name, iw, ih };
}

export function newBlur(at: number, dur: number): BlurLayer {
  return { ...base(at, dur), kind: "blur", w: 0.24, h: 0.08, mode: "blur", strength: 16, x: 0.5, y: 0.15 };
}

export function newAudio(kind: AudioItem["kind"], path: string, name: string, srcDur: number, at: number, total: number): AudioItem {
  const dur = kind === "music" ? Math.max(0.5, Math.min(srcDur, Math.max(total - at, 1))) : srcDur;
  return { uid: uid(), kind, path, name, at, srcStart: 0, dur, srcDur, volume: kind === "music" ? 0.5 : 1, kf: [], fadeIn: kind === "music" ? 1 : 0, fadeOut: kind === "music" ? 2 : 0, duck: kind === "music" };
}

// ---------------------------------------------------------------------------
// Interpolation (même courbe dans l'aperçu et dans les expressions FFmpeg)
// ---------------------------------------------------------------------------

/** Courbe douce entre deux points clés (lente au départ et à l'arrivée). */
export const smooth = (p: number) => p * p * (3 - 2 * p);

/** Valeur d'une propriété à l'instant t (local) ; `fallback` sans point clé. */
export function kfValue(kf: Kf[], prop: string, t: number, fallback: number): number {
  const list = kf.filter((k) => typeof k[prop] === "number");
  if (!list.length) return fallback;
  if (t <= list[0].t) return list[0][prop];
  const last = list[list.length - 1];
  if (t >= last.t) return last[prop];
  for (let i = 1; i < list.length; i++) {
    const a = list[i - 1], b = list[i];
    if (t <= b.t) {
      const p = b.t > a.t ? (t - a.t) / (b.t - a.t) : 1;
      return a[prop] + (b[prop] - a[prop]) * smooth(p);
    }
  }
  return last[prop];
}

export type LayerState = { x: number; y: number; scale: number; rot: number; opacity: number; reveal: number };

/** Durée des animations du texte (jamais plus de la moitié du calque). */
export const animDur = (l: TextLayer) => Math.max(0.05, Math.min(l.animDur, l.dur / 2));

/**
 * Fondus d'opacité d'un calque (entrée, sortie), en rampes LINÉAIRES : c'est exactement le filtre `fade`
 * de FFmpeg à l'export. Fondu du calque et animation du texte : la plus longue des deux l'emporte.
 */
export function fadeWindows(l: Layer): { fin: number; fout: number } {
  let fin = l.fadeIn, fout = l.fadeOut;
  if (l.kind === "text") {
    const d = animDur(l);
    if (l.animIn === "fade" || l.animIn === "slide") fin = Math.max(fin, d);
    if (l.animIn === "pop") fin = Math.max(fin, d / 2);
    if (l.animOut !== "none") fout = Math.max(fout, d);
  }
  return { fin: Math.min(fin, l.dur), fout: Math.min(fout, l.dur) };
}

/** Opacité animée par l'utilisateur (points clés « opacity » différents) ? */
export const opacityAnimated = (l: Layer) => {
  const v = l.kf.filter((k) => typeof k.opacity === "number").map((k) => k.opacity);
  return v.length > 1 && v.some((x) => Math.abs(x - v[0]) > 1e-3);
};

/** État d'un calque à l'instant local t : points clés, fondus, puis animation d'entrée / de sortie du texte. */
export function layerState(l: Layer, t: number): LayerState {
  const x = kfValue(l.kf, "x", t, l.x);
  let y = kfValue(l.kf, "y", t, l.y);
  let scale = kfValue(l.kf, "scale", t, l.scale);
  const rot = kfValue(l.kf, "rot", t, l.rot);
  let opacity = kfValue(l.kf, "opacity", t, l.opacity);
  const { fin, fout } = fadeWindows(l);
  if (fin > 0) opacity *= Math.min(1, Math.max(0, t / fin));
  if (fout > 0) opacity *= Math.min(1, Math.max(0, (l.dur - t) / fout));
  let reveal = 1;
  if (l.kind === "text") {
    const d = animDur(l);
    const pin = Math.min(1, Math.max(0, t / d));
    const pout = Math.min(1, Math.max(0, (l.dur - t) / d));
    if (l.animIn === "pop") scale *= popCurve(pin);
    else if (l.animIn === "slide") y += (1 - smooth(pin)) * 0.06;
    else if (l.animIn === "type") reveal = typeCurve(t, l);
    if (l.animOut === "pop") scale *= 0.85 + 0.15 * smooth(pout);
    else if (l.animOut === "slide") y -= (1 - smooth(pout)) * 0.06;
  }
  return { x, y, scale, rot, opacity: Math.max(0, Math.min(1, opacity)), reveal };
}

/** « Pop » : part de 60 %, dépasse à 108 % puis se pose à 100 %. */
export function popCurve(p: number): number {
  if (p >= 1) return 1;
  if (p < 0.7) return 0.6 + 0.48 * smooth(p / 0.7);
  return 1.08 - 0.08 * smooth((p - 0.7) / 0.3);
}

/** Machine à écrire : durée de frappe (4 × la durée d'animation, au plus 60 % du calque). */
export const typeDur = (l: TextLayer) => Math.max(0.2, Math.min(l.animDur * 4, l.dur * 0.6));

/** Machine à écrire : part des caractères visibles (0-1), à vitesse constante. */
export function typeCurve(t: number, l: TextLayer): number {
  return Math.min(1, Math.max(0, t / typeDur(l)));
}

// ---------------------------------------------------------------------------
// Piste principale : vitesse, rampe, durée, correspondance temps du montage ↔ temps source
// ---------------------------------------------------------------------------

export const clampSpeed = (s: number) => Math.min(4, Math.max(0.25, s || 1));

/** Vitesse à l'instant source s (rampe comprise). */
export function speedAt(p: Piece, s: number): number {
  const v = clampSpeed(p.speed);
  const r = p.ramp;
  if (!r) return v;
  const slow = Math.min(v, Math.max(0.1, r.speed));
  const a = r.at - r.dur / 2, b = r.at + r.dur / 2, e = Math.max(0.01, r.ease);
  if (s < a - e || s > b + e) return v;
  if (s >= a && s <= b) return slow;
  const p01 = s < a ? (s - (a - e)) / e : 1 - (s - b) / e;
  return v + (slow - v) * smooth(Math.min(1, Math.max(0, p01)));
}

/** Découpe pour l'export : intervalles source à vitesse constante (la rampe en marches fines). */
export function speedSegments(p: Piece): { start: number; end: number; speed: number }[] {
  if (!p.ramp) return [{ start: p.start, end: p.end, speed: clampSpeed(p.speed) }];
  const cuts = new Set<number>([p.start, p.end]);
  const r = p.ramp;
  const a = r.at - r.dur / 2, b = r.at + r.dur / 2, e = Math.max(0.01, r.ease);
  // 5 marches par transition : assez fin pour être fluide, sans multiplier les lectures du fichier à l'export
  const STEPS = 5;
  for (let i = 0; i <= STEPS; i++) {
    cuts.add(a - e + (e * i) / STEPS);
    cuts.add(b + (e * i) / STEPS);
  }
  const list = [...cuts].filter((c) => c >= p.start && c <= p.end).sort((x, y) => x - y);
  const out: { start: number; end: number; speed: number }[] = [];
  for (let i = 1; i < list.length; i++) {
    const s0 = list[i - 1], s1 = list[i];
    if (s1 - s0 < 0.02) continue;
    out.push({ start: s0, end: s1, speed: speedAt(p, (s0 + s1) / 2) });
  }
  return out.length ? out : [{ start: p.start, end: p.end, speed: clampSpeed(p.speed) }];
}

/** Durée de sortie d'un morceau (vitesse, rampe ou arrêt sur image compris). */
export function outLen(p: Piece): number {
  if (p.kind === "freeze") return Math.max(0.1, p.hold);
  if (!p.ramp) return Math.max(0.05, p.end - p.start) / clampSpeed(p.speed);
  return Math.max(0.05, speedSegments(p).reduce((acc, s) => acc + (s.end - s.start) / s.speed, 0));
}

/** Temps source pour un temps local de sortie (aperçu). */
export function srcAt(p: Piece, local: number): number {
  if (p.kind === "freeze") return p.start;
  if (!p.ramp) return Math.min(p.end, p.start + Math.max(0, local) * clampSpeed(p.speed));
  let acc = 0;
  for (const s of speedSegments(p)) {
    const len = (s.end - s.start) / s.speed;
    if (local <= acc + len) return s.start + (local - acc) * s.speed;
    acc += len;
  }
  return p.end;
}

/** Temps local de sortie pour un temps source (repères de kills sur la frise) ; null si hors du morceau. */
export function localAt(p: Piece, src: number): number | null {
  if (src < p.start - 1e-6 || src > p.end + 1e-6) return null;
  if (p.kind === "freeze") return 0;
  if (!p.ramp) return (src - p.start) / clampSpeed(p.speed);
  let acc = 0;
  for (const s of speedSegments(p)) {
    if (src <= s.end) return acc + (Math.max(src, s.start) - s.start) / s.speed;
    acc += (s.end - s.start) / s.speed;
  }
  return acc;
}

export type Layout = { lens: number[]; trans: number[]; offsets: number[]; total: number };

/** Positions des morceaux (même calcul que le moteur : transitions = chevauchement). */
export function layout(pieces: Piece[]): Layout {
  const lens = pieces.map(outLen);
  const trans = pieces.map(() => 0);
  for (let k = 1; k < pieces.length; k++) {
    const p = pieces[k - 1];
    if (!p.transition || p.transition === "none") continue;
    trans[k] = Math.min(Math.min(2, Math.max(0.1, p.transitionDur || 0.5)), lens[k - 1] * 0.5, lens[k] * 0.5);
  }
  const offsets: number[] = [];
  let acc = 0;
  for (let k = 0; k < pieces.length; k++) {
    if (k > 0) acc -= trans[k];
    offsets.push(acc);
    acc += lens[k];
  }
  return { lens, trans, offsets, total: pieces.length ? acc : 0 };
}

/** Durée du montage : piste principale, et au moins jusqu'au bout du dernier calque ou son. */
export function projectTotal(p: Project, lay = layout(p.pieces)): number {
  let end = lay.total;
  for (const tr of p.tracks) for (const it of tr.items) end = Math.max(end, it.at + it.dur);
  return end;
}

// ---------------------------------------------------------------------------
// Formats
// ---------------------------------------------------------------------------

/** Taille de sortie (paire) pour une hauteur de référence (le petit côté). */
export function frameSize(format: Format, short = 1080): { w: number; h: number } {
  const long = Math.floor((short * 16) / 9 / 2) * 2;
  if (format === "9:16") return { w: short, h: long };
  if (format === "1:1") return { w: short, h: short };
  return { w: long, h: short };
}

export const FORMAT_RATIO: Record<Format, number> = { "16:9": 16 / 9, "9:16": 9 / 16, "1:1": 1 };

// ---------------------------------------------------------------------------
// Ancien montage (avant la 0.3.0, gardé dans le navigateur) → projet
// ---------------------------------------------------------------------------

export function fromLegacy(s: any): Project | null {
  if (!s || !Array.isArray(s.pieces) || !s.pieces.length) return null;
  const p = newProject(typeof s.title === "string" && s.title ? s.title : "Montage");
  p.pieces = s.pieces.filter((x: any) => x && typeof x.path === "string").map(normalizePiece);
  if (s.format === "9:16-blur" || s.format === "9:16-crop") {
    p.format = "9:16";
    p.fit = s.format === "9:16-crop" ? "crop" : "blur";
    p.keys = defaultKeys("9:16");
  }
  const m = s.music;
  if (m && typeof m.path === "string") {
    const total = layout(p.pieces).total;
    const item: AudioItem = {
      uid: uid(), kind: "music", path: m.path, name: m.name ?? "Musique", at: m.at ?? 0, srcStart: m.offset ?? 0,
      dur: Math.max(0.5, Math.min((m.duration ?? total) - (m.offset ?? 0), total - (m.at ?? 0))), srcDur: m.duration ?? total,
      volume: m.volume ?? 0.5, kf: [], fadeIn: m.fadeIn ?? 1, fadeOut: m.fadeOut ?? 2, duck: m.duck ?? true,
    };
    p.tracks.push({ id: uid(), kind: "audio", name: "Musique", muted: false, locked: false, items: [item] });
  }
  return p;
}
