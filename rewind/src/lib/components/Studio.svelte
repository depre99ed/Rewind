<script lang="ts">
  import { onMount } from "svelte";
  import { Play, Pause, SkipBack, SkipForward, Download, Maximize, Film, Monitor, Smartphone, Sparkles, CloudCheck, Keyboard } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { studio, media, clampSpeed, fmtTime, pickMusic } from "$lib/studio.svelte";
  import { fileUrl } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import MediaPanel from "./studio/MediaPanel.svelte";
  import Inspector from "./studio/Inspector.svelte";
  import Timeline from "./studio/Timeline.svelte";
  import ExportDialog from "./studio/ExportDialog.svelte";
  import KeysOverlay from "./KeysOverlay.svelte";

  let stage: HTMLDivElement | undefined = $state();
  let vA: HTMLVideoElement | undefined = $state();
  let vB: HTMLVideoElement | undefined = $state();
  let flash: HTMLDivElement | undefined = $state();
  let blurCanvas: HTMLCanvasElement | undefined = $state();
  let music: HTMLAudioElement | undefined = $state();
  let exportOpen = $state(typeof window !== "undefined" && !!(window as any).__REWIND_MOCK__ && new URLSearchParams(location.search).get("export") === "1");
  let saved = $state(true);

  const L = $derived(studio.lay);
  const current = $derived(app.studio[studio.indexAt(studio.t)] ?? null);
  const vertical = $derived(app.studioFormat !== "16:9");

  // Infos des médias (pistes, vignettes, forme d'onde) pour chaque morceau
  $effect(() => {
    for (const p of app.studio) {
      media.load(p.path);
      media.loadStrip(p.path);
      media.loadInputs(p.clipId);
    }
    if (app.studioMusic) media.load(app.studioMusic.path);
  });
  // Touches clavier/souris du morceau affiché (si enregistrées avec le clip)
  const keyIndex = $derived(studio.indexAt(studio.t));
  const keyEvents = $derived(current ? media.inputs[current.clipId] ?? [] : []);
  const hasKeys = $derived(app.studio.some((p) => (media.inputs[p.clipId]?.length ?? 0) > 0));
  // Pistes séparées découvertes : un réglage de volume par piste
  $effect(() => {
    void media.info;
    void app.studio.length;
    studio.fillTracks();
  });
  // Sauvegarde automatique du montage
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    void JSON.stringify(app.studio);
    void JSON.stringify(app.studioMusic);
    void app.studioTitle;
    void app.studioFormat;
    saved = false;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      app.saveStudio();
      saved = true;
    }, 600);
  });

  // ---------------------------------------------------------------------------
  // Lecteur : deux vidéos. Pendant que l'une joue, l'autre attend déjà au début du morceau
  // suivant (pas de trou au changement de morceau) ; pendant une transition, les deux jouent.
  // ---------------------------------------------------------------------------
  type Slot = { el: HTMLVideoElement; idx: number; path: string };
  let slots: Slot[] = [];
  let raf = 0;
  let lastNow = 0;

  function prepare(s: Slot, idx: number) {
    const p = app.studio[idx];
    if (!p) return;
    s.idx = idx;
    if (s.path !== p.path) {
      s.path = p.path;
      s.el.src = fileUrl(p.path);
    }
  }

  function sync(s: Slot, idx: number, t: number, play: boolean) {
    prepare(s, idx);
    const p = app.studio[idx];
    const el = s.el;
    const target = studio.srcTime(idx, t);
    const rate = clampSpeed(p.speed);
    if (el.playbackRate !== rate) el.playbackRate = rate;
    if (el.readyState < 1) return;
    const drift = Math.abs(el.currentTime - target);
    if (play) {
      if (drift > 0.25 && !el.seeking) el.currentTime = target;
      if (el.paused) el.play().catch(() => {});
    } else {
      if (!el.paused) el.pause();
      if (drift > 0.015 && !el.seeking) el.currentTime = target;
    }
  }

  function reset(el: HTMLVideoElement) {
    el.style.opacity = "0";
    el.style.transform = "";
    el.style.clipPath = "";
    el.style.zIndex = "1";
  }

  function frame(now: number) {
    raf = requestAnimationFrame(frame);
    const P = app.studio;
    if (!slots.length) return;
    if (!P.length) {
      for (const s of slots) if (!s.el.paused) s.el.pause();
      music?.pause();
      lastNow = now;
      return;
    }
    // Horloge du montage
    if (studio.playing) {
      const dt = lastNow ? Math.min(0.1, (now - lastNow) / 1000) : 0;
      studio.t = Math.min(L.total, studio.t + dt);
      if (studio.t >= L.total - 0.001) {
        studio.playing = false;
        studio.t = L.total - 0.001;
      }
    }
    lastNow = now;
    const t = studio.t;
    const play = studio.playing;
    const k = Math.max(0, studio.indexAt(t));
    const overlap = k > 0 && L.trans[k] > 0 && t < L.offsets[k] + L.trans[k];
    const needed = overlap ? [k - 1, k] : [k];

    // Attribution des deux lecteurs
    const used = new Set<Slot>();
    const byIdx = new Map<number, Slot>();
    for (const n of needed) {
      const s = slots.find((x) => x.idx === n && x.path === P[n].path && !used.has(x));
      if (s) {
        used.add(s);
        byIdx.set(n, s);
      }
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
      if (k + 1 < P.length) sync(spare, k + 1, L.offsets[k + 1], false); // préchargé, en pause
      else if (!spare.el.paused) spare.el.pause();
    }

    // Image : transition en cours ou morceau seul
    const main = byIdx.get(k)!;
    if (flash) flash.style.opacity = "0";
    if (!overlap) {
      reset(main.el);
      main.el.style.opacity = "1";
      main.el.muted = false;
      main.el.volume = Math.min(1, Math.max(0, P[k].volume));
    } else {
      const out = byIdx.get(k - 1)!;
      const pr = Math.min(1, Math.max(0, (t - L.offsets[k]) / L.trans[k]));
      const type = P[k - 1].transition;
      reset(out.el);
      reset(main.el);
      out.el.style.zIndex = "1";
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
      } else if (type === "wipe") {
        main.el.style.clipPath = `inset(0 0 0 ${(1 - pr) * 100}%)`;
      } else main.el.style.opacity = String(pr);
      out.el.muted = false;
      main.el.muted = false;
      out.el.volume = Math.min(1, Math.max(0, P[k - 1].volume * (1 - pr)));
      main.el.volume = Math.min(1, Math.max(0, P[k].volume * pr));
    }

    // Fond flou du format vertical
    if (app.studioFormat === "9:16-blur" && blurCanvas && main.el.readyState >= 2) {
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

    // Musique de fond
    const m = app.studioMusic;
    if (music) {
      if (m && play && t >= m.at && m.duration > 0) {
        const url = fileUrl(m.path);
        if (music.dataset.src !== url) {
          music.dataset.src = url;
          music.src = url;
        }
        const mt = (m.offset + (t - m.at)) % m.duration;
        if (music.readyState >= 1 && Math.abs(music.currentTime - mt) > 0.3) music.currentTime = mt;
        let v = m.volume;
        if (m.fadeIn > 0) v *= Math.min(1, (t - m.at) / m.fadeIn);
        if (m.fadeOut > 0) v *= Math.min(1, Math.max(0, (L.total - t) / m.fadeOut));
        music.volume = Math.min(1, Math.max(0, v));
        if (music.paused) music.play().catch(() => {});
      } else if (!music.paused) music.pause();
    }
  }

  onMount(() => {
    if (vA && vB) slots = [{ el: vA, idx: -1, path: "" }, { el: vB, idx: -1, path: "" }];
    raf = requestAnimationFrame(frame);
    return () => {
      cancelAnimationFrame(raf);
      studio.playing = false;
      for (const s of slots) s.el.pause();
      music?.pause();
      clearTimeout(saveTimer);
      app.saveStudio();
    };
  });

  function toggle() {
    if (!app.studio.length) return;
    if (!studio.playing && studio.t >= L.total - 0.05) studio.t = 0;
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

  // Clips glissés directement sur l'aperçu : ajoutés à la fin
  let stageOver = $state(false);
  function stageDrop(e: DragEvent) {
    stageOver = false;
    const ids = e.dataTransfer?.getData("application/x-rewind-clip");
    if (!ids) return;
    e.preventDefault();
    studio.insertClips(ids.split(",").map((id) => app.clips.find((c) => c.id === id)).filter((c) => !!c));
  }

  function key(e: KeyboardEvent) {
    if (app.view !== "studio" || exportOpen || app.openClip) return;
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (e.key === " ") { e.preventDefault(); toggle(); }
    else if (ctrl && k === "z") { e.preventDefault(); e.shiftKey ? studio.redo() : studio.undo(); }
    else if (ctrl && k === "y") { e.preventDefault(); studio.redo(); }
    else if (ctrl && k === "d") { e.preventDefault(); studio.duplicate(); }
    else if (!ctrl && k === "s") studio.split();
    else if (e.key === "Delete" || e.key === "Backspace") studio.remove();
    else if (e.key === "ArrowLeft") { e.preventDefault(); studio.playing = false; studio.seek(studio.t - (e.shiftKey ? 1 : 1 / 30)); }
    else if (e.key === "ArrowRight") { e.preventDefault(); studio.playing = false; studio.seek(studio.t + (e.shiftKey ? 1 : 1 / 30)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); studio.jumpCut(-1); }
    else if (e.key === "ArrowDown") { e.preventDefault(); studio.jumpCut(1); }
    else if (e.key === "Home") studio.seek(0);
    else if (e.key === "End") studio.seek(L.total);
    else if (e.key === "+" || e.key === "=") studio.zoom = Math.min(40, studio.zoom * 1.3);
    else if (e.key === "-") studio.zoom = Math.max(1, studio.zoom / 1.3);
    else if (e.key === "Escape") { studio.junction = null; studio.select(null); }
  }
</script>

<svelte:window onkeydown={key} />

<section class="studio">
  <!-- Barre du haut -->
  <header class="top">
    <div class="name">
      <Sparkles size={16} />
      <input class="title" bind:value={app.studioTitle} placeholder={t("Nom du montage")} aria-label={t("Nom du montage")} />
      <span class="save faint" title={t("Le montage est gardé même si tu fermes Rewind")}>
        {#if saved}<CloudCheck size={14} />{t("Enregistré")}{:else}{t("Enregistrement…")}{/if}
      </span>
    </div>
    <div class="seg" role="group" aria-label={t("Format")}>
      <button class:on={app.studioFormat === "16:9"} onclick={() => (app.studioFormat = "16:9")}><Monitor size={14} />16:9</button>
      <button class:on={app.studioFormat === "9:16-blur"} onclick={() => (app.studioFormat = "9:16-blur")}><Smartphone size={14} />9:16 {t("flou")}</button>
      <button class:on={app.studioFormat === "9:16-crop"} onclick={() => (app.studioFormat = "9:16-crop")}><Smartphone size={14} />9:16 {t("recadré")}</button>
    </div>
    <button class="btn primary exp" onclick={() => (exportOpen = true)} disabled={!app.studio.length}><Download size={16} />{t("Exporter")}</button>
  </header>

  <div class="work">
    <MediaPanel />

    <div class="center">
      <div
        class="stage"
        class:over={stageOver}
        bind:this={stage}
        ondragover={(e) => { if (e.dataTransfer?.types.includes("application/x-rewind-clip")) { e.preventDefault(); stageOver = true; } }}
        ondragleave={() => (stageOver = false)}
        ondrop={stageDrop}
        role="region"
        aria-label={t("Aperçu")}
      >
        <div class="frame" class:vertical class:crop={app.studioFormat === "9:16-crop"} class:hidden={!app.studio.length}>
          {#if app.studioFormat === "9:16-blur"}<canvas class="blurbg" width="54" height="96" bind:this={blurCanvas}></canvas>{/if}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video bind:this={vA} playsinline preload="auto" onclick={toggle}></video>
          <!-- svelte-ignore a11y_media_has_caption -->
          <video bind:this={vB} playsinline preload="auto" onclick={toggle}></video>
          <div class="flash" bind:this={flash}></div>
          {#if app.studioKeys && keyEvents.length && keyIndex >= 0}
            <KeysOverlay events={keyEvents} time={studio.srcTime(keyIndex, studio.t)} compact />
          {/if}
        </div>
        {#if !app.studio.length}
          <div class="empty">
            <Film size={36} />
            <h3>{t("Ton montage commence ici")}</h3>
            <p>{t("Glisse des clips sur la frise ou sur cet écran, ou clique sur + dans la liste.")}<br />{t("Coupe avec S, ralentis un one-tap, ajoute une musique et des transitions, puis exporte pour TikTok, YouTube ou Discord.")}</p>
            {#if app.clips.some((c) => c.kind !== "screenshot")}
              <button class="btn primary" onclick={quickStart}><Sparkles size={15} />{t("Commencer avec mes 3 derniers clips")}</button>
            {/if}
          </div>
        {/if}
      </div>

      <div class="transport">
        <div class="now">
          {#if current}<span class="dot" style="background:{current.game ? 'var(--hi)' : 'var(--text-3)'}"></span><span class="cur">{current.title}</span>{/if}
        </div>
        <div class="ctrl">
          <button class="btn icon ghost" onclick={() => studio.jumpCut(-1)} disabled={!app.studio.length} title={t("Coupe précédente (↑)")}><SkipBack size={17} /></button>
          <button class="playbtn" onclick={toggle} disabled={!app.studio.length} title={t("Lecture / pause (Espace)")}>
            {#if studio.playing}<Pause size={20} fill="currentColor" />{:else}<Play size={20} fill="currentColor" />{/if}
          </button>
          <button class="btn icon ghost" onclick={() => studio.jumpCut(1)} disabled={!app.studio.length} title={t("Coupe suivante (↓)")}><SkipForward size={17} /></button>
        </div>
        <div class="right">
          {#if hasKeys}
            <button class="btn small ghost" class:on={app.studioKeys} onclick={() => (app.studioKeys = !app.studioKeys)} title={t("Touches clavier / souris : affichées ici et incrustées à l'export")}><Keyboard size={15} />{t("Touches")}</button>
          {/if}
          <span class="mono time">{fmtTime(studio.t, true)}<span class="faint"> / {fmtTime(L.total, true)}</span></span>
          <button class="btn icon ghost" onclick={fullscreen} disabled={!app.studio.length} title={t("Plein écran")}><Maximize size={16} /></button>
        </div>
      </div>
    </div>

    <Inspector />
  </div>

  <div class="bottom">
    <Timeline onAddMusic={pickMusic} />
  </div>

  <audio bind:this={music} preload="auto"></audio>
</section>

{#if exportOpen}
  <ExportDialog onClose={() => (exportOpen = false)} />
{/if}

<style>
  .studio { height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr) 300px; grid-template-columns: minmax(0, 1fr); background: var(--bg); }
  .studio > * { min-width: 0; }
  .top { display: flex; align-items: center; gap: 12px; padding: 10px 14px; border-bottom: 1px solid var(--line); background: var(--bg-2); }
  .name { display: flex; align-items: center; gap: 8px; flex: 1; min-width: 0; color: var(--hi); }
  .title { flex: 0 1 320px; min-width: 80px; height: 32px; padding: 0 10px; border-radius: 8px; border: 1px solid transparent; background: transparent; outline: none; font-weight: 650; font-size: 14px; color: var(--text); user-select: text; transition: all var(--dur); }
  .title:hover { border-color: var(--line); }
  .title:focus { border-color: var(--hi); background: var(--bg); }
  .save { display: inline-flex; align-items: center; gap: 5px; font-size: 11.5px; white-space: nowrap; }
  .seg { display: flex; padding: 3px; border-radius: 10px; background: var(--bg); border: 1px solid var(--line); gap: 2px; }
  .seg button { height: 28px; padding: 0 10px; border-radius: 7px; font-size: 12.5px; color: var(--text-2); display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
  .seg button.on { background: var(--panel-3); color: var(--text); }
  .exp { height: 36px; padding: 0 18px; }

  .work { display: grid; grid-template-columns: 290px minmax(0, 1fr) 290px; min-height: 0; }
  @media (max-width: 1280px) { .work { grid-template-columns: 250px minmax(0, 1fr) 260px; } }
  .center { display: flex; flex-direction: column; min-width: 0; min-height: 0; padding: 12px 14px 8px; gap: 8px; }
  .stage { position: relative; flex: 1; min-height: 0; display: grid; place-items: center; border-radius: 14px; background: radial-gradient(ellipse at center, #15171d, #090a0d); container-type: size; overflow: hidden; transition: box-shadow var(--dur); }
  .stage.over { box-shadow: inset 0 0 0 2px var(--hi); }
  .stage:fullscreen { border-radius: 0; background: #000; }
  .frame { position: relative; width: min(100cqw, calc(100cqh * 16 / 9)); aspect-ratio: 16 / 9; background: #000; overflow: hidden; border-radius: 6px; box-shadow: 0 24px 60px -30px rgba(0,0,0,.9); }
  .frame.vertical { width: min(100cqw, calc(100cqh * 9 / 16)); aspect-ratio: 9 / 16; }
  .frame.hidden { visibility: hidden; }
  .frame video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; opacity: 0; will-change: opacity, transform; cursor: pointer; }
  .frame.crop video { object-fit: cover; }
  .blurbg { position: absolute; inset: -6%; width: 112%; height: 112%; filter: blur(14px) brightness(.8); }
  .flash { position: absolute; inset: 0; background: #fff; opacity: 0; pointer-events: none; z-index: 5; }
  .empty { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; gap: 10px; padding: 24px; color: #fff; }
  .empty :global(svg) { color: var(--hi); }
  .empty h3 { margin: 4px 0 0; font-family: var(--display); font-size: 21px; letter-spacing: -0.02em; }
  .empty p { margin: 0 0 6px; max-width: 480px; font-size: 13.5px; line-height: 1.6; color: rgba(255,255,255,.72); }
  .transport { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 10px; }
  .now { display: flex; align-items: center; gap: 8px; min-width: 0; font-size: 12.5px; color: var(--text-2); }
  .dot { display: none; }
  .cur { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ctrl { display: flex; align-items: center; gap: 6px; }
  .playbtn { width: 46px; height: 46px; border-radius: 50%; display: grid; place-items: center; background: var(--hi); color: var(--hi-ink); box-shadow: 0 8px 24px -10px var(--hi); transition: transform 120ms var(--ease); }
  .playbtn:hover:not(:disabled) { transform: scale(1.06); }
  .playbtn:active:not(:disabled) { transform: scale(.95); }
  .playbtn:disabled { opacity: .4; }
  .right { display: flex; align-items: center; justify-content: flex-end; gap: 6px; }
  .time { font-size: 13px; }
  .right :global(.btn.on) { color: var(--hi); }
  .bottom { min-height: 0; }
</style>
