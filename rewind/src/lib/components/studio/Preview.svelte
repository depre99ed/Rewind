<script lang="ts">
  // Aperçu du Studio : piste principale (deux lecteurs pour enchaîner sans trou, transitions), zoom animé,
  // couleurs, arrêts sur image, calques (texte, images, GIF, vidéos, flou), touches et sons.
  // Toutes les positions passent par model.ts : l'export (render.rs) reçoit exactement les mêmes.
  import { onMount } from "svelte";
  import { Play, Pause, SkipBack, SkipForward, Maximize, Film, Sparkles, RotateCw } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { studio, media, fmtTime } from "$lib/studio/store.svelte";
  import { FORMAT_RATIO, kfValue, layerState, speedAt, type Layer } from "$lib/studio/model";
  import { fileUrl } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import LayerView from "./LayerView.svelte";
  import KeysOverlay from "../KeysOverlay.svelte";

  const P = $derived(studio.project);
  const L = $derived(studio.lay);
  const ratio = $derived(FORMAT_RATIO[P.format]);

  let stage: HTMLDivElement | undefined = $state();
  let frameEl: HTMLDivElement | undefined = $state();
  let fw = $state(640);
  let fh = $state(360);
  let vA: HTMLVideoElement | undefined = $state();
  let vB: HTMLVideoElement | undefined = $state();
  let flash: HTMLDivElement | undefined = $state();
  let blurCanvas: HTMLCanvasElement | undefined = $state();
  let zoomBox: HTMLDivElement | undefined = $state();

  const empty = $derived(!P.pieces.length && !P.tracks.length);
  const k = $derived(studio.indexAt(studio.t));
  const current = $derived(P.pieces[k] ?? null);

  // Calques visibles à l'instant t, du bas vers le haut (la piste 0 est au-dessus de tout)
  const visible = $derived.by(() => {
    const out: { l: Layer; z: number }[] = [];
    const n = P.tracks.length;
    P.tracks.forEach((tr, i) => {
      if (tr.kind !== "layer" || tr.hidden) return;
      for (const l of tr.items) if (studio.t >= l.at - 1e-3 && studio.t < l.at + l.dur) out.push({ l, z: 10 + (n - i) });
    });
    return out;
  });

  // Touches du morceau affiché (si enregistrées avec le clip)
  const keyEvents = $derived(current && current.kind !== "freeze" ? media.inputs[current.clipId] ?? [] : []);
  const keyTime = $derived(k >= 0 ? studio.srcTime(k, studio.t) : 0);
  const hasKeys = $derived(P.pieces.some((p) => (media.inputs[p.clipId]?.length ?? 0) > 0));

  $effect(() => {
    for (const p of P.pieces) {
      media.load(p.path);
      media.loadInputs(p.clipId);
    }
  });

  // ---------------------------------------------------------------------------
  // Lecteur principal : deux vidéos. Pendant que l'une joue, l'autre attend au début du morceau suivant.
  // ---------------------------------------------------------------------------
  type Slot = { el: HTMLVideoElement; idx: number; path: string };
  let slots: Slot[] = [];
  let raf = 0;
  let lastNow = 0;
  const audios = new Map<string, HTMLAudioElement>();

  function prepare(s: Slot, idx: number) {
    const p = P.pieces[idx];
    if (!p) return;
    s.idx = idx;
    if (s.path !== p.path) {
      s.path = p.path;
      s.el.src = fileUrl(p.path);
    }
  }
  function sync(s: Slot, idx: number, t: number, play: boolean) {
    prepare(s, idx);
    const p = P.pieces[idx];
    const el = s.el;
    const target = studio.srcTime(idx, t);
    const frozen = p.kind === "freeze";
    const rate = frozen ? 1 : speedAt(p, target);
    if (Math.abs(el.playbackRate - rate) > 0.01) el.playbackRate = rate;
    if (el.readyState < 1) return;
    const drift = Math.abs(el.currentTime - target);
    if (play && !frozen) {
      if (drift > 0.25 && !el.seeking) el.currentTime = target;
      if (el.paused) el.play().catch(() => {});
    } else {
      if (!el.paused) el.pause();
      if (drift > 0.015 && !el.seeking) el.currentTime = target;
    }
    // Couleur : mêmes formules que l'export (filtres CSS)
    const c = p.color;
    el.style.filter = c.b === 1 && c.c === 1 && c.s === 1 ? "" : `brightness(${c.b}) contrast(${c.c}) saturate(${c.s})`;
  }
  function reset(el: HTMLVideoElement) {
    el.style.opacity = "0";
    el.style.transform = "";
    el.style.clipPath = "";
    el.style.zIndex = "1";
  }

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    const pieces = P.pieces;
    const total = studio.total;
    if (studio.playing) {
      const dt = lastNow ? Math.min(0.1, (now - lastNow) / 1000) : 0;
      studio.t = Math.min(total, studio.t + dt);
      if (studio.t >= total - 0.001) {
        studio.playing = false;
        studio.t = Math.max(0, total - 0.001);
      }
    }
    lastNow = now;
    const t = studio.t;
    const play = studio.playing;
    syncAudio(t, play);
    syncPip(t, play);
    if (!slots.length) return;
    const kk = studio.indexAt(t);
    if (!pieces.length || kk < 0) {
      for (const s of slots) {
        if (!s.el.paused) s.el.pause();
        reset(s.el);
      }
      return;
    }
    const overlap = kk > 0 && L.trans[kk] > 0 && t < L.offsets[kk] + L.trans[kk];
    const needed = overlap ? [kk - 1, kk] : [kk];
    const used = new Set<Slot>();
    const byIdx = new Map<number, Slot>();
    for (const n of needed) {
      const s = slots.find((x) => x.idx === n && x.path === pieces[n].path && !used.has(x));
      if (s) { used.add(s); byIdx.set(n, s); }
    }
    for (const n of needed) {
      if (byIdx.has(n)) continue;
      const s = slots.find((x) => !used.has(x))!;
      used.add(s);
      byIdx.set(n, s);
    }
    for (const n of needed) sync(byIdx.get(n)!, n, t, play);
    const spare = slots.find((x) => !used.has(x));
    if (spare) {
      reset(spare.el);
      spare.el.muted = true;
      if (kk + 1 < pieces.length) sync(spare, kk + 1, L.offsets[kk + 1], false);
      else if (!spare.el.paused) spare.el.pause();
    }
    const main = byIdx.get(kk)!;
    if (flash) flash.style.opacity = "0";
    if (!overlap) {
      reset(main.el);
      main.el.style.opacity = "1";
      main.el.muted = pieces[kk].kind === "freeze";
      main.el.volume = Math.min(1, Math.max(0, pieces[kk].volume));
    } else {
      const out = byIdx.get(kk - 1)!;
      const pr = Math.min(1, Math.max(0, (t - L.offsets[kk]) / L.trans[kk]));
      const type = pieces[kk - 1].transition;
      reset(out.el);
      reset(main.el);
      main.el.style.zIndex = "2";
      out.el.style.opacity = "1";
      main.el.style.opacity = "1";
      if (type === "fade") main.el.style.opacity = String(pr);
      else if (type === "black") {
        out.el.style.opacity = String(pr < 0.5 ? 1 - pr * 2 : 0);
        main.el.style.opacity = String(pr < 0.5 ? 0 : pr * 2 - 1);
      } else if (type === "white") {
        out.el.style.opacity = pr < 0.5 ? "1" : "0";
        main.el.style.opacity = pr < 0.5 ? "0" : "1";
        if (flash) flash.style.opacity = String(1 - Math.abs(pr * 2 - 1));
      } else if (type === "zoom") {
        out.el.style.transform = `scale(${1 + pr * 1.6})`;
        main.el.style.opacity = String(pr);
      } else if (type === "slide") {
        out.el.style.transform = `translateX(${-pr * 100}%)`;
        main.el.style.transform = `translateX(${(1 - pr) * 100}%)`;
      } else if (type === "wipe") main.el.style.clipPath = `inset(0 0 0 ${(1 - pr) * 100}%)`;
      else main.el.style.opacity = String(pr);
      out.el.muted = main.el.muted = false;
      out.el.volume = Math.min(1, Math.max(0, pieces[kk - 1].volume * (1 - pr)));
      main.el.volume = Math.min(1, Math.max(0, pieces[kk].volume * pr));
    }
    // Zoom animé du morceau affiché (même courbe que l'export)
    if (zoomBox) {
      const p = pieces[kk];
      const lt = t - L.offsets[kk];
      if (p.zoom.length) {
        const z = kfValue(p.zoom, "z", lt, 1), x = kfValue(p.zoom, "x", lt, 0.5), y = kfValue(p.zoom, "y", lt, 0.5);
        zoomBox.style.transformOrigin = `${x * 100}% ${y * 100}%`;
        zoomBox.style.transform = z > 1.001 ? `scale(${z})` : "";
      } else zoomBox.style.transform = "";
    }
    // Fond flou d'un format vertical ou carré
    if (P.fit === "blur" && P.format !== "16:9" && blurCanvas && main.el.readyState >= 2) {
      const ctx = blurCanvas.getContext("2d");
      const vw = main.el.videoWidth, vh = main.el.videoHeight;
      if (ctx && vw && vh) {
        const cw = blurCanvas.width, ch = blurCanvas.height;
        const scale = Math.max(cw / vw, ch / vh);
        const sw = cw / scale, sh = ch / scale;
        ctx.filter = "blur(3px)";
        ctx.drawImage(main.el, (vw - sw) / 2, (vh - sh) / 2, sw, sh, 0, 0, cw, ch);
      }
    }
  }

  // Sons (musique, bruitages, voix off) : un lecteur par élément
  function syncAudio(t: number, play: boolean) {
    const alive = new Set<string>();
    for (const tr of P.tracks) {
      if (tr.kind !== "audio") continue;
      for (const a of tr.items) {
        alive.add(a.uid);
        let el = audios.get(a.uid);
        if (!el) {
          el = new Audio();
          el.preload = "auto";
          audios.set(a.uid, el);
        }
        const url = fileUrl(a.path);
        if (el.dataset.src !== url) {
          el.dataset.src = url;
          el.src = url;
        }
        const local = t - a.at;
        const inside = local >= 0 && local < a.dur;
        if (play && inside && !tr.muted && !studio.recording) {
          const target = a.srcStart + local;
          if (el.readyState >= 1 && Math.abs(el.currentTime - target) > 0.3) el.currentTime = target;
          let v = a.kf.length ? kfValue(a.kf, "v", local, a.volume) : a.volume;
          if (a.fadeIn > 0) v *= Math.min(1, local / a.fadeIn);
          if (a.fadeOut > 0) v *= Math.min(1, Math.max(0, (a.dur - local) / a.fadeOut));
          el.volume = Math.min(1, Math.max(0, v));
          if (el.paused) el.play().catch(() => {});
        } else if (!el.paused) el.pause();
      }
    }
    for (const [id, el] of audios) if (!alive.has(id)) { el.pause(); audios.delete(id); }
  }
  // Vidéos en incrustation
  function syncPip(t: number, play: boolean) {
    if (!frameEl) return;
    for (const el of frameEl.querySelectorAll<HTMLVideoElement>("video[data-pip]")) {
      const l = studio.findLayer(el.dataset.pip!);
      if (!l || l.kind !== "video") continue;
      const local = t - l.at;
      const target = l.srcStart + local * l.speed;
      if (Math.abs(el.playbackRate - l.speed) > 0.01) el.playbackRate = l.speed;
      el.volume = Math.min(1, Math.max(0, l.volume));
      if (el.readyState < 1) continue;
      if (play) {
        if (Math.abs(el.currentTime - target) > 0.25) el.currentTime = target;
        if (el.paused) el.play().catch(() => {});
      } else {
        if (!el.paused) el.pause();
        if (Math.abs(el.currentTime - target) > 0.02) el.currentTime = target;
      }
    }
  }

  onMount(() => {
    if (vA && vB) slots = [{ el: vA, idx: -1, path: "" }, { el: vB, idx: -1, path: "" }];
    raf = requestAnimationFrame(frame);
    const pause = () => (studio.playing = false);
    window.addEventListener("rewind-pause", pause);
    return () => {
      cancelAnimationFrame(raf);
      studio.playing = false;
      for (const s of slots) s.el.pause();
      for (const el of audios.values()) el.pause();
      window.removeEventListener("rewind-pause", pause);
    };
  });

  function toggle() {
    if (empty) return;
    if (!studio.playing && studio.t >= studio.total - 0.05) studio.t = 0;
    studio.playing = !studio.playing;
  }
  function fullscreen() {
    if (document.fullscreenElement) document.exitFullscreen();
    else stage?.requestFullscreen().catch(() => {});
  }
  function quickStart() {
    const recent = app.clips.filter((c) => c.kind !== "screenshot" && c.duration > 0.5).slice(0, 3).reverse();
    studio.insertClips(recent);
  }

  // Clips glissés sur l'aperçu : ajoutés à la fin
  let over = $state(false);
  function drop(e: DragEvent) {
    over = false;
    const ids = e.dataTransfer?.getData("application/x-rewind-clip");
    if (!ids) return;
    e.preventDefault();
    studio.insertClips(ids.split(",").map((id) => app.clips.find((c) => c.id === id)).filter((c) => !!c));
  }

  // ---------------------------------------------------------------------------
  // Poignées : déplacer, redimensionner, tourner le calque choisi (ou les touches)
  // ---------------------------------------------------------------------------
  const sel = $derived(studio.layer);
  const selState = $derived(sel ? layerState(sel, studio.t - sel.at) : null);
  const selActive = $derived(!!sel && studio.t >= sel.at - 1e-3 && studio.t < sel.at + sel.dur);
  let selBox = $state<{ w: number; h: number }>({ w: 0, h: 0 });
  $effect(() => {
    // Taille affichée du calque choisi (mesurée dans l'aperçu)
    void studio.t, sel, fw, fh;
    if (!sel || !frameEl) return;
    const el = frameEl.querySelector<HTMLElement>(`[data-layer="${sel.uid}"]`);
    if (el) selBox = { w: el.offsetWidth, h: el.offsetHeight };
  });

  type Drag = { mode: "move" | "scale" | "rot" | "w" | "h" | "keys" | "keysScale"; x0: number; y0: number; v: Record<string, number> };
  let drag: Drag | null = null;
  function start(e: PointerEvent, mode: Drag["mode"]) {
    e.stopPropagation();
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    studio.playing = false;
    studio.snapshot();
    if (mode === "keys" || mode === "keysScale") drag = { mode, x0: e.clientX, y0: e.clientY, v: { x: P.keys.x, y: P.keys.y, scale: P.keys.scale } };
    else if (sel && selState) drag = { mode, x0: e.clientX, y0: e.clientY, v: { ...selState, w: sel.kind === "blur" ? sel.w : 0, h: sel.kind === "blur" ? sel.h : 0 } };
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const dx = (e.clientX - drag.x0) / fw, dy = (e.clientY - drag.y0) / fh;
    const snapC = (v: number) => (Math.abs(v - 0.5) < 0.012 ? 0.5 : v); // aimant au centre
    if (drag.mode === "keys") return studio.setKeys({ x: clamp01(snapC(drag.v.x + dx)), y: clamp01(snapC(drag.v.y + dy)) }, false);
    if (drag.mode === "keysScale") return studio.setKeys({ scale: Math.max(0.08, Math.min(0.9, drag.v.scale + dx * 2)) }, false);
    if (!sel) return;
    if (drag.mode === "move") {
      const nx = snapC(drag.v.x + dx), ny = snapC(drag.v.y + dy);
      guides = { v: nx === 0.5, h: ny === 0.5 };
      studio.setAnimated(sel.uid, { x: nx, y: ny }, false);
    }
    else if (drag.mode === "scale") {
      const r = frameEl!.getBoundingClientRect();
      const cx = r.left + drag.v.x * fw, cy = r.top + drag.v.y * fh;
      const d0 = Math.hypot(drag.x0 - cx, drag.y0 - cy), d1 = Math.hypot(e.clientX - cx, e.clientY - cy);
      const s = Math.max(0.05, Math.min(6, drag.v.scale * (d1 / Math.max(4, d0))));
      if (sel.kind === "blur") studio.updateLayer(sel.uid, { w: Math.min(1, Math.max(0.02, drag.v.w * (d1 / Math.max(4, d0)))), h: Math.min(1, Math.max(0.02, drag.v.h * (d1 / Math.max(4, d0)))) }, false);
      else studio.setAnimated(sel.uid, { scale: s }, false);
    } else if (drag.mode === "rot") {
      const r = frameEl!.getBoundingClientRect();
      const cx = r.left + drag.v.x * fw, cy = r.top + drag.v.y * fh;
      let a = (Math.atan2(e.clientY - cy, e.clientX - cx) * 180) / Math.PI + 90;
      if (e.shiftKey) a = Math.round(a / 15) * 15;
      else if (Math.abs(a) < 3) a = 0;
      studio.setAnimated(sel.uid, { rot: Math.round(a * 10) / 10 }, false);
    } else if (drag.mode === "w" && sel.kind === "blur") studio.updateLayer(sel.uid, { w: Math.min(1, Math.max(0.02, drag.v.w + dx * 2)) }, false);
    else if (drag.mode === "h" && sel.kind === "blur") studio.updateLayer(sel.uid, { h: Math.min(1, Math.max(0.02, drag.v.h + dy * 2)) }, false);
  }
  // Repères de centrage affichés quand le calque se cale au milieu
  let guides = $state({ v: false, h: false });
  function end() {
    guides = { v: false, h: false };
    drag = null;
  }
  const clamp01 = (v: number) => Math.max(0, Math.min(1, v));
  function pick(l: Layer, e: PointerEvent) {
    if (e.button !== 0) return;
    studio.select({ type: "layer", id: l.uid });
    start(e, "move");
  }
  function pickKeys(e: PointerEvent) {
    if (e.button !== 0) return;
    studio.select({ type: "keys" });
    start(e, "keys");
  }
  function frameDown(e: PointerEvent) {
    // Clic dans le vide : on choisit le morceau sous la tête de lecture
    if (e.target === e.currentTarget || (e.target as HTMLElement).tagName === "VIDEO") {
      const p = P.pieces[studio.indexAt(studio.t)];
      studio.select(p ? { type: "piece", id: p.uid } : null);
    }
  }
