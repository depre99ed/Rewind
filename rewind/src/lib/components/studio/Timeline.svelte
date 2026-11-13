<script lang="ts">
  import {
    Scissors, Trash2, CopyPlus, Undo2, Redo2, Magnet, ZoomIn, ZoomOut, Maximize2, Music, Plus, Blend, Gauge,
    ChevronsLeft, ChevronsRight, Film, VolumeX,
  } from "@lucide/svelte";
  import { app, type StudioPiece, type Transition } from "$lib/state.svelte";
  import { studio, media, TRANSITIONS, SPEEDS, fmtTime, clampSpeed } from "$lib/studio.svelte";
  import { fileUrl, gameTone, KIND_LABEL, type Waveform } from "$lib/api";
  import { t } from "$lib/i18n.svelte";

  let { onAddMusic }: { onAddMusic: () => void } = $props();

  const PAD = 20; // marge à gauche et à droite de la frise
  const STRIP_H = 62; // hauteur des vignettes
  const WAVE_H = 24; // hauteur de la forme d'onde sous les vignettes
  const SNAP_PX = 9;

  let scroller: HTMLDivElement | undefined = $state();
  let width = $state(900);
  let scrollX = $state(0);
  let hoverT = $state<number | null>(null);

  const L = $derived(studio.lay);
  const fitPps = $derived(Math.max(2, (width - PAD * 2 - 40) / Math.max(L.total, 4)));
  const pps = $derived(fitPps * studio.zoom);
  const contentW = $derived(Math.max(width, L.total * pps + PAD * 2 + 160));
  const X = (t: number) => PAD + t * pps;
  function tAt(clientX: number) {
    const r = scroller!.getBoundingClientRect();
    return (clientX - r.left + scroller!.scrollLeft - PAD) / pps;
  }

  // Graduations : un repère lisible tous les ~90 px, petites marques entre deux
  const step = $derived([0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300].find((s) => s * pps >= 90) ?? 300);
  const ticks = $derived.by(() => {
    const out: number[] = [];
    const first = Math.max(0, Math.floor((scrollX - PAD) / pps / step) * step);
    const last = (scrollX + width) / pps + step;
    for (let t = first; t <= last; t += step) out.push(+t.toFixed(3));
    return out;
  });
  const fmtTick = (t: number) => (step < 1 ? fmtTime(t, true).replace(/0$/, "") : fmtTime(t));

  // ---------- vignettes (seules celles visibles sont dessinées) ----------
  function tiles(p: StudioPiece, left: number, w: number) {
    const s = media.strips[p.path];
    const tileW = s ? s.frame_w * (STRIP_H / s.frame_h) : (STRIP_H * 16) / 9;
    const n = Math.ceil(w / tileW);
    const j0 = Math.max(0, Math.floor((scrollX - left) / tileW) - 1);
    const j1 = Math.min(n, Math.ceil((scrollX + width - left) / tileW) + 1);
    const list: { left: number; pos: number }[] = [];
    for (let j = j0; j < j1; j++) {
      const src = p.start + (((j + 0.5) * tileW) / w) * (p.end - p.start);
      const fi = s ? Math.min(s.frames - 1, Math.max(0, Math.floor(src / s.interval))) : 0;
      list.push({ left: j * tileW, pos: -fi * tileW });
    }
    return { tileW, list, s };
  }

  // ---------- forme d'onde ----------
  type WaveParams = { w: Waveform | undefined; start: number; end: number; width: number; loop?: number; color: string; v: number };
  function wave(canvas: HTMLCanvasElement, params: WaveParams) {
    const draw = (q: WaveParams) => {
      const w = q.w;
      const cssW = Math.max(1, Math.min(q.width, 16000));
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      canvas.width = Math.round(cssW * dpr);
      canvas.height = Math.round(WAVE_H * dpr);
      canvas.style.width = `${q.width}px`;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      if (!w || !w.peaks.length) {
        ctx.fillStyle = q.color;
        ctx.globalAlpha = 0.25;
        ctx.fillRect(0, canvas.height / 2 - dpr * 0.5, canvas.width, dpr);
        return;
      }
      const bar = 2 * dpr;
      const gap = 1 * dpr;
      const span = q.end - q.start;
      const gain = Math.min(1.6, Math.max(0, q.v));
      ctx.fillStyle = q.color;
      ctx.globalAlpha = q.v <= 0.001 ? 0.25 : 0.9;
      for (let xpx = 0; xpx < canvas.width; xpx += bar + gap) {
        const a = q.start + (xpx / canvas.width) * span;
        const b = q.start + ((xpx + bar + gap) / canvas.width) * span;
        let peak = 0;
        for (let s = a; s < b; s += 1 / w.rate) {
          const src = q.loop && q.loop > 0 ? s % q.loop : s;
          const v = w.peaks[Math.floor(src * w.rate)] ?? 0;
          if (v > peak) peak = v;
        }
        const h = Math.max(dpr, (peak / 255) * gain * (canvas.height - 2 * dpr));
        ctx.fillRect(xpx, (canvas.height - h) / 2, bar, h);
      }
    };
    draw(params);
    return { update: draw };
  }

  // ---------- glisser : lecture, rognage, déplacement, musique ----------
  type Drag =
    | { kind: "scrub" }
    | { kind: "trim"; uid: string; i: number; side: "start" | "end"; x0: number; v0: number }
    | { kind: "move"; uid: string; from: number; x0: number; dx: number; started: boolean; to: number }
    | { kind: "music"; x0: number; at0: number; started: boolean };
  let drag = $state<Drag | null>(null);
  let trimTip = $state<{ x: number; text: string } | null>(null);
  let snapX = $state<number | null>(null);
  let dropAt = $state<number | null>(null);

  function scrubDown(e: PointerEvent) {
    if (e.button !== 0) return;
    studio.playing = false;
    drag = { kind: "scrub" };
    studio.seek(tAt(e.clientX));
  }

  function pieceDown(e: PointerEvent, p: StudioPiece, i: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    studio.select(p.uid);
    drag = { kind: "move", uid: p.uid, from: i, x0: e.clientX, dx: 0, started: false, to: i };
  }

  function handleDown(e: PointerEvent, p: StudioPiece, i: number, side: "start" | "end") {
    if (e.button !== 0) return;
    e.stopPropagation();
    studio.select(p.uid);
    studio.playing = false;
    studio.snapshot();
    drag = { kind: "trim", uid: p.uid, i, side, x0: e.clientX, v0: side === "start" ? p.start : p.end };
  }

  function musicDown(e: PointerEvent) {
    if (e.button !== 0 || !app.studioMusic) return;
    e.stopPropagation();
    studio.selected = null;
    studio.musicSelected = true;
    drag = { kind: "music", x0: e.clientX, at0: app.studioMusic.at, started: false };
  }

  /** Index d'insertion pour une position de la frise (entre deux morceaux). */
  function insertionIndex(t: number, skip = -1) {
    let idx = 0;
    app.studio.forEach((_, k) => {
      if (k === skip) return;
      if (L.offsets[k] + L.lens[k] / 2 < t) idx++;
    });
    return idx;
  }

  function onMove(e: PointerEvent) {
    if (!drag) {
      return;
    }
    if (drag.kind === "scrub") {
      studio.seek(tAt(e.clientX));
      return;
    }
    if (drag.kind === "move") {
      drag.dx = e.clientX - drag.x0;
      if (!drag.started && Math.abs(drag.dx) > 6) drag.started = true;
      if (drag.started) drag.to = insertionIndex(tAt(e.clientX), drag.from);
      return;
    }
    if (drag.kind === "music") {
      const m = app.studioMusic;
      if (!m) return;
      const dx = e.clientX - drag.x0;
      if (!drag.started && Math.abs(dx) > 4) {
        drag.started = true;
        studio.snapshot();
      }
      if (!drag.started) return;
      let at = Math.max(0, drag.at0 + dx / pps);
      snapX = null;
      if (studio.snap) {
        for (const c of [0, ...L.offsets, studio.t]) {
          if (Math.abs(c - at) * pps < SNAP_PX) {
            at = c;
            snapX = X(c);
            break;
          }
        }
      }
      studio.updateMusic({ at: Math.min(at, Math.max(0, L.total - 0.5)) }, false);
      trimTip = { x: X(at), text: `${t("Départ")} ${fmtTime(at, true)}` };
      return;
    }
    // Rognage
    const d = drag;
    const p = app.studio.find((x) => x.uid === d.uid);
    if (!p) return;
    const speed = clampSpeed(p.speed);
    let v = d.v0 + ((e.clientX - d.x0) / pps) * speed;
    const maxEnd = media.info[p.path]?.duration || p.srcDur || p.end;
    v = d.side === "start" ? Math.max(0, Math.min(v, p.end - 0.1)) : Math.min(maxEnd, Math.max(v, p.start + 0.1));
    // Aimant : repères du clip, tête de lecture, bords du clip
    snapX = null;
    if (studio.snap) {
      const cands = [0, maxEnd, ...p.markers.map((m) => m.time)];
      const i = app.studio.findIndex((x) => x.uid === p.uid);
      const playSrc = p.start + (studio.t - L.offsets[i]) * speed;
      if (playSrc > 0 && playSrc < maxEnd) cands.push(playSrc);
      for (const c of cands) {
        if ((Math.abs(c - v) / speed) * pps < SNAP_PX) {
          v = c;
          break;
        }
      }
      if (cands.some((c) => Math.abs(c - v) < 1e-6)) snapX = e.clientX - scroller!.getBoundingClientRect().left + scroller!.scrollLeft;
    }
    const patch = d.side === "start" ? { start: v } : { end: v };
    studio.update(p.uid, patch, false);
    const np = { ...p, ...patch };
    const len = (np.end - np.start) / speed;
    trimTip = {
      x: e.clientX - scroller!.getBoundingClientRect().left + scroller!.scrollLeft,
      text: `${d.side === "start" ? t("Début") : t("Fin")} ${fmtTime(v, true)} · ${len.toFixed(1)} s`,
    };
  }

  function onUp(e: PointerEvent) {
    const d = drag;
    drag = null;
    trimTip = null;
    snapX = null;
    if (!d) return;
    if (d.kind === "move") {
      if (d.started) {
        studio.move(d.from, d.to);
      } else {
        studio.playing = false;
        studio.seek(tAt(e.clientX));
      }
    } else if (d.kind === "trim") {
      studio.say(t("Morceau rogné"));
    } else if (d.kind === "music" && !d.started) {
      studio.playing = false;
      studio.seek(tAt(e.clientX));
    }
  }

  // ---------- glisser-déposer depuis la liste des clips ----------
  function dragOver(e: DragEvent) {
    if (!e.dataTransfer?.types.includes("application/x-rewind-clip")) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    dropAt = insertionIndex(tAt(e.clientX));
  }
  function drop(e: DragEvent) {
    const ids = e.dataTransfer?.getData("application/x-rewind-clip");
    const at = dropAt ?? app.studio.length;
    dropAt = null;
    if (!ids) return;
    e.preventDefault();
    const clips = ids.split(",").map((id) => app.clips.find((c) => c.id === id)).filter((c) => !!c);
    studio.insertClips(clips, at);
  }
  /** Position (x) de la ligne d'insertion. */
  function insertX(idx: number, skip = -1) {
    const order = app.studio.map((_, k) => k).filter((k) => k !== skip);
    if (!order.length) return X(0);
    if (idx >= order.length) {
      const last = order[order.length - 1];
      return X(L.offsets[last] + L.lens[last]);
    }
    return X(L.offsets[order[idx]]);
  }

  // ---------- zoom ----------
  function zoomTo(z: number, anchorClientX?: number) {
    if (!scroller) return;
    const r = scroller.getBoundingClientRect();
    const ax = anchorClientX ?? r.left + width / 2;
    const ta = tAt(ax);
    studio.zoom = Math.min(40, Math.max(1, z));
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollLeft = Math.max(0, PAD + ta * fitPps * studio.zoom - (ax - r.left));
    });
  }
  function wheel(e: WheelEvent) {
    if (e.ctrlKey) {
      e.preventDefault();
      zoomTo(studio.zoom * (e.deltaY < 0 ? 1.18 : 1 / 1.18), e.clientX);
    } else if (scroller && Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      e.preventDefault();
      scroller.scrollLeft += e.deltaY;
    }
  }

  // Suit la tête de lecture pendant la lecture
  $effect(() => {
    if (!studio.playing || !scroller) return;
    const px = X(studio.t);
    if (px < scroller.scrollLeft + 30 || px > scroller.scrollLeft + width - 80) scroller.scrollLeft = Math.max(0, px - width * 0.25);
  });

  // ---------- menus ----------
  let speedOpen = $state(false);
  const junctionPiece = $derived(studio.junction !== null ? app.studio[studio.junction - 1] : null);
  function openJunction(k: number) {
    studio.junction = studio.junction === k ? null : k;
    speedOpen = false;
  }
  function pickTransition(type: Transition) {
    if (studio.junction === null) return;
    studio.setTransition(studio.junction, type);
  }
  function junctionX(k: number) {
    return X(L.offsets[k] + L.trans[k] / 2);
  }

  const markerHot = new Set(["ace", "4k", "3k", "clutch", "onetap", "collateral", "streak"]);
  const accent = $derived(app.settings?.accent || "#c8f031");
  const musicColor = "#7b86ff";
