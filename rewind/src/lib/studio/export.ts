// Export du Studio v2 : traduit le projet en description pour le moteur (render.rs).
// Tout ce qui dépend du temps est calculé ICI avec les mêmes fonctions que l'aperçu (model.ts) :
// positions échantillonnées puis simplifiées, en pixels de sortie et en temps du montage.
import { api } from "../api";
import { t } from "../i18n.svelte";
import {
  type Layer, type Project, type AudioItem,
  clampSpeed, fadeWindows, isVariable, frameSize, isFilterFx, kfValue, layerState, layout, opacityAnimated, projectTotal, mergePunches, smooth, speedSegments, typeDur,
  LETTER_IN, LETTER_OUT, letterSpan,
} from "./model";
import { textPng, keysPngs, keysSize, layoutLabels, presetOf, fxPng } from "./render";
import { app } from "../state.svelte";
import { loadFonts } from "./fonts";
import { media } from "./store.svelte";

export type ExportOptions = { title: string; short: number; fps: number; maxMb: number };

type LKf = { t: number; x: number; y: number; s: number; r: number };

/** Simplification d'une courbe (Ramer-Douglas-Peucker) : garde les points utiles à `eps` près. */
function simplify(points: [number, number][], eps: number): Set<number> {
  const keep = new Set<number>([0, points.length - 1]);
  const rec = (a: number, b: number) => {
    if (b - a < 2) return;
    const [ta, va] = points[a], [tb, vb] = points[b];
    let worst = -1, idx = -1;
    for (let i = a + 1; i < b; i++) {
      const [ti, vi] = points[i];
      const lin = va + ((vb - va) * (ti - ta)) / Math.max(1e-9, tb - ta);
      const d = Math.abs(vi - lin);
      if (d > worst) { worst = d; idx = i; }
    }
    if (worst > eps) {
      keep.add(idx);
      rec(a, idx);
      rec(idx, b);
    }
  };
  rec(0, points.length - 1);
  return keep;
}

/** Instants d'échantillonnage : 30 par seconde si le calque bouge, sinon début et fin. */
function sampleTimes(l: Layer): number[] {
  const moving = l.kf.length > 1 || (l.kind === "text" && (l.animIn === "pop" || l.animIn === "slide" || l.animOut === "pop" || l.animOut === "slide"));
  if (!moving) return [0, l.dur];
  const out: number[] = [];
  const step = 1 / 30;
  for (let x = 0; x < l.dur; x += step) out.push(x);
  out.push(l.dur);
  for (const k of l.kf) if (k.t > 0 && k.t < l.dur) out.push(k.t);
  return [...new Set(out)].sort((a, b) => a - b);
}

/** Points clés en pixels de sortie (centre, échelle relative à maxScale, rotation). */
function layerKfs(l: Layer, W: number, H: number, maxScale: number): LKf[] {
  const times = sampleTimes(l);
  const st = times.map((x) => layerState(l, x));
  const props: [string, (i: number) => number, number][] = [
    ["x", (i) => st[i].x * W, 0.5],
    ["y", (i) => st[i].y * H, 0.5],
    ["s", (i) => st[i].scale / maxScale, 0.002],
    ["r", (i) => st[i].rot, 0.1],
  ];
  const keep = new Set<number>();
  for (const [, f, eps] of props) for (const i of simplify(times.map((x, i) => [x, f(i)]), eps)) keep.add(i);
  return [...keep].sort((a, b) => a - b).map((i) => ({ t: l.at + times[i], x: st[i].x * W, y: st[i].y * H, s: Math.min(1, st[i].scale / maxScale), r: st[i].rot }));
}

/** Plus grande échelle atteinte (le texte est dessiné à cette taille, puis réduit : jamais flou). */
function maxScaleOf(l: Layer): number {
  let m = 0.01;
  for (const x of sampleTimes(l)) m = Math.max(m, layerState(l, x).scale);
  return m;
}

/** Courbe {t, v} en temps local → [t, v] échantillonnée avec la même courbe douce que l'aperçu. */
function smoothCurve(kf: { t: number; [k: string]: number }[], prop: string, offset = 0): [number, number][] {
  const list = kf.filter((k) => typeof k[prop] === "number");
  if (list.length < 2) return list.map((k) => [k.t + offset, k[prop]]);
  const out: [number, number][] = [];
  for (let i = 1; i < list.length; i++) {
    const a = list[i - 1], b = list[i];
    for (let s = 0; s < 8; s++) {
      const p = s / 8;
      out.push([a.t + (b.t - a.t) * p + offset, a[prop] + (b[prop] - a[prop]) * smooth(p)]);
    }
  }
  const last = list[list.length - 1];
  out.push([last.t + offset, last[prop]]);
  return out;
}

async function png(b64: string): Promise<string> {
  return api.studioPng(b64);
}