</script>

<svelte:window onpointermove={move} onpointerup={end} />

<div class="preview">
  <div
    class="stage"
    class:over
    bind:this={stage}
    ondragover={(e) => { if (e.dataTransfer?.types.includes("application/x-rewind-clip")) { e.preventDefault(); over = true; } }}
    ondragleave={() => (over = false)}
    ondrop={drop}
    role="region"
    aria-label={t("Aperçu")}
  >
    <div class="frame" class:hidden={empty} style="--r:{ratio}" bind:this={frameEl} bind:clientWidth={fw} bind:clientHeight={fh} onpointerdown={frameDown} role="presentation">
      {#if P.fit === "blur" && P.format !== "16:9"}<canvas class="blurbg" width="54" height="96" bind:this={blurCanvas}></canvas>{/if}
      <div class="zoombox" class:cover={P.fit === "crop" && P.format !== "16:9"} bind:this={zoomBox}>
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={vA} playsinline preload="auto"></video>
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={vB} playsinline preload="auto"></video>
        <div class="flash" bind:this={flash}></div>
      </div>
      {#each visible as v (v.l.uid)}
        <div class="lz" style="z-index:{v.z}">
          <LayerView layer={v.l} local={studio.t - v.l.at} {fw} {fh} onpick={(e) => pick(v.l, e)} />
        </div>
      {/each}
      {#if P.keys.on && hasKeys && keyEvents.length}
        <div class="keysbox" class:on={studio.sel?.type === "keys"} style="left:{P.keys.x * fw}px;top:{P.keys.y * fh}px" onpointerdown={pickKeys} role="presentation">
          <KeysOverlay events={keyEvents} time={keyTime} width={P.keys.scale * fw} floating={false} />
          {#if studio.sel?.type === "keys"}<span class="h br" onpointerdown={(e) => start(e, "keysScale")} role="presentation"></span>{/if}
        </div>
      {/if}
      {#if guides.v}<i class="guide v"></i>{/if}
      {#if guides.h}<i class="guide h"></i>{/if}
      <!-- Cadre de sélection du calque choisi -->
      {#if sel && selState && selActive}
        <div class="selbox" style="left:{selState.x * fw}px;top:{selState.y * fh}px;width:{selBox.w}px;height:{selBox.h}px;transform:translate(-50%,-50%) rotate({selState.rot}deg)">
          <span class="h tl" onpointerdown={(e) => start(e, "scale")} role="presentation"></span>
          <span class="h tr" onpointerdown={(e) => start(e, "scale")} role="presentation"></span>
          <span class="h bl" onpointerdown={(e) => start(e, "scale")} role="presentation"></span>
          <span class="h br" onpointerdown={(e) => start(e, "scale")} role="presentation"></span>
          {#if sel.kind === "blur"}
            <span class="h mr" onpointerdown={(e) => start(e, "w")} role="presentation"></span>
            <span class="h bm" onpointerdown={(e) => start(e, "h")} role="presentation"></span>
          {:else}
            <span class="rot" onpointerdown={(e) => start(e, "rot")} role="presentation" title={t("Tourner (Maj : par 15°)")}><RotateCw size={12} /></span>
          {/if}
        </div>
      {/if}
    </div>
    {#if empty}
      <div class="empty">
        <Film size={36} />
        <h3>{t("Ton montage commence ici")}</h3>
        <p>{t("Ajoute des clips depuis la colonne de gauche ou glisse-les ici. Ensuite : texte, images, musique, zoom, ralenti…")}</p>
        {#if app.clips.some((c) => c.kind !== "screenshot")}
          <button class="btn primary" onclick={quickStart}><Sparkles size={15} />{t("Commencer avec mes 3 derniers clips")}</button>
        {/if}
      </div>
    {/if}
  </div>

  <div class="transport">
    <div class="now">{#if current}<span class="cur">{current.title}</span>{/if}</div>
    <div class="ctrl">
      <button class="btn icon ghost" onclick={() => studio.jumpCut(-1)} disabled={empty} title={t("Coupe précédente (↑)")}><SkipBack size={17} /></button>
      <button class="playbtn" onclick={toggle} disabled={empty} title={t("Lecture / pause (Espace)")}>
        {#if studio.playing}<Pause size={20} fill="currentColor" />{:else}<Play size={20} fill="currentColor" />{/if}
      </button>
      <button class="btn icon ghost" onclick={() => studio.jumpCut(1)} disabled={empty} title={t("Coupe suivante (↓)")}><SkipForward size={17} /></button>
    </div>
    <div class="right">
      <span class="mono time">{fmtTime(studio.t, true)}<span class="faint"> / {fmtTime(studio.total, true)}</span></span>
      <button class="btn icon ghost" onclick={fullscreen} disabled={empty} title={t("Plein écran")}><Maximize size={16} /></button>
    </div>
  </div>
</div>

<style>
  .preview { display: flex; flex-direction: column; min-width: 0; min-height: 0; padding: 12px 14px 8px; gap: 8px; }
  .stage { position: relative; flex: 1; min-height: 0; display: grid; place-items: center; border-radius: 14px; background: #08090c; container-type: size; overflow: hidden; transition: box-shadow var(--dur); }
  .stage.over { box-shadow: inset 0 0 0 2px var(--hi); }
  .stage:fullscreen { border-radius: 0; background: #000; }
  .frame { position: relative; width: min(100cqw, calc(100cqh * var(--r))); aspect-ratio: var(--r); background: #000; overflow: hidden; border-radius: 4px; box-shadow: 0 24px 60px -30px rgba(0, 0, 0, 0.9); touch-action: none; }
  .frame.hidden { visibility: hidden; }
  .zoombox { position: absolute; inset: 0; will-change: transform; }
  .zoombox video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; opacity: 0; }
  .zoombox.cover video { object-fit: cover; }
  .blurbg { position: absolute; inset: -6%; width: 112%; height: 112%; filter: blur(14px) brightness(0.8); }
  .flash { position: absolute; inset: 0; background: #fff; opacity: 0; pointer-events: none; z-index: 5; }
  .lz { position: absolute; inset: 0; pointer-events: none; }
  .keysbox { position: absolute; z-index: 60; transform: translate(-50%, -50%); cursor: move; border-radius: 10px; }
  .keysbox.on { outline: 1.5px solid #fff; outline-offset: 3px; }
  .guide { position: absolute; z-index: 79; pointer-events: none; background: #ff4fd8; }
  .guide.v { left: 50%; top: 0; bottom: 0; width: 1px; }
  .guide.h { top: 50%; left: 0; right: 0; height: 1px; }
  .selbox { position: absolute; z-index: 80; pointer-events: none; outline: 1.5px solid #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5); }
  .h { position: absolute; width: 11px; height: 11px; margin: -6px 0 0 -6px; border-radius: 3px; background: #fff; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.55); pointer-events: auto; }
  .h.tl { left: 0; top: 0; cursor: nwse-resize; }
  .h.tr { left: 100%; top: 0; cursor: nesw-resize; }
  .h.bl { left: 0; top: 100%; cursor: nesw-resize; }
  .h.br { left: 100%; top: 100%; cursor: nwse-resize; }
  .h.mr { left: 100%; top: 50%; cursor: ew-resize; }
  .h.bm { left: 50%; top: 100%; cursor: ns-resize; }
  .keysbox .h.br { position: absolute; }
  .rot { position: absolute; left: 50%; top: -30px; width: 22px; height: 22px; margin-left: -11px; border-radius: 50%; display: grid; place-items: center; background: #fff; color: #0b0c0f; box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.55); pointer-events: auto; cursor: grab; }
  .empty { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: 10px; padding: 24px; color: #fff; }
  .empty h3 { margin: 4px 0 0; font-size: 20px; font-weight: 600; letter-spacing: -0.02em; }
  .empty p { margin: 0 0 6px; max-width: 460px; font-size: 13.5px; line-height: 1.6; color: rgba(255, 255, 255, 0.7); }
  .transport { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 10px; }
  .now { min-width: 0; font-size: 12.5px; color: var(--text-2); }
  .cur { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ctrl { display: flex; align-items: center; gap: 6px; }
  .playbtn { width: 44px; height: 44px; border-radius: 50%; display: grid; place-items: center; background: var(--hi); color: var(--hi-ink); transition: transform 120ms var(--ease); }
  .playbtn:hover:not(:disabled) { transform: scale(1.06); }
  .playbtn:active:not(:disabled) { transform: scale(0.95); }
  .playbtn:disabled { opacity: 0.4; }
  .right { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .time { font-size: 13px; }
</style>