</script>

<svelte:window onpointermove={onMove} onpointerup={onUp} />

<div class="tl">
  <!-- Barre d'outils -->
  <div class="bar">
    <div class="grp">
      <button class="tb" onclick={() => studio.undo()} disabled={!studio.canUndo} title={t("Annuler (Ctrl+Z)")}><Undo2 size={16} /></button>
      <button class="tb" onclick={() => studio.redo()} disabled={!studio.canRedo} title={t("Rétablir (Ctrl+Y)")}><Redo2 size={16} /></button>
      <span class="sep"></span>
      <button class="tb wide" onclick={() => studio.split()} disabled={!app.studio.length} title={t("Couper à la tête de lecture (S)")}><Scissors size={15} />{t("Couper")}</button>
      <button class="tb wide" onclick={() => studio.duplicate()} disabled={!studio.sel} title={t("Dupliquer (Ctrl+D)")}><CopyPlus size={15} />{t("Dupliquer")}</button>
      <button class="tb wide danger" onclick={() => studio.remove()} disabled={!studio.sel} title={t("Supprimer (Suppr)")}><Trash2 size={15} />{t("Supprimer")}</button>
      <span class="sep"></span>
      <div class="pop-wrap">
        <button class="tb wide" class:on={speedOpen} onclick={() => { speedOpen = !speedOpen; studio.junction = null; }} disabled={!studio.sel} title={t("Vitesse du morceau")}>
          <Gauge size={15} />{studio.sel ? `${clampSpeed(studio.sel.speed)}×` : t("Vitesse")}
        </button>
        {#if speedOpen && studio.sel}
          <div class="pop speeds">
            {#each SPEEDS as v}
              <button class:on={clampSpeed(studio.sel.speed) === v} onclick={() => { studio.setSpeed(v); speedOpen = false; }}>{v}×</button>
            {/each}
          </div>
        {/if}
      </div>
      <button
        class="tb wide"
        onclick={() => openJunction(studio.selIndex + 1)}
        disabled={studio.selIndex < 0 || studio.selIndex >= app.studio.length - 1}
        title={t("Transition vers le morceau suivant")}
      ><Blend size={15} />{t("Transition")}</button>
    </div>
    <div class="grp right">
      {#if studio.hint}<span class="hint">{studio.hint}</span>{/if}
      <span class="clock mono">{fmtTime(studio.t, true)}<span class="faint"> / {fmtTime(L.total, true)}</span></span>
      <span class="sep"></span>
      <button class="tb" class:on={studio.snap} onclick={() => (studio.snap = !studio.snap)} title={t("Aimant : colle aux kills, à la tête de lecture et aux bords")}><Magnet size={16} /></button>
      <button class="tb" onclick={() => zoomTo(studio.zoom / 1.4)} disabled={studio.zoom <= 1} title={t("Dézoomer (−)")}><ZoomOut size={16} /></button>
      <input class="zoom" type="range" min="1" max="40" step="0.1" value={studio.zoom} oninput={(e) => zoomTo(+(e.currentTarget as HTMLInputElement).value)} aria-label={t("Zoom de la frise")} />
      <button class="tb" onclick={() => zoomTo(studio.zoom * 1.4)} title={t("Zoomer (+)")}><ZoomIn size={16} /></button>
      <button class="tb" onclick={() => zoomTo(1)} title={t("Tout voir")}><Maximize2 size={15} /></button>
    </div>
  </div>

  <!-- Frise -->
  <div
    class="scroller"
    class:dragging={!!drag}
    bind:this={scroller}
    bind:clientWidth={width}
    onscroll={() => (scrollX = scroller?.scrollLeft ?? 0)}
    onwheel={wheel}
    ondragover={dragOver}
    ondragleave={() => (dropAt = null)}
    ondrop={drop}
    onpointermove={(e) => { if (!drag) hoverT = tAt(e.clientX); }}
    onpointerleave={() => (hoverT = null)}
    role="application"
    aria-label={t("Frise du montage")}
  >
    <div class="content" style="width:{contentW}px">
      <!-- Règle -->
      <div class="ruler" onpointerdown={scrubDown} role="presentation" style="--minor:{(step * pps) / 5}px;--pad:{PAD}px">
        {#each ticks as t}
          <span class="tick" style="left:{X(t)}px">{fmtTick(t)}</span>
        {/each}
      </div>

      <!-- Piste vidéo -->
      <div class="track video" onpointerdown={scrubDown} role="presentation">
        <span class="lane-tag"><Film size={12} /></span>
        {#if !app.studio.length}
          <div class="drop-empty" class:over={dropAt !== null}>
            <Plus size={18} /> {t("Glisse des clips ici, ou clique sur + dans la liste à gauche")}
          </div>
        {/if}
        {#each app.studio as p, i (p.uid)}
          {@const left = X(L.offsets[i])}
          {@const w = Math.max(6, L.lens[i] * pps)}
          {@const tl = tiles(p, left, w)}
          {@const moving = drag?.kind === "move" && drag.started && drag.uid === p.uid}
          <div
            class="piece"
            class:sel={p.uid === studio.selected}
            class:moving
            style="left:{left}px;width:{w}px;--c:{gameTone(p.game)};z-index:{moving ? 50 : i + 1};{moving && drag?.kind === 'move' ? `transform:translate(${drag.dx}px,-6px)` : ''}"
            onpointerdown={(e) => pieceDown(e, p, i)}
            role="button"
            tabindex="0"
            aria-label={p.title}
          >
            <div class="strip">
              {#each tl.list as tile}
                {#if tl.s}
                  <i style="left:{tile.left}px;width:{tl.tileW}px;background-image:url('{fileUrl(tl.s.sprite)}');background-size:{tl.s.frames * tl.tileW}px {STRIP_H}px;background-position:{tile.pos}px 0"></i>
                {:else if p.thumb}
                  <i style="left:{tile.left}px;width:{tl.tileW}px;background-image:url('{fileUrl(p.thumb)}');background-size:cover"></i>
                {/if}
              {/each}
            </div>
            <canvas class="wave" use:wave={{ w: media.waves[p.path], start: p.start, end: p.end, width: w, color: "rgba(255,255,255,.85)", v: p.volume }}></canvas>
            {#each p.markers.filter((m) => m.time >= p.start && m.time <= p.end) as m}
              <span
                class="mk"
                class:hot={markerHot.has(m.kind)}
                style="left:{((m.time - p.start) / clampSpeed(p.speed)) * pps}px"
                title={m.label || KIND_LABEL[m.kind] || m.kind}
              ></span>
            {/each}
            <div class="lbl">
              <span class="name">{p.title}</span>
              {#if clampSpeed(p.speed) !== 1}<b class="badge">{clampSpeed(p.speed)}×</b>{/if}
              {#if p.volume <= 0.001}<b class="badge mute"><VolumeX size={11} /></b>{/if}
              <span class="dur mono">{L.lens[i].toFixed(1)} s</span>
            </div>
            <span class="h l" onpointerdown={(e) => handleDown(e, p, i, "start")} role="separator" aria-label={t("Rogner le début")}><em></em></span>
            <span class="h r" onpointerdown={(e) => handleDown(e, p, i, "end")} role="separator" aria-label={t("Rogner la fin")}><em></em></span>
          </div>
        {/each}
        <!-- Chevauchements (transitions) et boutons de jonction -->
        {#each app.studio as _, k}
          {#if k > 0}
            {#if L.trans[k] > 0}
              <div class="overlap" style="left:{X(L.offsets[k])}px;width:{L.trans[k] * pps}px"></div>
            {/if}
            <button
              class="junction"
              class:on={app.studio[k - 1].transition !== "none"}
              class:open={studio.junction === k}
              style="left:{junctionX(k)}px"
              onpointerdown={(e) => e.stopPropagation()}
              onclick={() => openJunction(k)}
              title={t("Transition entre les morceaux {a} et {b}", { a: k, b: k + 1 })}
            ><Blend size={12} /></button>
          {/if}
        {/each}
        {#if drag?.kind === "move" && drag.started}
          <div class="insert" style="left:{insertX(drag.to, drag.from)}px"></div>
        {/if}
        {#if dropAt !== null && app.studio.length}
          <div class="insert" style="left:{insertX(dropAt)}px"></div>
        {/if}
      </div>

      <!-- Piste musique -->
      <div class="track music" onpointerdown={scrubDown} role="presentation">
        <span class="lane-tag"><Music size={12} /></span>
        {#if app.studioMusic}
          {@const m = app.studioMusic}
          {@const mw = Math.max(8, (L.total - m.at) * pps)}
          <div class="mblock" class:sel={studio.musicSelected} style="left:{X(m.at)}px;width:{mw}px" onpointerdown={musicDown} role="button" tabindex="0" aria-label={t("Musique")}>
            <canvas class="mwave" use:wave={{ w: media.waves[m.path], start: m.offset, end: m.offset + Math.max(0.1, L.total - m.at), width: mw, loop: m.duration, color: musicColor, v: m.volume }}></canvas>
            {#if m.fadeIn > 0}<span class="fade in" style="width:{m.fadeIn * pps}px"></span>{/if}
            {#if m.fadeOut > 0}<span class="fade out" style="width:{m.fadeOut * pps}px"></span>{/if}
            <span class="mlabel"><Music size={11} />{m.name} · {Math.round(m.volume * 100)} %{m.duck ? ` · ${t("atténuation auto")}` : ""}</span>
          </div>
        {:else}
          <button class="addmusic" style="left:{X(0)}px;width:{Math.max(220, L.total * pps)}px" onpointerdown={(e) => e.stopPropagation()} onclick={onAddMusic}>
            <Plus size={14} /> {t("Ajouter une musique de fond")}
          </button>
        {/if}
      </div>

      <!-- Tête de lecture, survol, aimant -->
      {#if hoverT !== null && !drag && hoverT >= 0 && hoverT <= L.total}
        <div class="hoverline" style="left:{X(hoverT)}px"><span class="mono">{fmtTime(hoverT, true)}</span></div>
      {/if}
      {#if snapX !== null}<div class="snapline" style="left:{snapX}px"></div>{/if}
      {#if app.studio.length}
        <div class="playhead" style="left:{X(studio.t)}px"><i></i></div>
      {/if}
      {#if trimTip}<div class="tip mono" style="left:{trimTip.x}px">{trimTip.text}</div>{/if}
    </div>
  </div>

  <!-- Menu des transitions (jonction choisie) -->
  {#if studio.junction !== null && junctionPiece}
    {@const k = studio.junction}
    <div class="jpanel appear">
      <div class="jhead">
        <strong>{t("Transition")}</strong>
        <span class="faint">{t("entre les morceaux {a} et {b}", { a: k, b: k + 1 })}</span>
        <button class="x" onclick={() => (studio.junction = null)} aria-label={t("Fermer")}>×</button>
      </div>
      <div class="jgrid">
        {#each TRANSITIONS as tr}
          <button class="jt" class:on={junctionPiece.transition === tr.id} onclick={() => pickTransition(tr.id)} title={t(tr.hint)}>
            <span class="jprev {tr.id}"><i></i><i></i></span>
            {t(tr.label)}
          </button>
        {/each}
      </div>
      {#if junctionPiece.transition !== "none"}
        <label class="jdur">
          {t("Durée")} <b class="mono">{(L.trans[k] || junctionPiece.transitionDur).toFixed(1)} s</b>
          <input type="range" min="0.2" max="2" step="0.1" value={junctionPiece.transitionDur}
            onpointerdown={() => studio.snapshot()}
            oninput={(e) => studio.update(junctionPiece.uid, { transitionDur: +(e.currentTarget as HTMLInputElement).value }, false)} />
        </label>
      {/if}
      <div class="jfoot">
        <button class="btn small ghost" onclick={() => studio.setTransitionAll(junctionPiece.transition)}>{t("Appliquer à toutes les jonctions")}</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .tl { position: relative; display: flex; flex-direction: column; min-height: 0; min-width: 0; height: 100%; background: var(--bg-2); border-top: 1px solid var(--line); }
  .bar { display: flex; justify-content: space-between; align-items: center; gap: 10px; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  .grp { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .grp.right { gap: 6px; }
  .tb { height: 30px; min-width: 30px; padding: 0 8px; border-radius: 8px; display: inline-flex; align-items: center; justify-content: center; gap: 6px; color: var(--text-2); font-size: 12.5px; font-weight: 550; transition: background var(--dur), color var(--dur); }
  .tb:hover:not(:disabled) { background: var(--panel-2); color: var(--text); }
  .tb.on { background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); }
  .tb.danger:hover:not(:disabled) { color: var(--rec); background: color-mix(in srgb, var(--rec) 12%, transparent); }
  .tb:disabled { opacity: .35; cursor: default; }
  .sep { width: 1px; height: 18px; background: var(--line); margin: 0 4px; }
  .clock { font-size: 12.5px; min-width: 116px; text-align: right; }
  .hint { font-size: 12px; color: var(--accent); animation: rise 240ms var(--ease) both; white-space: nowrap; }
  .zoom { width: 90px; }
  .pop-wrap { position: relative; }
  .pop { position: absolute; bottom: calc(100% + 8px); left: 0; z-index: 60; display: flex; gap: 4px; padding: 6px; border-radius: 12px; background: var(--panel); border: 1px solid var(--line-2); box-shadow: var(--shadow); }
  .speeds button { height: 30px; padding: 0 9px; border-radius: 8px; font-size: 12.5px; font-weight: 600; color: var(--text-2); font-family: var(--mono); }
  .speeds button:hover { background: var(--panel-3); color: var(--text); }
  .speeds button.on { background: var(--accent); color: var(--accent-ink); }

  .scroller { position: relative; flex: 1; min-height: 0; width: 100%; overflow-x: auto; overflow-y: hidden; }
  .scroller.dragging { cursor: grabbing; }
  .content { position: relative; height: 100%; min-height: 200px; }
  .ruler { position: relative; height: 26px; cursor: col-resize; border-bottom: 1px solid var(--line);
    background-image: repeating-linear-gradient(90deg, var(--line-2) 0 1px, transparent 1px var(--minor)); background-position: var(--pad) 100%; background-size: 100% 5px; background-repeat: repeat-x; }
  .tick { position: absolute; top: 4px; font-family: var(--mono); font-size: 10.5px; color: var(--text-3); padding-left: 5px; border-left: 1px solid var(--line-2); line-height: 16px; pointer-events: none; white-space: nowrap; }

  .track { position: relative; }
  .track.video { height: 112px; margin-top: 10px; }
  .track.music { height: 44px; margin-top: 6px; }
  .lane-tag { position: absolute; left: 3px; top: 50%; transform: translateY(-50%); color: var(--text-3); opacity: .6; pointer-events: none; }

  .piece { position: absolute; top: 8px; height: 90px; border-radius: 10px; overflow: hidden; background: var(--panel-3); cursor: grab; box-shadow: 0 0 0 1px color-mix(in srgb, var(--c) 45%, transparent), 0 6px 16px -10px rgba(0,0,0,.8); transition: box-shadow var(--dur), transform 160ms var(--ease); touch-action: none; }
  .piece:hover { box-shadow: 0 0 0 1px color-mix(in srgb, var(--c) 80%, transparent), 0 8px 20px -10px rgba(0,0,0,.9); }
  .piece.sel { box-shadow: 0 0 0 2px var(--accent), 0 0 0 5px color-mix(in srgb, var(--accent) 22%, transparent), 0 10px 26px -12px rgba(0,0,0,.9); }
  .piece.moving { opacity: .85; cursor: grabbing; transition: none; }
  .strip { position: absolute; left: 0; right: 0; top: 0; height: 62px; overflow: hidden; background: #000; }
  .strip i { position: absolute; top: 0; height: 100%; background-repeat: no-repeat; border-right: 1px solid rgba(0,0,0,.35); }
  .wave { position: absolute; left: 0; bottom: 0; height: 24px; background: color-mix(in srgb, var(--c) 34%, #0c0d11); }
  .lbl { position: absolute; left: 14px; right: 14px; top: 5px; display: flex; align-items: center; gap: 5px; font-size: 11.5px; font-weight: 650; color: #fff; text-shadow: 0 1px 3px rgba(0,0,0,.9); pointer-events: none; min-width: 0; }
  .lbl .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .lbl .dur { margin-left: auto; font-size: 10.5px; font-weight: 500; opacity: .85; padding: 1px 5px; border-radius: 5px; background: rgba(0,0,0,.45); text-shadow: none; }
  .badge { display: inline-flex; align-items: center; padding: 1px 5px; border-radius: 5px; background: var(--accent); color: var(--accent-ink); font-size: 10.5px; text-shadow: none; }
  .badge.mute { background: var(--rec); color: #fff; }
  .mk { position: absolute; top: 52px; width: 9px; height: 9px; margin-left: -4.5px; transform: rotate(45deg); background: #fff; border: 1.5px solid rgba(0,0,0,.6); border-radius: 2px; z-index: 2; }
  .mk.hot { background: #ffcf3f; }
  .h { position: absolute; top: 0; bottom: 0; width: 14px; z-index: 3; cursor: ew-resize; display: grid; place-items: center; background: color-mix(in srgb, var(--accent) 88%, transparent); opacity: 0; transition: opacity var(--dur); touch-action: none; }
  .h em { width: 2px; height: 22px; border-radius: 2px; background: var(--accent-ink); opacity: .7; }
  .piece:hover .h { opacity: .55; }
  .piece.sel .h { opacity: 1; }
  .h:hover { opacity: 1 !important; }
  .h.l { left: 0; }
  .h.r { right: 0; }

  .overlap { position: absolute; top: 8px; height: 90px; z-index: 40; pointer-events: none; border-radius: 6px;
    background: repeating-linear-gradient(135deg, color-mix(in srgb, var(--accent) 38%, transparent) 0 5px, transparent 5px 10px); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 70%, transparent); }
  .junction { position: absolute; top: 41px; width: 26px; height: 26px; margin-left: -13px; z-index: 45; border-radius: 50%; display: grid; place-items: center; background: var(--panel); color: var(--text-2); border: 1px solid var(--line-2); box-shadow: var(--shadow-s); transition: transform 140ms var(--ease), background var(--dur); }
  .junction:hover { transform: scale(1.15); color: var(--text); }
  .junction.on { background: var(--accent); color: var(--accent-ink); border-color: transparent; }
  .junction.open { outline: 2px solid var(--accent); outline-offset: 2px; }
  .insert { position: absolute; top: 2px; height: 102px; width: 3px; margin-left: -1.5px; border-radius: 3px; background: var(--accent); box-shadow: 0 0 12px var(--accent); z-index: 60; pointer-events: none; }
  .drop-empty { position: absolute; left: 20px; right: 20px; top: 8px; height: 90px; border-radius: 12px; border: 1.5px dashed var(--line-2); display: flex; align-items: center; justify-content: center; gap: 8px; color: var(--text-3); font-size: 13px; transition: all var(--dur); }
  .drop-empty.over { border-color: var(--accent); color: var(--accent); background: color-mix(in srgb, var(--accent) 7%, transparent); }

  .mblock { position: absolute; top: 4px; height: 36px; border-radius: 9px; overflow: hidden; cursor: grab; background: color-mix(in srgb, #7b86ff 20%, var(--panel)); box-shadow: 0 0 0 1px color-mix(in srgb, #7b86ff 55%, transparent); touch-action: none; }
  .mblock.sel { box-shadow: 0 0 0 2px var(--accent); }
  .mwave { position: absolute; left: 0; top: 6px; height: 24px; }
  .mlabel { position: absolute; left: 8px; top: 3px; display: flex; align-items: center; gap: 5px; font-size: 11px; font-weight: 600; color: var(--text); text-shadow: 0 1px 2px rgba(0,0,0,.5); pointer-events: none; white-space: nowrap; }
  .fade { position: absolute; top: 0; bottom: 0; pointer-events: none; }
  .fade.in { left: 0; background: linear-gradient(90deg, rgba(0,0,0,.55), transparent); }
  .fade.out { right: 0; background: linear-gradient(270deg, rgba(0,0,0,.55), transparent); }
  .addmusic { position: absolute; top: 4px; height: 36px; border-radius: 9px; border: 1.5px dashed var(--line-2); display: flex; align-items: center; justify-content: center; gap: 6px; color: var(--text-3); font-size: 12.5px; transition: all var(--dur); }
  .addmusic:hover { border-color: #7b86ff; color: #9aa3ff; background: color-mix(in srgb, #7b86ff 8%, transparent); }

  .playhead { position: absolute; top: 0; bottom: 0; width: 2px; margin-left: -1px; background: var(--text); z-index: 70; pointer-events: none; box-shadow: 0 0 6px rgba(0,0,0,.6); }
  .playhead i { position: absolute; top: 0; left: -6px; width: 14px; height: 14px; background: var(--text); clip-path: polygon(0 0, 100% 0, 100% 55%, 50% 100%, 0 55%); }
  .hoverline { position: absolute; top: 0; bottom: 0; width: 1px; background: color-mix(in srgb, var(--text) 35%, transparent); z-index: 65; pointer-events: none; }
  .hoverline span { position: absolute; top: 4px; left: 5px; font-size: 10.5px; padding: 1px 5px; border-radius: 5px; background: var(--panel-3); color: var(--text); white-space: nowrap; }
  .snapline { position: absolute; top: 0; bottom: 0; width: 1px; background: #ffcf3f; z-index: 66; pointer-events: none; box-shadow: 0 0 6px #ffcf3f; }
  .tip { position: absolute; top: 30px; transform: translateX(-50%); z-index: 80; padding: 4px 8px; border-radius: 7px; background: var(--text); color: var(--bg); font-size: 11.5px; white-space: nowrap; pointer-events: none; }

  .jpanel { position: absolute; left: 50%; bottom: calc(100% + 10px); transform: translateX(-50%); z-index: 90; width: min(560px, 92%); padding: 14px; border-radius: 16px; background: var(--panel); border: 1px solid var(--line-2); box-shadow: var(--shadow); }
  .jhead { display: flex; align-items: baseline; gap: 8px; margin-bottom: 10px; font-size: 13px; }
  .jhead .x { margin-left: auto; font-size: 20px; line-height: 1; color: var(--text-3); width: 26px; height: 26px; border-radius: 7px; }
  .jhead .x:hover { background: var(--panel-3); color: var(--text); }
  .jgrid { display: grid; grid-template-columns: repeat(7, 1fr); gap: 6px; }
  .jt { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 8px 4px; border-radius: 10px; font-size: 11.5px; color: var(--text-2); border: 1px solid var(--line); transition: all var(--dur); }
  .jt:hover { border-color: var(--line-2); color: var(--text); }
  .jt.on { border-color: var(--accent); color: var(--accent); background: color-mix(in srgb, var(--accent) 9%, transparent); }
  .jprev { position: relative; width: 44px; height: 26px; border-radius: 5px; overflow: hidden; background: #3a5a8c; }
  .jprev i { position: absolute; inset: 0; }
  .jprev i:first-child { background: linear-gradient(135deg, #f0a23b, #b8612a); }
  .jprev i:last-child { background: linear-gradient(135deg, #5a9bff, #2b4f8f); opacity: 0; }
  .jt:hover .jprev.fade i:last-child { animation: jfade 1.2s ease-in-out infinite; }
  .jt:hover .jprev.black i:last-child { animation: jblack 1.2s ease-in-out infinite; }
  .jt:hover .jprev.white i:last-child { animation: jwhite 1.2s ease-in-out infinite; }
  .jt:hover .jprev.zoom i:first-child { animation: jzoom 1.2s ease-in infinite; }
  .jt:hover .jprev.zoom i:last-child { animation: jfade 1.2s ease-in-out infinite; }
  .jt:hover .jprev.slide i:last-child { opacity: 1; animation: jslide 1.2s ease-in-out infinite; }
  .jt:hover .jprev.wipe i:last-child { opacity: 1; animation: jwipe 1.2s ease-in-out infinite; }
  @keyframes jfade { 0%, 20% { opacity: 0; } 70%, 100% { opacity: 1; } }
  @keyframes jblack { 0%, 15% { opacity: 0; background: #000; } 45% { opacity: 1; background: #000; } 50% { background: linear-gradient(135deg, #5a9bff, #2b4f8f); opacity: 0; } 100% { opacity: 1; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } }
  @keyframes jwhite { 0%, 15% { opacity: 0; background: #fff; } 45% { opacity: 1; background: #fff; } 50% { background: linear-gradient(135deg, #5a9bff, #2b4f8f); opacity: 1; } 100% { opacity: 1; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } }
  @keyframes jzoom { 0% { transform: scale(1); } 100% { transform: scale(2.2); } }
  @keyframes jslide { 0% { transform: translateX(100%); } 70%, 100% { transform: translateX(0); } }
  @keyframes jwipe { 0% { clip-path: inset(0 0 0 100%); } 70%, 100% { clip-path: inset(0 0 0 0); } }
  .jdur { display: flex; align-items: center; gap: 10px; margin-top: 12px; font-size: 12.5px; color: var(--text-2); }
  .jdur input { flex: 1; }
  .jfoot { display: flex; justify-content: flex-end; margin-top: 8px; }
</style>