/** Compile le projet. `onStep` : texte d'avancement de la préparation (avant FFmpeg). */
export async function compile(p: Project, o: ExportOptions, onStep: (s: string) => void = () => {}) {
  const { w: W, h: H } = frameSize(p.format, 1080);
  const lay = layout(p.pieces);
  const total = projectTotal(p, lay);
  await loadFonts();

  // ---- Piste principale (ralentis / accélérés : marches fines, appliquées en une courbe par le moteur) ----
  const pieces: any[] = [];
  p.pieces.forEach((pc) => {
    const color = [pc.color.b, pc.color.c, pc.color.s];
    const tracks = media.separateTracks(pc.path).length ? pc.trackVolumes : [];
    if (pc.kind === "freeze") {
      pieces.push({ path: pc.path, start: pc.start, end: pc.start + 0.05, speed: 1, volume: 0, track_volumes: [], transition: pc.transition, transition_dur: pc.transitionDur, clip_id: "", freeze: Math.max(0.1, pc.hold), color });
      return;
    }
    const segs = isVariable(pc) ? speedSegments(pc).map((s) => [s.start, s.end, clampSpeed(s.speed)]) : [];
    pieces.push({
      path: pc.path, start: pc.start, end: pc.end, speed: clampSpeed(pc.speed), volume: pc.volume, track_volumes: tracks,
      transition: pc.transition, transition_dur: pc.transitionDur, clip_id: pc.clipId, freeze: 0, color, segs,
    });
  });

  // ---- Zoom animé : points de chaque morceau en temps du montage, 1 (pas de zoom) ailleurs ----
  const zoom: { t: number; z: number; x: number; y: number }[] = [];
  p.pieces.forEach((pc, i) => {
    if (!pc.zoom.length || pc.zoom.every((k) => (k.z ?? 1) <= 1.001)) return;
    const a = lay.offsets[i], b = a + lay.lens[i];
    const at = (lt: number) => ({ z: kfValue(pc.zoom, "z", lt, 1), x: kfValue(pc.zoom, "x", lt, 0.5), y: kfValue(pc.zoom, "y", lt, 0.5) });
    zoom.push({ t: Math.max(0, a - 0.001), z: 1, x: 0.5, y: 0.5 }, { t: a, ...at(0) });
    const zs = smoothCurve(pc.zoom, "z"), xs = smoothCurve(pc.zoom, "x"), ys = smoothCurve(pc.zoom, "y");
    zs.forEach(([lt], k) => {
      if (lt > 0 && lt < b - a) zoom.push({ t: a + lt, z: zs[k][1], x: xs[k]?.[1] ?? 0.5, y: ys[k]?.[1] ?? 0.5 });
    });
    zoom.push({ t: b, ...at(b - a) }, { t: b + 0.001, z: 1, x: 0.5, y: 0.5 });
  });
  zoom.sort((x, y) => x.t - y.t);
  mergePunches(p, zoom);

  // ---- Calques : de la piste du bas à celle du haut ----
  const layers: any[] = [];
  const effects: { kind: string; at: number; dur: number; strength: number }[] = [];
  const order = [...p.tracks].reverse();
  let done = 0;
  const count = order.reduce((a, tr) => a + (tr.kind === "layer" && !tr.hidden ? tr.items.length : 0), 0);
  for (const tr of order) {
    if (tr.kind !== "layer" || tr.hidden) continue;
    for (const l of tr.items) {
      onStep(t("Préparation des calques {n}/{m}", { n: ++done, m: count }));
      const { fin, fout } = fadeWindows(l);
      const opKf = opacityAnimated(l) ? smoothCurve(l.kf, "opacity", l.at) : [];
      const base = { at: l.at, dur: l.dur, opacity: opacityAnimated(l) ? 1 : l.opacity, op_kf: opKf, fade_in: fin, fade_out: fout, windows: [] as [number, number][] };
      if (l.kind === "fx" && l.fx === "punch") continue; // dans le zoom (addPunches)
      if (l.kind === "fx") {
        // Filtre sur toute l'image : appliqué par le moteur après les calques ; voile : image plein cadre
        if (isFilterFx(l)) effects.push({ kind: l.fx, at: l.at, dur: l.dur, strength: l.strength });
        else layers.push({ ...base, kind: "image", path: await png(fxPng(l, W, H)), w: W, h: H, kf: [{ t: l.at, x: W / 2, y: H / 2, s: 1, r: 0 }] });
        continue;
      }
      if (l.kind === "blur") {
        const kf = layerKfs(l, W, H, 1);
        layers.push({ ...base, kind: "blur", path: "", w: l.w * W, h: l.h * H, kf, mode: l.mode, strength: l.strength * (H / 1080) });
        continue;
      }
      const maxScale = maxScaleOf(l);
      const kf = layerKfs(l, W, H, maxScale);
      if (l.kind === "text") {
        const k = (H / 1080) * maxScale;
        if (LETTER_IN.includes(l.animIn) || LETTER_OUT.includes(l.animOut)) {
          // Lettre par lettre : une image du texte toutes les 1/30 s pendant l'animation, le texte fixe entre les deux
          const span = letterSpan(l);
          const inA = LETTER_IN.includes(l.animIn) ? Math.min(span, l.dur) : 0;
          const outA = LETTER_OUT.includes(l.animOut) ? Math.min(span, l.dur - inA) : 0;
          const step = 1 / 30;
          const frames = async (from: number, to: number) => {
            for (let tt = from; tt < to - 1e-4; tt += step) {
              const img = textPng(l, k, 1, Math.min(tt + step / 2, l.dur - 1e-3));
              layers.push({ ...base, kind: "image", path: await png(img.b64), w: img.w, h: img.h, kf, windows: [[l.at + tt, l.at + Math.min(to, tt + step)]] });
            }
          };
          if (inA > 0) await frames(0, inA);
          if (l.dur - inA - outA > 1e-3) {
            const img = textPng(l, k);
            layers.push({ ...base, kind: "image", path: await png(img.b64), w: img.w, h: img.h, kf, windows: [[l.at + inA, l.at + l.dur - outA]] });
          }
          if (outA > 0) await frames(l.dur - outA, l.dur);
        } else if (l.animIn === "type") {
          // Machine à écrire : une image par groupe de lettres, chacune affichée à son tour
          const full = l.text.replace(/\n/g, "").length;
          const steps = Math.max(1, Math.min(40, full));
          const d = typeDur(l);
          for (let s = 1; s <= steps; s++) {
            const r = s / steps;
            const img = textPng(l, k, r);
            const from = l.at + d * ((s - 0.5) / steps);
            const to = s === steps ? l.at + l.dur : l.at + d * ((s + 0.5) / steps);
            layers.push({ ...base, kind: "image", path: await png(img.b64), w: img.w, h: img.h, kf, windows: [[Math.max(l.at, from), to]] });
          }
        } else {
          const img = textPng(l, k);
          layers.push({ ...base, kind: "image", path: await png(img.b64), w: img.w, h: img.h, kf });
        }
        continue;
      }
      const wpx = maxScale * W;
      const hpx = wpx * (l.ih / Math.max(1, l.iw));
      if (l.kind === "video") layers.push({ ...base, kind: "video", path: l.path, w: wpx, h: hpx, kf, src_start: l.srcStart, speed: l.speed, volume: l.volume });
      else layers.push({ ...base, kind: l.kind, path: l.path, w: wpx, h: hpx, kf });
    }
  }

  // ---- Son ----
  const audio: any[] = [];
  for (const tr of p.tracks) {
    if (tr.kind !== "audio" || tr.muted) continue;
    for (const a of tr.items as AudioItem[]) {
      audio.push({
        path: a.path, at: a.at, src_start: a.srcStart, dur: a.dur, volume: a.volume,
        kf: a.kf.length > 1 ? smoothCurve(a.kf, "v") : a.kf.length === 1 ? [[0, a.kf[0].v]] : [],
        fade_in: a.fadeIn, fade_out: a.fadeOut, duck: a.duck,
      });
    }
  }

  // ---- Touches clavier / souris ----
  let keys: any = null;
  const hasKeys = p.keys.on && p.pieces.some((pc) => pc.kind !== "freeze" && (media.inputs[pc.clipId]?.length ?? 0) > 0);
  if (hasKeys) {
    onStep(t("Préparation des touches"));
    const preset = presetOf(app.settings?.overlay_keys);
    const panelW = p.keys.scale * W;
    const u = panelW / keysSize(1, preset).w;
    const pngs = keysPngs(u, layoutLabels(app.settings?.overlay_layout, media.labels), (s) => t(s), preset);
    const sprites = [];
    for (const k of pngs.keys) sprites.push({ key: k.key, path: await png(k.b64) });
    keys = { base: await png(pngs.base), sprites, x: p.keys.x * W - pngs.w / 2, y: p.keys.y * H - pngs.h / 2 };
  }

  return {
    title: o.title, w: W, h: H, fit: p.fit, short: o.short, fps: o.fps, max_mb: o.maxMb, total,
    pieces, zoom, layers, audio, keys, effects, end_fade: Math.min(p.endFade ?? 0, total),
  };
}

/** Jeu le plus présent dans le montage (dossier de rangement de l'export). */
export function mainGame(p: Project): string {
  const games = new Map<string, number>();
  for (const pc of p.pieces) games.set(pc.game, (games.get(pc.game) ?? 0) + (pc.end - pc.start));
  return [...games.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "";
}
