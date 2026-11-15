<script lang="ts">
  // Frise multi-pistes : calques en haut (la piste du haut s'affiche par-dessus), vidéo principale, son en bas.
  import {
    Scissors, Trash2, CopyPlus, Undo2, Redo2, Magnet, ZoomIn, ZoomOut, Maximize2, Snowflake, Eye, EyeOff, Lock, LockOpen,
    ChevronUp, ChevronDown, Volume2, VolumeX, Type, Image as ImageIcon, Film, Music, Mic, Droplets, Video, Blend, AudioLines, Sparkles, FileX,
  } from "@lucide/svelte";
  import { untrack } from "svelte";
  import { studio, media, fmtTime } from "$lib/studio/store.svelte";
  import { outLen, localAt, fxInfo, isFullBlur, type Layer, type AudioItem, type Piece, type Track } from "$lib/studio/model";
  import { momentTone } from "$lib/moments";
  import { fontOf } from "$lib/studio/fonts";
  import MomentIcon from "../MomentIcon.svelte";
  import { fileUrl, type Waveform } from "$lib/api";
  import { app } from "$lib/state.svelte";
  import { t } from "$lib/i18n.svelte";

  const HEAD = 176; // colonne des noms de pistes
  const PAD = 16;
  const MAIN_H = 62;
  const LAYER_H = 34;
  const AUDIO_H = 40;
  const SNAP_PX = 8;

  let scroller: HTMLDivElement | undefined = $state();
  let width = $state(900);
  let scrollX = $state(0);

  const P = $derived(studio.project);
  const L = $derived(studio.lay);
  const total = $derived(Math.max(studio.total, 4));
  const fitPps = $derived(Math.max(2, (width - HEAD - PAD * 2 - 40) / total));
  const pps = $derived(fitPps * studio.zoom);
  const contentW = $derived(HEAD + PAD * 2 + total * pps + 200);
  const X = (s: number) => HEAD + PAD + s * pps;
  function tAt(clientX: number) {
    const r = scroller!.getBoundingClientRect();
    return (clientX - r.left + scroller!.scrollLeft - HEAD - PAD) / pps;
  }

  const layerTracks = $derived(P.tracks.filter((tr) => tr.kind === "layer"));
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
    const n = Math.ceil(w / tileW);
    const j0 = Math.max(0, Math.floor((scrollX - left) / tileW) - 1);
    const j1 = Math.min(n, Math.ceil((scrollX + width - left) / tileW) + 1);
    const list: { left: number; pos: number }[] = [];
    for (let j = j0; j < j1; j++) {
      const src = p.kind === "freeze" ? p.start : p.start + (((j + 0.5) * tileW) / w) * (p.end - p.start);
      const fi = s ? Math.min(s.frames - 1, Math.max(0, Math.floor(src / s.interval))) : 0;
      list.push({ left: j * tileW, pos: -fi * tileW });
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
  function snapPoints(exclude?: string, withHead = true): number[] {
    const pts = withHead ? [studio.t, 0] : [0];
    L.offsets.forEach((o, i) => {
      pts.push(o, o + L.lens[i]);
      // Les kills aussi : un texte ou un son se cale pile sur le moment
      for (const m of P.pieces[i].markers) {
        const lt = localAt(P.pieces[i], m.time);
        if (lt !== null) pts.push(o + lt);
      }
    });
    for (const tr of P.tracks) for (const it of tr.items as { uid: string; at: number; dur: number }[]) if (it.uid !== exclude) pts.push(it.at, it.at + it.dur);
    return pts;
  }
  function snap(v: number, exclude?: string, withHead = true): number {
    if (!studio.snap) {
      snapLine = null;
      return v;
    }
    let best = v, d = SNAP_PX / pps, hit = false;
    for (const p of snapPoints(exclude, withHead)) if (Math.abs(p - v) < d) { d = Math.abs(p - v); best = p; hit = true; }
    snapLine = hit ? best : null;
    return best;
  }

  // ---------- glisser ----------
  type Drag =
    | { kind: "scrub" }
    | { kind: "item"; id: string; track: string; tkind: "layer" | "audio"; video: boolean; x0: number; y0: number; at0: number; started: boolean }
    | { kind: "trimItem"; id: string; side: "start" | "end"; x0: number; at0: number; dur0: number; src0: number; max: number }
    | { kind: "trimPiece"; id: string; side: "start" | "end"; x0: number; v0: number; speed: number; freeze: boolean }
    | { kind: "piece"; id: string; from: number; x0: number; y0: number; started: boolean; to: number };
  let drag = $state<Drag | null>(null);
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
    studio.seek(snap(tAt(e.clientX), undefined, false));
  }
  function itemDown(e: PointerEvent, it: Layer | AudioItem, tr: Track) {
    if (e.button !== 0) return;
    e.stopPropagation();
    studio.select({ type: tr.kind === "audio" ? "audio" : "layer", id: it.uid });
    if (tr.locked) return;
    drag = { kind: "item", id: it.uid, track: tr.id, tkind: tr.kind, video: it.kind === "video", x0: e.clientX, y0: e.clientY, at0: it.at, started: false };
  }
  function trimDown(e: PointerEvent, it: Layer | AudioItem, side: "start" | "end", tr: Track) {
    e.stopPropagation();
    if (tr.locked) return;
    studio.select({ type: tr.kind === "audio" ? "audio" : "layer", id: it.uid });
    studio.snapshot();
    const src0 = "srcStart" in it ? it.srcStart : 0;
    const speed = it.kind === "video" ? (it as any).speed : 1;
    const max = "srcDur" in it ? ((it as any).srcDur - src0) / speed : Infinity;
    drag = { kind: "trimItem", id: it.uid, side, x0: e.clientX, at0: it.at, dur0: it.dur, src0, max };
  }
  function pieceDown(e: PointerEvent, p: Piece, i: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    studio.select({ type: "piece", id: p.uid });
    drag = { kind: "piece", id: p.uid, from: i, x0: e.clientX, y0: e.clientY, started: false, to: i };
  }
  function pieceTrimDown(e: PointerEvent, p: Piece, side: "start" | "end") {
    e.stopPropagation();
    studio.select({ type: "piece", id: p.uid });
    studio.snapshot();
    drag = { kind: "trimPiece", id: p.uid, side, x0: e.clientX, v0: p.kind === "freeze" ? p.hold : side === "start" ? p.start : p.end, speed: p.speed, freeze: p.kind === "freeze" };
  }
  function onMove(e: PointerEvent) {
    const d = drag;
    if (!d) return;
    if (d.kind === "scrub") return studio.seek(snap(tAt(e.clientX), undefined, false));
    const dt = (e.clientX - ("x0" in d ? d.x0 : 0)) / pps;
    if (d.kind === "item") {
      if (!d.started && Math.abs(e.clientX - d.x0) + Math.abs(e.clientY - d.y0) < 4) return;
      if (!d.started) { studio.snapshot(); d.started = true; }
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
        const end = snap(d.at0 + d.dur0 + dt, d.id);
        const dur = Math.max(0.1, Math.min(d.max, end - d.at0));
        patchItem(d.id, { dur });
        tip = { x: X(d.at0 + dur), text: fmtTime(dur, true) };
      } else {
        let at = snap(d.at0 + dt, d.id);
        at = Math.max(0, Math.min(d.at0 + d.dur0 - 0.1, at));
        const shift = at - d.at0;
        if ("srcStart" in it && d.src0 + shift < 0) at = d.at0 - d.src0;
        const s2 = at - d.at0;
        const patch: Record<string, unknown> = { at, dur: d.dur0 - s2 };
        if ("srcStart" in it) patch.srcStart = d.src0 + s2 * (it.kind === "video" ? (it as any).speed : 1);
        patchItem(d.id, patch);
        tip = { x: X(at), text: fmtTime(d.dur0 - s2, true) };
      }
    } else if (d.kind === "trimPiece") {
      const p = P.pieces.find((x) => x.uid === d.id);
      if (!p) return;
      const i = P.pieces.indexOf(p);
      const off = L.offsets[i] ?? 0;
      if (d.freeze) {
        let hold = Math.max(0.2, Math.min(10, d.v0 + (d.side === "end" ? dt : -dt)));
        // Bord droit aimanté (tête de lecture, autres éléments, kills)
        if (d.side === "end") hold = Math.max(0.2, Math.min(10, snap(off + hold, p.uid) - off));
        studio.updatePiece(d.id, { hold }, false);
        tip = { x: X(off + hold), text: fmtTime(hold, true) };
        return;
      }
      const src = dt * d.speed;
      if (d.side === "start") {
        const start = Math.max(0, Math.min(p.end - 0.2, d.v0 + src));
        studio.updatePiece(d.id, { start }, false);
        tip = { x: X(off), text: fmtTime(outLen({ ...p, start }), true) };
      } else {
        let end = Math.min(p.srcDur, Math.max(p.start + 0.2, d.v0 + src));
        if (!p.ramp) {
          const edge = snap(off + (end - p.start) / d.speed, p.uid);
          end = Math.min(p.srcDur, Math.max(p.start + 0.2, p.start + (edge - off) * d.speed));
        }
        studio.updatePiece(d.id, { end }, false);
        tip = { x: X(off + outLen({ ...p, end })), text: fmtTime(outLen({ ...p, end }), true) };
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
    <button class="btn small ghost" onclick={() => studio.split()} title={t("Couper à la tête de lecture (S)")}><Scissors size={15} />{t("Couper")}</button>
    <button class="btn icon small ghost" onclick={() => studio.duplicate()} disabled={!studio.sel || studio.sel.type === "keys"} title={t("Dupliquer (Ctrl+D)")}><CopyPlus size={15} /></button>
    <button class="btn icon small ghost" onclick={() => studio.removeSel()} disabled={!studio.sel} title={t("Supprimer (Suppr)")}><Trash2 size={15} /></button>
    <button class="btn small ghost" onclick={() => studio.freezeFrame()} disabled={studio.indexAt(studio.t) < 0} title={t("Figer l'image à la tête de lecture pendant 2 s")}><Snowflake size={15} />{t("Figer")}</button>
    <span class="hint faint">{studio.hint}</span>
    <button class="magnet" class:on={studio.snap} onclick={() => { studio.snap = !studio.snap; studio.say(studio.snap ? "Aimant activé : les éléments se calent sur les bords, la tête de lecture et les kills" : "Aimant désactivé : placement libre"); }} aria-pressed={studio.snap} title={t(studio.snap ? "Aimant activé : les éléments se calent sur les bords, la tête de lecture et les kills" : "Aimant désactivé : placement libre")}><Magnet size={15} /></button>
    <button class="btn icon small ghost" onclick={() => (studio.zoom = Math.max(1, studio.zoom / 1.4))} title={t("Dézoomer (-)")}><ZoomOut size={15} /></button>
    <input class="zoom" type="range" min="1" max="60" step="0.1" bind:value={studio.zoom} aria-label={t("Zoom de la frise")} />
    <button class="btn icon small ghost" onclick={() => (studio.zoom = Math.min(60, studio.zoom * 1.4))} title={t("Zoomer (+)")}><ZoomIn size={15} /></button>
    <button class="btn icon small ghost" onclick={() => (studio.zoom = 1)} title={t("Tout voir")}><Maximize2 size={15} /></button>
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
              <div class="item layer {l.kind}" class:sel={isSel(l.uid)} style="left:{X(l.at)}px;width:{Math.max(6, l.dur * pps)}px" onpointerdown={(e) => itemDown(e, l, tr)} role="presentation" title={itemLabel(l)}>
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
            {@const left = X(L.offsets[i])}
            {@const w = Math.max(8, L.lens[i] * pps)}
            {@const tl = tiles(p, left, w)}
            <div
              class="piece"
              class:sel={isSel(p.uid)}
              class:freeze={p.kind === "freeze"}
              class:missing={media.missing[p.path]}
              class:moving={drag?.kind === "piece" && drag.id === p.uid && drag.started}
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
                <span class="ptitle" class:after={i > 0}>{p.title}{#if p.speed !== 1 || p.ramp} · {p.ramp ? t("ralenti") : `${p.speed}×`}{/if}</span>
                {#each p.markers as m}
                  {@const lt = localAt(p, m.time)}
                  {#if lt !== null}<span class="mk {momentTone(m.kind)}" style="left:{lt * pps}px" title={m.label}><MomentIcon kind={m.kind} size={13} /></span>{/if}
                {/each}
              {/if}
              {#if media.missing[p.path]}<span class="lost" title={p.path}><FileX size={12} />{t("Fichier introuvable")}</span>{/if}
              {#if isSel(p.uid)}{#each p.zoom as z}<i class="kf" style="left:{z.t * pps}px"></i>{/each}{/if}
              <span class="grip l" onpointerdown={(e) => pieceTrimDown(e, p, "start")} role="presentation"></span>
              <span class="grip r" onpointerdown={(e) => pieceTrimDown(e, p, "end")} role="presentation"></span>
            </div>
            {#if i > 0}
              <button class="junction" class:on={studio.junction === i} class:has={P.pieces[i - 1].transition !== "none"} style="left:{X(L.offsets[i] + L.trans[i] / 2)}px" onpointerdown={(e) => e.stopPropagation()} onclick={() => { studio.sel = null; studio.junction = i; }} title={t("Transition")}><Blend size={11} /></button>
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
              <div class="item audio {au.kind}" class:sel={isSel(au.uid)} style="left:{X(au.at)}px;width:{Math.max(6, au.dur * pps)}px" onpointerdown={(e) => itemDown(e, au, tr)} role="presentation" title={au.name}>
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
      <div class="playhead" style="left:{X(studio.t)}px"><i></i></div>
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
  .grip { position: absolute; top: 0; bottom: 0; width: 7px; cursor: ew-resize; z-index: 2; }
  .grip.l { left: 0; }
  .grip.r { right: 0; }
  .item.sel .grip, .piece.sel .grip { background: rgba(255, 255, 255, 0.85); }
  .kf { position: absolute; top: 50%; width: 8px; height: 8px; margin: -4px 0 0 -4px; transform: rotate(45deg); background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6); pointer-events: none; z-index: 2; }
  .piece { position: absolute; top: 3px; bottom: 3px; border-radius: 7px; overflow: hidden; background: #0d0e12; border: 1px solid var(--line-2); cursor: grab; }
  .piece.moving { opacity: 0.5; }
  .piece.freeze { background: color-mix(in srgb, #9ad 18%, #0d0e12); }
  .strip { position: absolute; left: 0; right: 0; top: 0; height: 40px; overflow: hidden; }
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
  .mk { position: absolute; top: 22px; z-index: 2; translate: -50% 0; width: 20px; height: 20px; border-radius: 6px; display: grid; place-items: center; color: #f2f3f5; background: rgba(8, 9, 12, 0.88); box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.16), 0 2px 6px rgba(0, 0, 0, 0.6); pointer-events: none; }
  .mk::after { content: ""; position: absolute; left: 50%; top: 100%; width: 1px; height: 12px; margin-left: -0.5px; background: rgba(255, 255, 255, 0.55); }
  .mk.gold { color: #ffd25a; }
  .mk.green { color: #3ddc97; }
  .mk.dim { color: #9aa3b5; }
  /* Aimant : état visible d'un coup d'œil (fond plein quand il est actif) */
  .magnet { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; color: var(--text-3); border: 1px solid transparent; transition: color var(--dur) var(--ease), background var(--dur) var(--ease), border-color var(--dur) var(--ease); }
  .magnet:hover { color: var(--text); background: var(--panel-2); }
  .magnet.on { color: var(--hi-ink); background: var(--hi); }
  .snapline { position: absolute; top: 24px; bottom: 0; width: 1px; margin-left: -0.5px; background: var(--accent); z-index: 7; pointer-events: none; }
  .item.fx { --tc: #b9bec9; background: color-mix(in srgb, var(--tc) 10%, var(--panel-2)); background-image: radial-gradient(color-mix(in srgb, var(--tc) 30%, transparent) 1px, transparent 1.2px); background-size: 7px 7px; }
  .junction { position: absolute; top: 50%; z-index: 4; width: 20px; height: 20px; margin: -10px 0 0 -10px; border-radius: 6px; display: grid; place-items: center; background: var(--panel-3); border: 1px solid var(--line-2); color: var(--text-3); }
  .junction.has { color: var(--text); background: var(--panel-2); }
  .junction.on { outline: 2px solid #fff; color: var(--text); }
  .dropline { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: var(--hi); z-index: 5; }
  .emptylane { position: absolute; left: 196px; top: 50%; transform: translateY(-50%); font-size: 12px; }
  .playhead { position: absolute; top: 0; bottom: 0; width: 0; z-index: 7; pointer-events: none; }
  .playhead i { position: absolute; top: 0; bottom: 0; left: -1px; width: 2px; background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.4); }
  .playhead i::before { content: ""; position: absolute; top: 0; left: -5px; width: 12px; height: 10px; background: #fff; clip-path: polygon(0 0, 100% 0, 50% 100%); }
  .tip { position: absolute; top: 26px; z-index: 8; transform: translateX(-50%); padding: 2px 6px; border-radius: 5px; background: #fff; color: #0b0c0f; font-size: 11px; pointer-events: none; }
</style>
