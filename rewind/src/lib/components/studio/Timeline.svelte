<script lang="ts">
  // Frise multi-pistes : calques en haut (la piste du haut s'affiche par-dessus), vidéo principale, son en bas.
  import {
    Scissors, Trash2, CopyPlus, Undo2, Redo2, Magnet, Link2, ZoomIn, ZoomOut, Maximize2, Snowflake, Eye, EyeOff, Lock, LockOpen,
    ChevronUp, ChevronDown, Volume2, VolumeX, Type, Image as ImageIcon, Film, Music, Mic, Droplets, Video, Blend, AudioLines, Sparkles, FileX,
    ArrowLeftToLine, ArrowRightToLine, Crosshair,
  } from "@lucide/svelte";
  import { untrack } from "svelte";
  import { studio, media, fmtTime, TRANSITIONS } from "$lib/studio/store.svelte";
  import { KEEP_KILLS_DEFAULT } from "$lib/studio/killfx";
  import Slider from "./Slider.svelte";
  import { outLen, localAt, srcAt, clampSpeed, fxInfo, isFullBlur, isVariable, hasCurve, type Layer, type AudioItem, type Piece, type Track } from "$lib/studio/model";
  import { momentTone } from "$lib/moments";
  import { fontOf } from "$lib/studio/fonts";
  import MomentIcon from "../MomentIcon.svelte";
  import { fileUrl, type Waveform } from "$lib/api";
  import { app } from "$lib/state.svelte";
  import { t } from "$lib/i18n.svelte";

  const HEAD = 176; // colonne des noms de pistes
  const PAD = 16;
  // Piste vidéo plus haute (2026-11-25) : vignettes lisibles (62 px de haut au lieu de 40)
  const MAIN_H = 84;
  const LAYER_H = 36;
  const AUDIO_H = 44;
  const SNAP_PX = 8;

  let scroller: HTMLDivElement | undefined = $state();
  let width = $state(900);
  let scrollX = $state(0);

  const P = $derived(studio.project);
  const L = $derived(studio.lay);
  /**
   * Échelle gardée pendant et après un rognage (comme Medal) : avant, l'échelle suivait la durée du montage,
   * donc raccourcir un morceau zoomait toute la frise sous la souris (« ça nous tire sur le côté »).
   * Revient à la normale au bouton « Tout voir », en changeant de projet ou de nombre de morceaux.
   */
  let keepTotal = $state(0);
  const total = $derived(Math.max(studio.total, keepTotal, 4));
  // (clé simple : l'effet ne repart que si le projet ou le nombre de morceaux change, pas à chaque modification)
  const pieceKey = $derived(`${P.id}:${P.pieces.length}`);
  $effect(() => {
    void pieceKey;
    untrack(() => (keepTotal = 0));
  });
  const fitPps = $derived(Math.max(2, (width - HEAD - PAD * 2 - 40) / total));
  const pps = $derived(fitPps * studio.zoom);
  const contentW = $derived(HEAD + PAD * 2 + total * pps + 200);
  const X = (s: number) => HEAD + PAD + s * pps;
  function tAt(clientX: number) {
    const r = scroller!.getBoundingClientRect();
    return (clientX - r.left + scroller!.scrollLeft - HEAD - PAD) / pps;
  }

  const layerTracks = $derived(P.tracks.filter((tr) => tr.kind === "layer"));
  const killPieces = $derived(studio.killPieces());
  // Réglages de « Garder les kills » : panneau ouvert sur le bouton, résultat calculé en direct
  let kkOpen = $state(false);
  let kkEl: HTMLElement | undefined = $state();
  const kk = $derived(studio.keepKills);
  const kkPlan = $derived(kkOpen ? studio.keepKillsPlan(kk) : { plan: new Map(), gaps: 0, before: 0, after: 0 });
  const sec = (v: number) => `${v.toFixed(1)} s`;
  $effect(() => {
    if (!kkOpen) return;
    const down = (e: PointerEvent) => { if (kkEl && !kkEl.contains(e.target as Node)) kkOpen = false; };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); kkOpen = false; } };
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("keydown", key, true);
    return () => { window.removeEventListener("pointerdown", down, true); window.removeEventListener("keydown", key, true); };
  });
  const audioTracks = $derived(P.tracks.filter((tr) => tr.kind === "audio"));

  // Graduations
  const step = $derived([0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300].find((s) => s * pps >= 90) ?? 300);
  const ticks = $derived.by(() => {
    const out: number[] = [];
    const first = Math.max(0, Math.floor((scrollX - PAD) / pps / step) * step);
    const last = (scrollX + width) / pps + step;
    for (let s = first; s <= last; s += step) out.push(+s.toFixed(3));
    return out;
  });
  const fmtTick = (s: number) => (step < 1 ? fmtTime(s, true).replace(/0$/, "") : fmtTime(s));

  // ---------- vignettes et forme d'onde ----------
  function tiles(p: Piece, left: number, w: number) {
    const s = media.strips[p.path];
    const tileW = s ? s.frame_w * ((MAIN_H - 22) / s.frame_h) : ((MAIN_H - 22) * 16) / 9;
    // Grille calée sur le clip source, pas sur le bord du morceau : quand on rogne le début, les vignettes restent
    // en place sous la souris au lieu de glisser (2026-11-25).
    const shift = p.kind === "freeze" ? 0 : ((p.start / clampSpeed(p.speed)) * pps) % tileW;
    const n = Math.ceil((w + shift) / tileW);
    const j0 = Math.max(0, Math.floor((scrollX - left + shift) / tileW) - 1);
    const j1 = Math.min(n, Math.ceil((scrollX + width - left + shift) / tileW) + 1);
    const list: { left: number; pos: number }[] = [];
    for (let j = j0; j < j1; j++) {
      const x = j * tileW - shift;
      const src = p.kind === "freeze" ? p.start : srcAt(p, Math.max(0, Math.min(w, x + tileW / 2)) / pps);
      const fi = s ? Math.min(s.frames - 1, Math.max(0, Math.floor(src / s.interval))) : 0;
      list.push({ left: x, pos: -fi * tileW });
    }
    return { tileW, list, s };
  }
  type WaveParams = { w: Waveform | undefined; start: number; end: number; width: number; height: number; v: number };
  function wave(canvas: HTMLCanvasElement, params: WaveParams) {
    const draw = (q: WaveParams) => {
      const cssW = Math.max(1, Math.min(q.width, 16000));
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      canvas.width = Math.round(cssW * dpr);
      canvas.height = Math.round(q.height * dpr);
      canvas.style.width = `${q.width}px`;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.fillStyle = "rgba(255,255,255,0.8)";
      const w = q.w;
      if (!w || !w.peaks.length) {
        ctx.globalAlpha = 0.25;
        ctx.fillRect(0, canvas.height / 2 - dpr * 0.5, canvas.width, dpr);
        return;
      }
      const bar = 2 * dpr, gap = 1 * dpr, span = q.end - q.start;
      const gain = Math.min(1.6, Math.max(0, q.v));
      ctx.globalAlpha = q.v <= 0.001 ? 0.25 : 0.85;
      for (let xpx = 0; xpx < canvas.width; xpx += bar + gap) {
        const a = q.start + (xpx / canvas.width) * span;
        const b = q.start + ((xpx + bar + gap) / canvas.width) * span;
        let peak = 0;
        for (let s = a; s < b; s += 1 / w.rate) peak = Math.max(peak, w.peaks[Math.floor(s * w.rate)] ?? 0);
        const h = Math.max(dpr, (peak / 255) * gain * (canvas.height - 2 * dpr));
        ctx.fillRect(xpx, (canvas.height - h) / 2, bar, h);
      }
    };
    draw(params);
    return { update: draw };
  }

  // ---------- aimantation ----------
  /** Instant où l'élément s'est calé (trait vertical affiché pendant le glissement). */
  let snapLine = $state<number | null>(null);
  /** `head` : tête de lecture à viser (null : aucune). Pendant un rognage, c'est sa place au début du geste. */
  function snapPoints(exclude?: string, head: number | null = studio.t): number[] {
    const pts = head !== null ? [head, 0] : [0];
    L.offsets.forEach((o, i) => {
      pts.push(o, o + L.lens[i]);
      // Les kills aussi : un texte ou un son se cale pile sur le moment
      for (const m of studio.momentsOf(P.pieces[i])) {
        const lt = localAt(P.pieces[i], m.time);
        if (lt !== null) pts.push(o + lt);
      }
    });
    for (const tr of P.tracks) for (const it of tr.items as { uid: string; at: number; dur: number }[]) if (it.uid !== exclude) pts.push(it.at, it.at + it.dur);
    return pts;
  }
  function snap(v: number, exclude?: string, head: number | null = studio.t): number {
    return snapTo(v, snapPoints(exclude, head));
  }
  function snapTo(v: number, pts: number[]): number {
    if (!studio.snap) {
      snapLine = null;
      return v;
    }
    let best = v, d = SNAP_PX / pps, hit = false;
    for (const p of pts) if (Math.abs(p - v) < d) { d = Math.abs(p - v); best = p; hit = true; }
    snapLine = hit ? best : null;
    return best;
  }
  /**
   * Points d'aimant pour la fin du morceau i pendant qu'on la rogne : seulement ce qui ne bouge pas avec le bord.
   * (2026-11-25 : le bord s'aimantait à SA PROPRE place et au début du morceau suivant, qui le suivent : il collait
   * et avançait par sauts de 8 px derrière la souris.)
   */
  function trimPoints(i: number, head: number): number[] {
    const edge = L.offsets[i] + L.lens[i];
    const pts = [head, 0, L.offsets[i]];
    for (let k = 0; k < i; k++) pts.push(L.offsets[k], L.offsets[k] + L.lens[k]);
    for (let k = 0; k <= i; k++) {
      for (const m of studio.momentsOf(P.pieces[k])) {
        const lt = localAt(P.pieces[k], m.time);
        if (lt !== null) pts.push(L.offsets[k] + lt);
      }
    }
    for (const tr of P.tracks) for (const it of tr.items as (Layer | AudioItem)[]) {
      const moves = studio.follow && (tr.kind === "layer" || (it as AudioItem).kind !== "music");
      if (!moves || it.at < edge - 1e-3) pts.push(it.at, it.at + it.dur);
    }
    return pts;
  }
  /** Fin (temps source) qui donne au morceau la durée `len` à l'écran : vitesse, ralentis et courbe compris. */
  function solveEnd(p: Piece, len: number): number {
    let lo = Math.min(p.srcDur, p.start + 0.2), hi = p.srcDur;
    if (outLen({ ...p, end: hi }) <= len) return hi;
    if (outLen({ ...p, end: lo }) >= len) return lo;
    for (let n = 0; n < 40; n++) {
      const m = (lo + hi) / 2;
      if (outLen({ ...p, end: m }) < len) lo = m;
      else hi = m;
    }
    return Math.round(((lo + hi) / 2) * 1000) / 1000;
  }
  /** Début (temps source) qui donne au morceau la durée `len` à l'écran. */
  function solveStart(p: Piece, len: number): number {
    let lo = 0, hi = Math.max(0, p.end - 0.2);
    if (outLen({ ...p, start: lo }) <= len) return lo;
    if (outLen({ ...p, start: hi }) >= len) return hi;
    for (let n = 0; n < 40; n++) {
      const m = (lo + hi) / 2;
      if (outLen({ ...p, start: m }) > len) lo = m;
      else hi = m;
    }
    return Math.round(((lo + hi) / 2) * 1000) / 1000;
  }

  // ---------- glisser ----------
  type Drag =
    | { kind: "scrub" }
    | { kind: "item"; id: string; track: string; tkind: "layer" | "audio"; video: boolean; x0: number; y0: number; at0: number; started: boolean }
    | { kind: "trimItem"; id: string; side: "start" | "end"; x0: number; t0: number; at0: number; dur0: number; src0: number; max: number }
    | { kind: "trimPiece"; id: string; side: "start" | "end"; x0: number; t0: number; freeze: boolean; len0: number; off0: number[]; total0: number }
    | { kind: "piece"; id: string; from: number; x0: number; y0: number; started: boolean; to: number };
  let drag = $state<Drag | null>(null);
  /**
   * Rognage (revu le 2026-11-25) :
   *  - FIN d'un morceau : en direct. Le bord reste sous la souris et ce qui suit recule avec lui ; rien ne saute au lâcher.
   *  - DÉBUT d'un morceau : le bord tiré suit la souris, le reste du montage reste affiché à sa place (positions du
   *    début du geste, `ghost`), puis se recolle au lâcher.
   * Dans les deux cas, l'aperçu montre l'image du bord tiré (la tête de lecture le suit) : on voit où on coupe.
   */
  let ghost = $state<{ i: number; len0: number; off0: number[]; total0: number } | null>(null);
  /** Juste après un rognage du début : le recollage est animé (jamais pendant un glissement) */
  let settling = $state(false);
  /** Décalage d'affichage du morceau k pendant le rognage d'un début (exact, transitions comprises) */
  function pieceShift(k: number): number {
    const g = ghost;
    if (!g || k < g.i) return 0;
    if (k === g.i) return g.len0 - (L.lens[k] ?? g.len0);
    return (g.off0[k] ?? L.offsets[k]) - L.offsets[k];
  }
  /** Décalage d'affichage de ce qui se trouve à l'instant t du montage (éléments liés à l'image, tête de lecture) */
  function shiftAt(t: number): number {
    const g = ghost;
    if (!g) return 0;
    let k = -1;
    for (let j = 0; j < L.offsets.length; j++) if (L.offsets[j] <= t + 1e-6) k = j;
    if (k < 0) return 0;
    if (t > L.offsets[k] + L.lens[k] + 1e-6) return g.total0 - L.total; // après la fin de la piste vidéo
    return pieceShift(k);
  }
  /** Décalage d'affichage d'un élément : seuls ceux qui suivent l'image (la musique reste fixe) */
  function itemShift(it: { at: number; kind: string }, audio: boolean): number {
    if (!ghost || !studio.follow || (audio && it.kind === "music")) return 0;
    return shiftAt(it.at);
  }
  let tip = $state<{ x: number; text: string } | null>(null);
  /** Piste visée pendant un glissement : « layer:<id> », « audio:<id> », « main », « new-layer », « new-audio » */
  let over = $state<string | null>(null);
  const dragging = $derived((drag?.kind === "item" || drag?.kind === "piece") && drag.started);
  function rowAt(e: { clientX: number; clientY: number }): string | null {
    return (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest<HTMLElement>("[data-row]")?.dataset.row ?? null;
  }
  /** Place d'insertion dans la piste vidéo pour un temps donné. */
  function slotAt(x: number, from = -1) {
    let to = P.pieces.length;
    for (let i = 0; i < P.pieces.length; i++) if (x < L.offsets[i] + L.lens[i] / 2) { to = i; break; }
    return from >= 0 && to > from ? to - 1 : to;
  }

  function scrubDown(e: PointerEvent) {
    if (e.button !== 0) return;
    studio.playing = false;
    drag = { kind: "scrub" };
    studio.seek(snap(tAt(e.clientX), undefined, null));
  }
  function itemDown(e: PointerEvent, it: Layer | AudioItem, tr: Track) {
    if (e.button !== 0) return;
    e.stopPropagation();
    studio.select({ type: tr.kind === "audio" ? "audio" : "layer", id: it.uid });
    if (tr.locked) return;
    drag = { kind: "item", id: it.uid, track: tr.id, tkind: tr.kind, video: it.kind === "video", x0: e.clientX, y0: e.clientY, at0: it.at, started: false };
  }
  function trimDown(e: PointerEvent, it: Layer | AudioItem, side: "start" | "end", tr: Track) {
    if (e.button !== 0) return;
    e.stopPropagation();
    keepTotal = total;
    if (tr.locked) return;
    studio.playing = false;
    studio.select({ type: tr.kind === "audio" ? "audio" : "layer", id: it.uid });
    studio.beginGesture();
    const src0 = "srcStart" in it ? it.srcStart : 0;
    const speed = it.kind === "video" ? (it as any).speed : 1;
    const max = "srcDur" in it ? ((it as any).srcDur - src0) / speed : Infinity;
    drag = { kind: "trimItem", id: it.uid, side, x0: e.clientX, t0: studio.t, at0: it.at, dur0: it.dur, src0, max };
  }
  function pieceDown(e: PointerEvent, p: Piece, i: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    studio.select({ type: "piece", id: p.uid });
    drag = { kind: "piece", id: p.uid, from: i, x0: e.clientX, y0: e.clientY, started: false, to: i };
  }
  function pieceTrimDown(e: PointerEvent, p: Piece, side: "start" | "end") {
    if (e.button !== 0) return;
    e.stopPropagation();
    keepTotal = total;
    studio.playing = false;
    studio.select({ type: "piece", id: p.uid });
    studio.beginGesture();
    drag = { kind: "trimPiece", id: p.uid, side, x0: e.clientX, t0: studio.t, freeze: p.kind === "freeze", len0: outLen(p), off0: [...L.offsets], total0: L.total };
  }
  function onMove(e: PointerEvent) {
    const d = drag;
    if (!d) return;
    if (d.kind === "scrub") return studio.seek(snap(tAt(e.clientX), undefined, null));
    const dt = (e.clientX - ("x0" in d ? d.x0 : 0)) / pps;
    if (d.kind === "item") {
      if (!d.started && Math.abs(e.clientX - d.x0) + Math.abs(e.clientY - d.y0) < 4) return;
      if (!d.started) { studio.beginGesture(); d.started = true; }
      const it = findItem(d.id);
      if (!it) return;
      // Piste visée : celle sous le pointeur. Même type : l'élément y passe tout de suite ;
      // piste vidéo (pour une vidéo) ou « nouvelle piste » : au lâcher.
      over = rowAt(e);
      const same = over?.startsWith(d.tkind + ":") ? over.slice(d.tkind.length + 1) : undefined;
      let at = Math.max(0, d.at0 + dt);
      const sa = snap(at, d.id), sb = snap(at + it.dur, d.id) - it.dur;
      at = Math.abs(sa - at) <= Math.abs(sb - at) ? sa : sb;
      studio.moveItem(d.id, Math.max(0, at), same, false);
      tip = { x: X(at), text: fmtTime(at, true) };
    } else if (d.kind === "trimItem") {
      const it = findItem(d.id);
      if (!it) return;
      if (d.side === "end") {
        const end = snap(d.at0 + d.dur0 + dt, d.id, d.t0);
        const dur = Math.max(0.1, Math.min(d.max, end - d.at0));
        patchItem(d.id, { dur });
        studio.seek(d.at0 + dur - 0.001); // l'aperçu montre la dernière image gardée
        tip = { x: X(d.at0 + dur), text: fmtTime(dur, true) };
      } else {
        let at = snap(d.at0 + dt, d.id, d.t0);
        at = Math.max(0, Math.min(d.at0 + d.dur0 - 0.1, at));
        const shift = at - d.at0;
        if ("srcStart" in it && d.src0 + shift < 0) at = d.at0 - d.src0;
        const s2 = at - d.at0;
        const patch: Record<string, unknown> = { at, dur: d.dur0 - s2 };
        if ("srcStart" in it) patch.srcStart = d.src0 + s2 * (it.kind === "video" ? (it as any).speed : 1);
        patchItem(d.id, patch);
        studio.seek(at);
        tip = { x: X(at), text: fmtTime(d.dur0 - s2, true) };
      }
    } else if (d.kind === "trimPiece") {
      const p = P.pieces.find((x) => x.uid === d.id);
      if (!p) return;
      const i = P.pieces.indexOf(p);
      const off = L.offsets[i] ?? 0;
      // Durée voulue à l'écran : le bord tiré reste sous la souris, même avec ralentis ou courbe de vitesse
      // (avant, le geste était converti en secondes du clip à vitesse fixe : sur un mini-ralenti de kill, le bord
      // partait 3 fois plus loin que la souris).
      if (d.side === "end") {
        const want = Math.max(0, snapTo(off + d.len0 + dt, trimPoints(i, d.t0)) - off);
        if (d.freeze) studio.updatePiece(d.id, { hold: Math.max(0.2, Math.min(10, Math.round(want * 1000) / 1000)) }, false);
        else studio.updatePiece(d.id, { end: solveEnd(p, want) }, false);
        const len = studio.lay.lens[i] ?? want;
        studio.seek(off + len - 0.001); // l'aperçu montre la dernière image gardée
        tip = { x: X(off + len), text: fmtTime(len, true) };
      } else {
        const want = Math.max(0, d.len0 - dt);
        if (d.freeze) studio.updatePiece(d.id, { hold: Math.max(0.2, Math.min(10, Math.round(want * 1000) / 1000)) }, false);
        else studio.updatePiece(d.id, { start: solveStart(p, want) }, false);
        const len = studio.lay.lens[i] ?? want;
        ghost = { i, len0: d.len0, off0: d.off0, total0: d.total0 };
        studio.seek(off); // l'aperçu montre la première image gardée
        tip = { x: X(off + d.len0 - len), text: fmtTime(len, true) };
      }
    } else if (d.kind === "piece") {
      if (!d.started && Math.abs(e.clientX - d.x0) + Math.abs(e.clientY - d.y0) < 5) return;
      d.started = true;
      over = rowAt(e);
      d.to = slotAt(tAt(e.clientX), d.from);
      if (over?.startsWith("layer:") || over === "new-layer") tip = { x: X(Math.max(0, tAt(e.clientX))), text: t("Par-dessus la vidéo") };
      else tip = null;
    }
  }
  function onUp(e: PointerEvent) {
    const d = drag;
    const target = over;
    drag = null;
    if (ghost) {
      settling = true;
      setTimeout(() => (settling = false), 260);
    }
    // Fin du geste tenu : une seule action pour Ctrl+Z
    if (d?.kind === "trimPiece" || d?.kind === "trimItem" || (d?.kind === "item" && d.started)) studio.endGesture();
    ghost = null;
    tip = null;
    over = null;
    snapLine = null;
    if (!d) return;
    if (d.kind === "piece" && d.started) {
      // Un clip monté dans une piste de calques devient une vidéo par-dessus
      if (target?.startsWith("layer:")) return studio.pieceToLayer(d.id, Math.max(0, tAt(e.clientX)), target.slice(6));
      if (target === "new-layer") return studio.pieceToLayer(d.id, Math.max(0, tAt(e.clientX)), null);
      if (d.to !== d.from) studio.movePiece(d.from, d.to);
    } else if (d.kind === "item" && d.started) {
      const it = findItem(d.id);
      if (!it) return;
      if (target === "main" && d.video) return studio.layerToPiece(d.id, slotAt(tAt(e.clientX)));
      if ((target === "new-layer" && d.tkind === "layer") || (target === "new-audio" && d.tkind === "audio")) return studio.moveToNewTrack(d.id, it.at);
    }
  }
  function findItem(id: string): (Layer | AudioItem) | null {
    for (const tr of P.tracks) for (const it of tr.items as (Layer | AudioItem)[]) if (it.uid === id) return it;
    return null;
  }
  function patchItem(id: string, patch: Record<string, unknown>) {
    const tr = studio.trackOf(id);
    if (tr?.kind === "audio") studio.updateAudio(id, patch as Partial<AudioItem>, false);
    else studio.updateLayer(id, patch as Partial<Layer>, false);
  }

  // Clips glissés depuis la liste : piste vidéo = à la suite, piste de calques = par-dessus à l'endroit visé
  let dropAt = $state<number | null>(null);
  let clipOver = $state<string | null>(null);
  const isClip = (e: DragEvent) => !!e.dataTransfer?.types.includes("application/x-rewind-clip");
  function dragOver(e: DragEvent) {
    if (!isClip(e)) return;
    e.preventDefault();
    clipOver = "main";
    dropAt = slotAt(tAt(e.clientX));
  }
  function dropClips(e: DragEvent) {
    const ids = e.dataTransfer?.getData("application/x-rewind-clip");
    const at = dropAt ?? P.pieces.length;
    dropAt = null;
    clipOver = null;
    if (!ids) return;
    e.preventDefault();
    studio.insertClips(ids.split(",").map((id) => app.clips.find((c) => c.id === id)).filter((c) => !!c), at);
  }
  function layerDragOver(e: DragEvent, row: string) {
    if (!isClip(e)) return;
    e.preventDefault();
    clipOver = row;
    dropAt = null;
  }
  async function layerDrop(e: DragEvent, track: string) {
    const ids = e.dataTransfer?.getData("application/x-rewind-clip");
    clipOver = null;
    if (!ids) return;
    e.preventDefault();
    let at = Math.max(0, tAt(e.clientX));
    for (const id of ids.split(",")) {
      const c = app.clips.find((x) => x.id === id);
      if (!c) continue;
      await studio.addMedia(c.path, { at, track, name: c.title });
      at += c.duration;
    }
  }

  // Molette : Ctrl = zoom autour du pointeur, sinon défilement horizontal
  function wheel(e: WheelEvent) {
    if (!scroller) return;
    if (e.ctrlKey) {
      e.preventDefault();
      const anchor = tAt(e.clientX);
      const before = X(anchor) - scroller.scrollLeft;
      studio.zoom = Math.min(60, Math.max(1, studio.zoom * (e.deltaY < 0 ? 1.18 : 1 / 1.18)));
      requestAnimationFrame(() => scroller && (scroller.scrollLeft = X(anchor) - before));
    } else if (Math.abs(e.deltaY) > Math.abs(e.deltaX) && !e.shiftKey && scroller.scrollHeight <= scroller.clientHeight + 1) {
      // Pas de pistes à faire défiler verticalement : la molette fait défiler la frise de gauche à droite
      // (la barre de défilement est masquée, c'était le seul moyen de revenir au début)
      e.preventDefault();
      scroller.scrollLeft += e.deltaY;
    }
    // Sinon : défilement vertical laissé aux pistes ; Maj + molette = horizontal
  }
  // La tête de lecture reste visible, dans les deux sens : en lecture, après un saut (Début, clic, retour à 0
  // en fin de lecture). Avant, la frise ne suivait que vers la droite : le début restait caché sous les noms
  // de pistes (« Vidéo ») et on ne pouvait plus y revenir. Seul le temps déclenche ce suivi (pas le zoom ni
  // un défilement à la main).
  $effect(() => {
    const at = studio.t;
    untrack(() => {
      if (!scroller) return;
      const x = X(at);
      const left = scroller.scrollLeft + HEAD + 4; // bord gauche visible, à droite des noms de pistes
      const right = scroller.scrollLeft + width - 40;
      if (x < left) scroller.scrollLeft = Math.max(0, x - HEAD - 40);
      else if (x > right) scroller.scrollLeft = x - HEAD - 40;
    });
  });

  const kindIcon = { text: Type, image: ImageIcon, gif: ImageIcon, video: Video, blur: Droplets, fx: Sparkles } as const;
  const audioIcon = { music: Music, sfx: AudioLines, voice: Mic } as const;
  const itemLabel = (l: Layer) =>
    l.kind === "text" ? l.text.replace(/\n/g, " ") || t("Texte")
    : l.kind === "blur" ? t(isFullBlur(l) ? (l.mode === "pixel" ? "Image pixelisée" : "Image floutée") : "Zone floutée")
    : l.kind === "fx" ? t(fxInfo(l.fx).label)
    : l.name;
  const isSel = (id: string) => (studio.sel && studio.sel.type !== "keys" ? studio.sel.id === id : false);
  const kfOf = (it: Layer | AudioItem) => it.kf.map((k) => k.t).filter((x, i, a) => a.indexOf(x) === i);
</script>

<svelte:window onpointermove={onMove} onpointerup={onUp} />

<div class="tl">
  <div class="tools">
    <button class="btn icon small ghost" onclick={() => studio.undo()} disabled={!studio.canUndo} title={t("Annuler (Ctrl+Z)")}><Undo2 size={16} /></button>
    <button class="btn icon small ghost" onclick={() => studio.redo()} disabled={!studio.canRedo} title={t("Rétablir (Ctrl+Y)")}><Redo2 size={16} /></button>
    <span class="sep"></span>
    <button class="btn icon small ghost" onclick={() => studio.trimToPlayhead("start")} disabled={studio.indexAt(studio.t) < 0} title={t("Retirer le début du morceau, jusqu'à la tête de lecture (Q)")}><ArrowLeftToLine size={15} /></button>
    <button class="btn small ghost" onclick={() => studio.split()} title={t("Couper à la tête de lecture (S)")}><Scissors size={15} />{t("Couper")}</button>
    <button class="btn icon small ghost" onclick={() => studio.trimToPlayhead("end")} disabled={studio.indexAt(studio.t) < 0} title={t("Retirer la fin du morceau, après la tête de lecture (W)")}><ArrowRightToLine size={15} /></button>
    <span class="kkwrap" bind:this={kkEl}>
      <button class="btn small ghost" class:on={kkOpen} onclick={() => (kkOpen = !kkOpen)} disabled={!killPieces} aria-expanded={kkOpen} title={t("Ne garder que les kills : marges, blancs entre les kills et transitions réglables")}><Crosshair size={15} />{t("Garder les kills")}<ChevronDown size={13} /></button>
      {#if kkOpen}
        <div class="kkpop" role="dialog" aria-label={t("Garder les kills")}>
          <div class="kkcol">
            <span class="kkh">{t("Autour de chaque kill")}</span>
            <Slider label={t("Avant")} value={kk.before} min={0.5} max={8} step={0.5} def={KEEP_KILLS_DEFAULT.before} format={sec} oninput={(v) => studio.setKeepKills({ before: v })} />
            <Slider label={t("Après")} value={kk.after} min={0.5} max={8} step={0.5} def={KEEP_KILLS_DEFAULT.after} format={sec} oninput={(v) => studio.setKeepKills({ after: v })} />
            <label class="kkrow"><span>{t("Couper les blancs entre deux kills")}</span><span class="switch"><input type="checkbox" checked={kk.cutGaps} onchange={(e) => studio.setKeepKills({ cutGaps: e.currentTarget.checked })} /><span></span></span></label>
            {#if kk.cutGaps}
              <Slider label={t("Écart mini")} value={kk.gap} min={3} max={30} step={1} def={KEEP_KILLS_DEFAULT.gap} format={sec} oninput={(v) => studio.setKeepKills({ gap: v })} />
              <p class="kknote faint">{t("Coupe seulement entre deux kills séparés de plus de {s} ; plus proches, ils restent ensemble.", { s: sec(kk.gap) })}</p>
            {:else}
              <p class="kknote faint">{t("Seuls le début et la fin de chaque clip sont raccourcis.")}</p>
            {/if}
          </div>
          <div class="kkcol" class:off={!kk.cutGaps}>
            <span class="kkh">{t("Transition aux coupes")}</span>
            <div class="kkchips">{#each TRANSITIONS as tr}<button class="kkchip" class:on={kk.transition === tr.id} disabled={!kk.cutGaps} onclick={() => studio.setKeepKills({ transition: tr.id })} title={t(tr.hint)}>{t(tr.label)}</button>{/each}</div>
            {#if kk.transition !== "none"}
              <Slider label={t("Durée")} value={kk.transitionDur} min={0.1} max={1} step={0.1} def={KEEP_KILLS_DEFAULT.transitionDur} format={sec} oninput={(v) => studio.setKeepKills({ transitionDur: v })} />
            {/if}
          </div>
          <div class="kkfoot">
            <p class="kksum">
              {#if kkPlan.plan.size}
                <span class="mono">{fmtTime(kkPlan.before)} → {fmtTime(kkPlan.after)}</span>
                <span class="faint">{kkPlan.gaps ? t(kkPlan.gaps > 1 ? "{n} blancs retirés" : "{n} blanc retiré", { n: kkPlan.gaps }) : t("aucun blanc entre les kills")}</span>
              {:else}
                <span class="faint">{t("Rien à retirer avec ces réglages")}</span>
              {/if}
            </p>
            <button class="btn small ghost" onclick={() => studio.setKeepKills({ ...KEEP_KILLS_DEFAULT })}>{t("Réglages par défaut")}</button>
            <button class="btn small primary" disabled={!kkPlan.plan.size} onclick={() => { studio.trimAroundKills(); kkOpen = false; }}>{t("Appliquer")}</button>
          </div>
        </div>
      {/if}
    </span>
    <button class="btn icon small ghost" onclick={() => studio.duplicate()} disabled={!studio.sel || studio.sel.type === "keys"} title={t("Dupliquer (Ctrl+D)")}><CopyPlus size={15} /></button>
    <button class="btn icon small ghost" onclick={() => studio.removeSel()} disabled={!studio.sel} title={t("Supprimer (Suppr)")}><Trash2 size={15} /></button>
    <button class="btn small ghost" onclick={() => studio.freezeFrame()} disabled={studio.indexAt(studio.t) < 0} title={t("Figer l'image à la tête de lecture pendant 2 s")}><Snowflake size={15} />{t("Figer")}</button>
    <span class="hint faint">{studio.hint}</span>
    <button class="magnet" class:on={studio.follow} onclick={() => studio.setFollow(!studio.follow)} aria-pressed={studio.follow} title={t(studio.follow ? "Textes et effets liés à l'image : un ralenti ou une coupe ne les décale plus (la musique reste fixe)" : "Textes et effets fixes : ils restent à leur seconde quand la vidéo change de durée")}><Link2 size={15} /></button>
    <button class="magnet" class:on={studio.snap} onclick={() => { studio.snap = !studio.snap; studio.say(studio.snap ? "Aimant activé : les éléments se calent sur les bords, la tête de lecture et les kills" : "Aimant désactivé : placement libre"); }} aria-pressed={studio.snap} title={t(studio.snap ? "Aimant activé : les éléments se calent sur les bords, la tête de lecture et les kills" : "Aimant désactivé : placement libre")}><Magnet size={15} /></button>
    <button class="btn icon small ghost" onclick={() => (studio.zoom = Math.max(1, studio.zoom / 1.4))} title={t("Dézoomer (-)")}><ZoomOut size={15} /></button>
    <input class="zoom" type="range" min="1" max="60" step="0.1" bind:value={studio.zoom} aria-label={t("Zoom de la frise")} />
    <button class="btn icon small ghost" onclick={() => (studio.zoom = Math.min(60, studio.zoom * 1.4))} title={t("Zoomer (+)")}><ZoomIn size={15} /></button>
    <button class="btn icon small ghost" onclick={() => { studio.zoom = 1; keepTotal = 0; }} title={t("Tout voir")}><Maximize2 size={15} /></button>
  </div>

  <div class="scroll" bind:this={scroller} bind:clientWidth={width} onscroll={() => (scrollX = scroller?.scrollLeft ?? 0)} onwheel={wheel}>
    <div class="content" style="width:{contentW}px">
      <!-- Règle -->
      <div class="ruler" onpointerdown={scrubDown} role="presentation">
        <div class="headcell"></div>
        {#each ticks as s}
          <span class="tick" style="left:{X(s)}px"><i></i><em class="mono">{fmtTick(s)}</em></span>
        {/each}
        <!-- Nouvelle piste de calques : par-dessus la règle pendant un glissement (rien ne bouge sous le pointeur) -->
        {#if (dragging && ((drag?.kind === "item" && drag.tkind === "layer") || drag?.kind === "piece")) || clipOver}
          <div class="newzone" class:on={over === "new-layer" || clipOver === "new-layer"} data-row="new-layer"
            ondragover={(e) => layerDragOver(e, "new-layer")} ondragleave={() => (clipOver = null)} ondrop={(e) => layerDrop(e, "new")} role="region" aria-label={t("Nouvelle piste")}>
            + {t("Lâche ici : nouvelle piste au-dessus de tout")}
          </div>
        {/if}
      </div>

      <!-- Pistes de calques (la première est affichée par-dessus) -->
      {#each layerTracks as tr, ti (tr.id)}
        <div class="row" style="height:{LAYER_H}px" data-track={tr.id} data-row="layer:{tr.id}" class:dim={tr.kind === "layer" && tr.hidden} class:target={over === `layer:${tr.id}` || clipOver === `layer:${tr.id}`}
          ondragover={(e) => layerDragOver(e, `layer:${tr.id}`)} ondragleave={() => (clipOver = null)} ondrop={(e) => layerDrop(e, tr.id)} role="region" aria-label={tr.name}>
          <div class="head">
            <span class="name" title={tr.name}>{tr.name}</span>
            <button class="ib" onclick={() => studio.moveTrack(tr.id, -1)} disabled={P.tracks.indexOf(tr) === 0} title={t("Monter : afficher par-dessus")}><ChevronUp size={13} /></button>
            <button class="ib" onclick={() => studio.moveTrack(tr.id, 1)} disabled={ti === layerTracks.length - 1} title={t("Descendre : afficher dessous")}><ChevronDown size={13} /></button>
            <button class="ib" onclick={() => studio.toggleTrack(tr.id, "hidden")} title={t(tr.kind === "layer" && tr.hidden ? "Afficher" : "Masquer")}>{#if tr.kind === "layer" && tr.hidden}<EyeOff size={13} />{:else}<Eye size={13} />{/if}</button>
            <button class="ib" onclick={() => studio.toggleTrack(tr.id, "locked")} title={t(tr.locked ? "Déverrouiller" : "Verrouiller")}>{#if tr.locked}<Lock size={13} />{:else}<LockOpen size={13} />{/if}</button>
          </div>
          <div class="lane" onpointerdown={scrubDown} role="presentation">
            {#each tr.items as l (l.uid)}
              {@const Icon = kindIcon[l.kind]}
              <div class="item layer {l.kind}" class:sel={isSel(l.uid)} class:settle={settling} style="left:{X(l.at + itemShift(l, false))}px;width:{Math.max(6, l.dur * pps)}px" onpointerdown={(e) => itemDown(e, l, tr)} role="presentation" title={itemLabel(l)}>
                <span class="grip l" onpointerdown={(e) => trimDown(e, l, "start", tr)} role="presentation"></span>
                {#if l.kind === "image" || l.kind === "gif"}
                  <span class="ithumb" style="background-image:url('{fileUrl(l.path)}')"></span>
                {:else if l.kind === "video" && media.strips[l.path]}
                  {@const s = media.strips[l.path]}
                  <span class="ithumb" style="background-image:url('{fileUrl(s.sprite)}');background-size:{s.frames * 48}px 100%;background-position:{-Math.min(s.frames - 1, Math.floor(l.srcStart / s.interval)) * 48}px 0"></span>
                {:else}
                  <Icon size={12} />
                {/if}
                <span class="lbl" style={l.kind === "text" ? `font-family:'${fontOf(l.font).family}';font-weight:${fontOf(l.font).weight};font-size:13px` : ""}>{itemLabel(l)}</span>
                {#if isSel(l.uid)}{#each kfOf(l) as k}<i class="kf" style="left:{k * pps}px"></i>{/each}{/if}
                <span class="grip r" onpointerdown={(e) => trimDown(e, l, "end", tr)} role="presentation"></span>
              </div>
            {/each}
          </div>
        </div>
      {/each}

      <!-- Piste vidéo principale -->
      <div class="row main" style="height:{MAIN_H}px" data-row="main" class:target={(over === "main" && drag?.kind === "item" && drag.video) || clipOver === "main"} ondragover={dragOver} ondragleave={() => { dropAt = null; clipOver = null; }} ondrop={dropClips} role="region" aria-label={t("Piste vidéo")}>
        <div class="head"><Film size={13} /><span class="name">{t("Vidéo")}</span></div>
        <div class="lane" onpointerdown={scrubDown} role="presentation">
          {#each P.pieces as p, i (p.uid)}
            {@const left = X(L.offsets[i] + pieceShift(i))}
            {@const w = Math.max(8, L.lens[i] * pps)}
            {@const tl = tiles(p, left, w)}
            <div
              class="piece"
              class:sel={isSel(p.uid)}
              class:freeze={p.kind === "freeze"}
              class:missing={media.missing[p.path]}
              class:moving={drag?.kind === "piece" && drag.id === p.uid && drag.started}
              class:settle={settling}
              style="left:{left}px;width:{w}px"
              onpointerdown={(e) => pieceDown(e, p, i)}
              role="presentation"
              title={p.title}
            >
              <div class="strip">{#each tl.list as tile}<i style="left:{tile.left}px;width:{tl.tileW}px;{tl.s ? `background-image:url('${fileUrl(tl.s.sprite)}');background-size:${tl.s.frames * tl.tileW}px 100%;background-position:${tile.pos}px 0` : ''}"></i>{/each}</div>
              {#if p.kind !== "freeze"}<canvas class="wave" use:wave={{ w: media.waves[p.path], start: p.start, end: p.end, width: w, height: 18, v: p.volume }}></canvas>{/if}
              {#if p.kind === "freeze"}
                <!-- Image figée : la vignette de l'image tenue + un flocon ; le nom seulement s'il y a la place -->
                <span class="ptitle frz" class:narrow={w < 110} title={t("Image figée")}><Snowflake size={11} />{#if w >= 110}{t("Image figée")}{/if}</span>
              {:else}
                <span class="ptitle" class:after={i > 0}>{p.title}{#if p.speed !== 1 || isVariable(p)} · {hasCurve(p) ? t("courbe") : p.ramp || p.killRamps?.length ? t("ralenti") : `${p.speed}×`}{/if}</span>
                {#each studio.momentsOf(p) as m}
                  {@const lt = localAt(p, m.time)}
                  {#if lt !== null}<span class="mk {momentTone(m.kind)}" class:precise={m.precise} style="left:{lt * pps}px" title={m.precise ? t("{x} · moment exact où le kill apparaît", { x: t(m.label) }) : t(m.label)}><MomentIcon kind={m.kind} size={13} /></span>{/if}
                {/each}
              {/if}
              {#if media.missing[p.path]}<span class="lost" title={p.path}><FileX size={12} />{t("Fichier introuvable")}</span>{/if}
              {#if isSel(p.uid)}{#each p.zoom as z}<i class="kf" style="left:{z.t * pps}px"></i>{/each}{/if}
              <span class="grip l" onpointerdown={(e) => pieceTrimDown(e, p, "start")} role="presentation"></span>
              <span class="grip r" onpointerdown={(e) => pieceTrimDown(e, p, "end")} role="presentation"></span>
            </div>
            {#if i > 0}
              <button class="junction" class:on={studio.junction === i} class:has={P.pieces[i - 1].transition !== "none"} style="left:{X(L.offsets[i] + L.trans[i] / 2 + pieceShift(i))}px" onpointerdown={(e) => e.stopPropagation()} onclick={() => { studio.sel = null; studio.junction = i; }} title={t("Transition")}><Blend size={11} /></button>
            {/if}
          {/each}
          {#if dropAt !== null}<i class="dropline" style="left:{X(dropAt < P.pieces.length ? L.offsets[dropAt] : L.total)}px"></i>{/if}
          {#if drag?.kind === "piece" && drag.started}<i class="dropline" style="left:{X(drag.to < P.pieces.length ? L.offsets[drag.to] : L.total)}px"></i>{/if}
          {#if !P.pieces.length}<span class="emptylane faint">{t("Glisse des clips ici")}</span>{/if}
        </div>
      </div>

      <!-- Pistes son -->
      {#each audioTracks as tr (tr.id)}
        <div class="row" style="height:{AUDIO_H}px" data-track={tr.id} data-row="audio:{tr.id}" class:dim={tr.kind === "audio" && tr.muted} class:target={over === `audio:${tr.id}`}>
          <div class="head">
            <span class="name" title={tr.name}>{tr.name}</span>
            <button class="ib" onclick={() => studio.toggleTrack(tr.id, "muted")} title={t(tr.kind === "audio" && tr.muted ? "Remettre le son" : "Couper le son")}>{#if tr.kind === "audio" && tr.muted}<VolumeX size={13} />{:else}<Volume2 size={13} />{/if}</button>
            <button class="ib" onclick={() => studio.toggleTrack(tr.id, "locked")} title={t(tr.locked ? "Déverrouiller" : "Verrouiller")}>{#if tr.locked}<Lock size={13} />{:else}<LockOpen size={13} />{/if}</button>
          </div>
          <div class="lane" onpointerdown={scrubDown} role="presentation">
            {#each tr.items as a (a.uid)}
              {@const Icon = audioIcon[(a as AudioItem).kind]}
              {@const au = a as AudioItem}
              <div class="item audio {au.kind}" class:sel={isSel(au.uid)} class:settle={settling} style="left:{X(au.at + itemShift(au, true))}px;width:{Math.max(6, au.dur * pps)}px" onpointerdown={(e) => itemDown(e, au, tr)} role="presentation" title={au.name}>
                <span class="grip l" onpointerdown={(e) => trimDown(e, au, "start", tr)} role="presentation"></span>
                <canvas class="awave" use:wave={{ w: media.waves[au.path], start: au.srcStart, end: au.srcStart + au.dur, width: Math.max(6, au.dur * pps), height: AUDIO_H - 6, v: au.volume }}></canvas>
                <span class="alabel"><Icon size={11} />{au.name}</span>
                {#if isSel(au.uid)}{#each kfOf(au) as k}<i class="kf" style="left:{k * pps}px"></i>{/each}{/if}
                <span class="grip r" onpointerdown={(e) => trimDown(e, au, "end", tr)} role="presentation"></span>
              </div>
            {/each}
          </div>
        </div>
      {/each}
      {#if dragging && drag?.kind === "item" && drag.tkind === "audio"}
        <div class="row newrow" class:on={over === "new-audio"} data-row="new-audio">
          <div class="head"><span class="name">+ {t("Nouvelle piste")}</span></div>
          <div class="lane hintlane">{t("Lâche ici : nouvelle piste son")}</div>
        </div>
      {/if}

      <!-- Aimant : trait à l'endroit où l'élément s'est calé -->
      {#if snapLine !== null && drag}<i class="snapline" style="left:{X(snapLine)}px"></i>{/if}
      <!-- Tête de lecture -->
      <div class="playhead" style="left:{X(studio.t + shiftAt(studio.t))}px"><i></i></div>
      {#if tip}<span class="tip mono" style="left:{tip.x}px">{tip.text}</span>{/if}
    </div>
  </div>
</div>

<style>
  .tl { height: 100%; display: flex; flex-direction: column; border-top: 1px solid var(--line); background: var(--bg-2); min-height: 0; }
  .tools { display: flex; align-items: center; gap: 2px; padding: 6px 10px; border-bottom: 1px solid var(--line); }
  .sep { width: 1px; height: 18px; background: var(--line); margin: 0 6px; }
  .hint { flex: 1; text-align: center; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .zoom { width: 90px; }
  .kkwrap { position: relative; display: inline-flex; }
  .kkwrap > .btn.on { background: color-mix(in srgb, var(--text) 9%, transparent); }
  .kkpop { position: absolute; left: 0; bottom: calc(100% + 8px); z-index: 50; width: 560px; display: grid; grid-template-columns: 1fr 1fr; gap: 12px 20px; padding: 14px 16px; border-radius: 14px; background: var(--panel); border: 1px solid var(--line-2); box-shadow: var(--shadow); animation: kkin 180ms var(--ease); }
  @keyframes kkin { from { opacity: 0; transform: translateY(4px); } }
  @media (prefers-reduced-motion: reduce) { .kkpop { animation: none; } }
  .kkcol { display: flex; flex-direction: column; gap: 6px; min-width: 0; transition: opacity var(--dur) var(--ease); }
  .kkcol.off { opacity: 0.45; }
  .kkh { font-size: 12px; font-weight: 600; color: var(--text); margin-bottom: 2px; }
  .kkrow { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 30px; font-size: 12px; color: var(--text-2); cursor: pointer; }
  .kkchips { display: flex; flex-wrap: wrap; gap: 4px; }
  .kkchip { height: 26px; padding: 0 9px; border-radius: 7px; font-size: 12px; color: var(--text-2); background: var(--panel); border: 1px solid var(--line); transition: color var(--dur) var(--ease), border-color var(--dur) var(--ease); }
  .kkchip:hover:not(:disabled) { color: var(--text); border-color: var(--line-2); }
  .kkchip.on, .kkchip.on:hover { color: var(--hi-ink); background: var(--hi); border-color: var(--hi); }
  .kknote { margin: 0; font-size: 11.5px; line-height: 1.4; }
  .kkfoot { grid-column: 1 / -1; display: flex; align-items: center; gap: 8px; padding-top: 12px; border-top: 1px solid var(--line); }
  .kksum { flex: 1; margin: 0; display: flex; align-items: baseline; gap: 10px; font-size: 12px; }
  .scroll { position: relative; flex: 1; min-height: 0; overflow: auto; }
  .content { position: relative; min-height: 100%; }
  .ruler { position: sticky; top: 0; z-index: 6; height: 24px; background: var(--bg-2); border-bottom: 1px solid var(--line); cursor: pointer; }
  .headcell { position: sticky; left: 0; width: 176px; height: 100%; background: var(--bg-2); z-index: 2; border-right: 1px solid var(--line); }
  .tick { position: absolute; top: 0; height: 100%; }
  .tick i { position: absolute; left: 0; bottom: 0; width: 1px; height: 7px; background: var(--line-2); }
  .tick em { position: absolute; left: 4px; top: 4px; font-style: normal; font-size: 10.5px; color: var(--text-3); white-space: nowrap; }
  .row { position: relative; display: flex; border-bottom: 1px solid var(--line); }
  .row.dim .lane { opacity: 0.45; }
  /* Piste visée pendant un glissement, et zones « nouvelle piste » */
  .row.target { background: color-mix(in srgb, var(--text) 7%, transparent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--text) 30%, transparent); }
  .newzone { position: absolute; left: 180px; right: 4px; top: 2px; bottom: 2px; z-index: 3; display: flex; align-items: center; padding-left: 12px; border-radius: 6px; border: 1px dashed var(--line-2); background: var(--bg-2); font-size: 11.5px; color: var(--text-3); animation: newin 160ms var(--ease); }
  .newzone.on { color: var(--text); border-color: #fff; background: color-mix(in srgb, var(--text) 8%, var(--bg-2)); }
  .newrow { height: 30px; animation: newin 160ms var(--ease); }
  @keyframes newin { from { opacity: 0; } }
  .newrow .lane { display: flex; align-items: center; padding-left: 170px; font-size: 11.5px; color: var(--text-3); border: 1px dashed var(--line-2); border-radius: 6px; margin: 3px 4px; inset: 0 0 0 150px; padding-left: 12px; }
  .newrow.on .lane { color: var(--text); border-color: #fff; background: color-mix(in srgb, var(--text) 8%, transparent); }
  .newrow .head .name { color: var(--text-3); }
  .head { position: sticky; left: 0; z-index: 4; width: 176px; flex: none; display: flex; align-items: center; gap: 2px; padding: 0 6px 0 10px; background: var(--bg-2); border-right: 1px solid var(--line); color: var(--text-3); font-size: 11.5px; }
  .head .name { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--text-2); }
  .main .head { gap: 6px; }
  .ib { width: 20px; height: 20px; border-radius: 5px; display: grid; place-items: center; color: var(--text-3); flex: none; }
  .ib:hover:not(:disabled) { color: var(--text); background: var(--panel-3); }
  .ib:disabled { opacity: 0.3; }
  .lane { position: absolute; inset: 0; cursor: pointer; }
  /* Éléments : neutres ; la sélection est un liseré blanc (la couleur est réservée à ce qui signale) */
  /* Éléments : une teinte discrète par type (texte, image, vidéo, flou, son) + un filet de couleur à gauche,
     pour reconnaître le type d'un coup d'œil ; la sélection est un liseré blanc. */
  .item { --tc: #9aa3b5; position: absolute; top: 4px; bottom: 4px; display: flex; align-items: center; gap: 6px; padding: 0 8px 0 10px; border-radius: 6px; background: color-mix(in srgb, var(--tc) 16%, var(--panel-2)); border: 1px solid color-mix(in srgb, var(--tc) 28%, transparent); font-size: 11.5px; color: var(--text); overflow: hidden; cursor: grab; white-space: nowrap; transition: filter var(--dur) var(--ease); }
  .item::before { content: ""; position: absolute; left: 0; top: 0; bottom: 0; width: 3px; background: var(--tc); }
  .item:hover { filter: brightness(1.12); }
  .item:active { cursor: grabbing; }
  .item.text { --tc: #a78bfa; }
  .item.image, .item.gif { --tc: #38bdf8; }
  .item.video { --tc: #3ddc97; }
  .item.blur { --tc: #9aa3b5; background: repeating-linear-gradient(45deg, var(--panel-3) 0 6px, var(--panel-2) 6px 12px); }
  .item.audio.music { --tc: #ffd25a; }
  .item.audio.sfx { --tc: #ff8a3d; }
  .item.audio.voice { --tc: #f472b6; }
  .ithumb { width: 48px; height: 100%; margin: 0 2px 0 -6px; flex: none; background-size: cover; background-position: center; border-radius: 3px; }
  .item.sel, .piece.sel { outline: 2px solid #fff; outline-offset: -1px; z-index: 3; }
  .lbl { overflow: hidden; text-overflow: ellipsis; }
  .item.audio { padding: 0; }
  .awave { position: absolute; left: 0; top: 2px; height: calc(100% - 4px); opacity: 0.7; }
  .alabel { position: relative; display: flex; align-items: center; gap: 4px; padding: 0 8px; font-size: 11px; color: var(--text-2); text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8); overflow: hidden; text-overflow: ellipsis; }
  /* Poignées de rognage : zone de 10 px ; trait visible au survol, plein quand l'élément est choisi */
  .grip { position: absolute; top: 0; bottom: 0; width: 10px; cursor: ew-resize; z-index: 2; }
  .grip::after { content: ""; position: absolute; top: 0; bottom: 0; width: 6px; background: #fff; opacity: 0; transition: opacity var(--dur) var(--ease); }
  .grip.l { left: 0; }
  .grip.l::after { left: 0; }
  .grip.r { right: 0; }
  .grip.r::after { right: 0; }
  .item:hover .grip::after, .piece:hover .grip::after { opacity: 0.35; }
  .grip:hover::after { opacity: 0.7; }
  .item.sel .grip::after, .piece.sel .grip::after { opacity: 0.85; }
  .kf { position: absolute; top: 50%; width: 8px; height: 8px; margin: -4px 0 0 -4px; transform: rotate(45deg); background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6); pointer-events: none; z-index: 2; }
  .piece { position: absolute; top: 3px; bottom: 3px; border-radius: 7px; overflow: hidden; background: #0d0e12; border: 1px solid var(--line-2); cursor: grab; }
  .piece.moving { opacity: 0.5; }
  .piece.freeze { background: color-mix(in srgb, #9ad 18%, #0d0e12); }
  .strip { position: absolute; left: 0; right: 0; top: 0; height: 62px; overflow: hidden; }
  .strip i { position: absolute; top: 0; height: 100%; background-repeat: no-repeat; background-color: #15171d; }
  .wave { position: absolute; left: 0; bottom: 1px; height: 18px; }
  .ptitle { position: absolute; left: 6px; top: 4px; display: flex; align-items: center; gap: 4px; max-width: calc(100% - 12px); padding: 1px 6px; border-radius: 5px; background: rgba(0, 0, 0, 0.72); font-size: 11px; color: #fff; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  /* Après une jonction, le titre laisse la place au bouton de transition */
  .ptitle.after { left: 14px; max-width: calc(100% - 22px); }
  .ptitle.frz { left: 50%; translate: -50% 0; max-width: calc(100% - 26px); }
  .ptitle.frz.narrow { padding: 2px 4px; }
  .piece.freeze .strip { opacity: 0.75; }
  /* Fichier déplacé ou supprimé : seul endroit rouge de la frise (c'est une erreur) */
  .piece.missing { border-color: color-mix(in srgb, var(--rec) 60%, transparent); background: repeating-linear-gradient(45deg, #1a0d0d 0 8px, #120909 8px 16px); }
  .lost { position: absolute; left: 8px; bottom: 6px; z-index: 3; display: flex; align-items: center; gap: 5px; padding: 2px 7px; border-radius: 5px; font-size: 11px; font-weight: 500; color: #fff; background: color-mix(in srgb, var(--rec) 80%, #000); }
  /* Repères de kills : picto dans une pastille sombre (lisible sur n'importe quelle image) + trait à l'instant exact */
  .mk { position: absolute; top: 34px; z-index: 2; translate: -50% 0; width: 20px; height: 20px; border-radius: 6px; display: grid; place-items: center; color: #f2f3f5; background: rgba(8, 9, 12, 0.88); box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.16), 0 2px 6px rgba(0, 0, 0, 0.6); pointer-events: none; }
  .mk::after { content: ""; position: absolute; left: 50%; top: 100%; width: 1px; height: 12px; margin-left: -0.5px; background: rgba(255, 255, 255, 0.55); }
  /* Recalé sur le kill feed : trait plein jusqu'en bas du morceau (moment exact) */
  .mk.precise::after { height: 30px; background: rgba(255, 255, 255, 0.85); }
  .mk.gold { color: #ffd25a; }
  .mk.green { color: #3ddc97; }
  .mk.dim { color: #9aa3b5; }
  /* Aimant : état visible d'un coup d'œil (fond plein quand il est actif) */
  .magnet { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; color: var(--text-3); border: 1px solid transparent; transition: color var(--dur) var(--ease), background var(--dur) var(--ease), border-color var(--dur) var(--ease); }
  .magnet:hover { color: var(--text); background: var(--panel-2); }
  .magnet.on { color: var(--hi-ink); background: var(--hi); }
  .snapline { position: absolute; top: 24px; bottom: 0; width: 1px; margin-left: -0.5px; background: var(--accent); z-index: 7; pointer-events: none; }
  .item.fx { --tc: #b9bec9; background: color-mix(in srgb, var(--tc) 10%, var(--panel-2)); background-image: radial-gradient(color-mix(in srgb, var(--tc) 30%, transparent) 1px, transparent 1.2px); background-size: 7px 7px; }
  /* En bas de la jonction : au milieu, il recouvrait les poignées de rognage des deux morceaux */
  .junction { position: absolute; bottom: 4px; z-index: 4; width: 18px; height: 18px; margin: 0 0 0 -9px; border-radius: 6px; display: grid; place-items: center; background: var(--panel-3); border: 1px solid var(--line-2); color: var(--text-3); }
  .junction.has { color: var(--text); background: var(--panel-2); }
  .junction.on { outline: 2px solid #fff; color: var(--text); }
  .dropline { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: var(--hi); z-index: 5; }
  .emptylane { position: absolute; left: 196px; top: 50%; transform: translateY(-50%); font-size: 12px; }
  .playhead { position: absolute; top: 0; bottom: 0; width: 0; z-index: 7; pointer-events: none; }
  .playhead i { position: absolute; top: 0; bottom: 0; left: -1px; width: 2px; background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.4); }
  .playhead i::before { content: ""; position: absolute; top: 0; left: -5px; width: 12px; height: 10px; background: #fff; clip-path: polygon(0 0, 100% 0, 50% 100%); }
  .tip { position: absolute; top: 26px; z-index: 8; transform: translateX(-50%); padding: 2px 6px; border-radius: 5px; background: #fff; color: #0b0c0f; font-size: 11px; pointer-events: none; }
  /* Au lâcher d'un rognage, le montage se recolle en douceur (pas pendant le geste) */
  .piece.settle, .item.settle { transition: left 180ms var(--ease); }
  @media (prefers-reduced-motion: reduce) { .piece.settle, .item.settle { transition: none; } }
</style>
