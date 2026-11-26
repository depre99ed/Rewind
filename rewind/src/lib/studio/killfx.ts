// Kills d'un morceau et effets automatiques posés sur chaque kill (2026-11-23).
//
// Moments des kills :
//  - clip avec les repères du jeu (GSI : clips automatiques, et depuis la 0.4.8 ceux faits au raccourci) : le jeu
//    fait foi (aucun kill inventé, aucun oublié) ; chaque repère est recalé sur l'apparition de la ligne dans le
//    kill feed (+0,1 à 0,2 s, à l'image près). Mesuré sur 12 clips : 35 kills sur 35, aucun en trop.
//  - clip sans repères (anciens clips, clips Medal) : la détection du kill feed seule.
import type { Marker } from "../api";
import type { Piece, Transition } from "./model";

/** Repères du jeu qui sont des kills du joueur (pas sa mort, ni une aide, ni une série) */
export const KILL_KINDS = new Set(["kill", "headshot", "onetap", "knife", "zeus", "collateral"]);

/** Écart toléré entre le repère du jeu et l'apparition dans le kill feed (mesuré : +0,1 à +0,2 s) */
const BEFORE = 0.3, AFTER = 0.7;

export type Moment = Marker & { precise: boolean };

/** Repères du morceau, kills recalés sur le kill feed (temps source). */
export function momentsOf(p: Piece, feed: number[] | undefined): Moment[] {
  const markers = p.markers ?? [];
  if (!feed?.length) return markers.map((m) => ({ ...m, precise: false }));
  const used = new Set<number>();
  // décalage appliqué à chaque heure de repère d'origine (une série tombe à la même heure que son kill)
  const shift = new Map<number, number>();
  for (const m of [...markers].sort((a, b) => a.time - b.time)) {
    if (!KILL_KINDS.has(m.kind) || shift.has(m.time)) continue;
    let best = -1;
    feed.forEach((f, i) => {
      if (used.has(i) || f < m.time - BEFORE || f > m.time + AFTER) return;
      if (best < 0 || Math.abs(f - m.time) < Math.abs(feed[best] - m.time)) best = i;
    });
    if (best >= 0) {
      used.add(best);
      shift.set(m.time, feed[best] - m.time);
    }
  }
  const out: Moment[] = markers.map((m) => (shift.has(m.time) ? { ...m, time: m.time + shift.get(m.time)!, precise: true } : { ...m, precise: false }));
  // Sans repère de kill du jeu : les lignes vues dans le kill feed
  if (!markers.some((m) => KILL_KINDS.has(m.kind))) feed.forEach((f) => out.push({ time: f, kind: "kill", label: "Kill", precise: true }));
  return out.sort((a, b) => a.time - b.time);
}

/** Heures (temps source) des kills du morceau, dans les limites du morceau, sans doublon. */
export function killTimes(p: Piece, feed: number[] | undefined): number[] {
  const times = momentsOf(p, feed).filter((m) => KILL_KINDS.has(m.kind)).map((m) => m.time);
  const out: number[] = [];
  for (const t of times.sort((a, b) => a - b)) {
    if (t < p.start || t > p.end) continue;
    if (!out.length || t - out[out.length - 1] > 0.05) out.push(t);
  }
  return out;
}

/**
 * Réglages de « Garder les kills » (choisis au moment de l'appliquer, gardés d'une fois sur l'autre) :
 * marges avant / après chaque kill, coupe des blancs entre deux kills (seulement s'ils sont séparés d'au moins
 * `gap` secondes) et transition posée à ces coupes.
 */
export type KeepKills = { before: number; after: number; cutGaps: boolean; gap: number; transition: Transition; transitionDur: number };
export const KEEP_KILLS_DEFAULT: KeepKills = { before: 2.5, after: 2, cutGaps: true, gap: 8, transition: "none", transitionDur: 0.3 };

/**
 * Passages à garder autour des kills (2026-11-26) : de `before` avant le premier kill à `after` après le dernier.
 * Entre deux kills séparés d'au moins `gap` secondes (si `cutGaps`), le blanc est retiré (chaque kill garde ses
 * marges) ; plus proches, rien ne change. Un blanc de moins d'une seconde n'est pas coupé.
 * Bornés à [start, end] : on ne fait que retirer, jamais rallonger.
 */
export function killPassages(kills: number[], start: number, end: number, o: Pick<KeepKills, "before" | "after" | "cutGaps" | "gap"> = KEEP_KILLS_DEFAULT): { start: number; end: number }[] {
  const out: { start: number; end: number }[] = [];
  const r = (v: number) => Math.round(v * 1000) / 1000;
  let prev = -Infinity;
  for (const k of [...kills].sort((a, b) => a - b)) {
    const a = r(Math.max(start, k - o.before));
    const b = r(Math.min(end, k + o.after));
    const last = out[out.length - 1];
    if (last && (!o.cutGaps || k - prev < o.gap || a - last.end < 1)) last.end = Math.max(last.end, b);
    else if (b - a >= 0.05) out.push({ start: a, end: b });
    prev = k;
  }
  return out;
}

/** Réglages des effets posés à chaque kill (gardés d'une fois sur l'autre). */
export type KillFx = {
  flash: { on: boolean; color: string; strength: number; dur: number };
  shake: { on: boolean; strength: number; dur: number };
  punch: { on: boolean; strength: number; dur: number };
  /** Son à chaque kill : un son fourni (id) ou le tien (id « custom ») */
  sound: { on: boolean; id: string; path: string; name: string; dur: number; volume: number };
  /** Vitesse au kill : < 1 ralenti, > 1 accéléré */
  slow: { on: boolean; speed: number; dur: number };
  /** Décalage (s) par rapport à l'apparition du kill : négatif = un peu avant */
  offset: number;
  scope: "all" | "piece";
  /** Version des réglages (2 : décalage de -0,2 s par défaut) */
  rev?: number;
};

export const KILL_FX_DEFAULT: KillFx = {
  flash: { on: false, color: "#ffffff", strength: 0.6, dur: 0.25 },
  shake: { on: false, strength: 0.45, dur: 0.35 },
  punch: { on: true, strength: 0.5, dur: 0.4 },
  slow: { on: false, speed: 0.35, dur: 0.5 },
  sound: { on: false, id: "hitmarker", path: "", name: "Hitmarker", dur: 0.12, volume: 0.8 },
  // La ligne du kill feed apparaît un peu après le kill : l'effet part 0,2 s avant (réglé à l'usage, 2026-11-23)
  offset: -0.2,
  scope: "all",
  rev: 2,
};

const KEY = "rewind.studio.killfx";
export function loadKillFx(): KillFx {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) || "null");
    // Réglages d'avant la version 2 : l'ancien décalage par défaut (0) devient le nouveau (-0,2 s)
    if (v && typeof v === "object" && !v.rev && !v.offset) v.offset = KILL_FX_DEFAULT.offset;
    if (v && typeof v === "object") return { ...KILL_FX_DEFAULT, ...v, rev: 2, flash: { ...KILL_FX_DEFAULT.flash, ...v.flash }, shake: { ...KILL_FX_DEFAULT.shake, ...v.shake }, punch: { ...KILL_FX_DEFAULT.punch, ...v.punch }, slow: { ...KILL_FX_DEFAULT.slow, ...v.slow }, sound: { ...KILL_FX_DEFAULT.sound, ...v.sound } };
  } catch {}
  return structuredClone(KILL_FX_DEFAULT);
}
export function saveKillFx(c: KillFx) {
  try { localStorage.setItem(KEY, JSON.stringify(c)); } catch {}
}
