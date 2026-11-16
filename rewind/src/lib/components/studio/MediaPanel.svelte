<script lang="ts">
  // Colonne de gauche du Studio : tout ce qu'on peut ajouter au montage.
  import {
    Clapperboard, Type, Image as ImageIcon, Music, Sparkles, Plus, Search, Layers, Mic, Square, Upload, Droplets, Snowflake, ZoomIn, Gauge,
    Vibrate, Zap, ScanLine, Contrast, SunMoon, Moon, Aperture, RectangleHorizontal, Grid3x3,
  } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { studio, fmtTime } from "$lib/studio/store.svelte";
  import { FX_LIST, type FxKind } from "$lib/studio/model";
  import { fileUrl, formatDuration, api, gameLabel } from "$lib/api";
  import { fit } from "$lib/fit";
  import { t } from "$lib/i18n.svelte";

  type Tab = "clips" | "text" | "media" | "audio" | "fx";
  let tab = $state<Tab>("clips");
  const TABS: { id: Tab; icon: typeof Clapperboard; label: string }[] = [
    { id: "clips", icon: Clapperboard, label: "Clips" },
    { id: "text", icon: Type, label: "Texte" },
    { id: "media", icon: ImageIcon, label: "Médias" },
    { id: "audio", icon: Music, label: "Son" },
    { id: "fx", icon: Sparkles, label: "Effets" },
  ];
  let q = $state("");
  let game = $state("");

  const clips = $derived(app.ownClips.filter((c) => c.kind !== "screenshot" && c.duration > 0.3));
  const games = $derived([...new Set(clips.map((c) => gameLabel(c.game)))]);
  const shown = $derived(clips.filter((c) => (!game || gameLabel(c.game) === game) && (!q || c.title.toLowerCase().includes(q.toLowerCase()))));

  function dragClip(e: DragEvent, id: string) {
    e.dataTransfer?.setData("application/x-rewind-clip", id);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
  }

  async function pick(kind: "media" | "music" | "sfx") {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const filters =
      kind === "media"
        ? [{ name: t("Images, GIF et vidéos"), extensions: ["png", "jpg", "jpeg", "webp", "gif", "mp4", "mkv", "mov", "webm"] }]
        : [{ name: t("Son"), extensions: ["mp3", "wav", "ogg", "m4a", "aac", "flac", "opus", "wma"] }];
    const path = await open({ multiple: false, directory: false, filters }).catch(() => null);
    if (typeof path !== "string") return;
    if (kind === "media") await studio.addMedia(path);
    else await studio.addAudio(path, kind);
    rememberSound(path, kind);
  }

  // Sons récents (musique, bruitages)
  const RECENT = "rewind.studio.sounds";
  type Recent = { path: string; name: string; kind: "music" | "sfx" };
  let recent = $state<Recent[]>(load());
  function load(): Recent[] {
    try {
      const l = JSON.parse(localStorage.getItem(RECENT) || localStorage.getItem("rewind.music.recent") || "[]");
      return Array.isArray(l) ? l.filter((x) => x && typeof x.path === "string").map((x) => ({ path: x.path, name: x.name ?? x.path.split(/[\\/]/).pop(), kind: x.kind ?? "music" })) : [];
    } catch {
      return [];
    }
  }
  function rememberSound(path: string, kind: "media" | "music" | "sfx") {
    if (kind === "media") return;
    const name = (path.split(/[\\/]/).pop() ?? "").replace(/\.[^.]+$/, "");
    recent = [{ path, name, kind }, ...recent.filter((r) => r.path !== path)].slice(0, 10);
    try { localStorage.setItem(RECENT, JSON.stringify(recent)); } catch {}
  }

  // Voix off : enregistre le micro choisi dans Rewind pendant que le montage défile
  let recTimer: ReturnType<typeof setInterval> | undefined;
  let recSecs = $state(0);
  async function toggleRecord() {
    if (studio.recording) {
      clearInterval(recTimer);
      const at = studio.recording.at;
      studio.recording = null;
      studio.playing = false;
      try {
        const r = await api.voiceoverStop();
        await studio.addAudio(r.path, "voice", at);
      } catch (e) {
        studio.say(t("Voix off impossible : {e}", { e: String(e) }));
      }
      return;
    }
    try {
      await api.voiceoverStart();
      studio.recording = { at: studio.t, since: Date.now() };
      recSecs = 0;
      recTimer = setInterval(() => (recSecs = (Date.now() - (studio.recording?.since ?? Date.now())) / 1000), 200);
      studio.playing = studio.total > 0;
    } catch (e) {
      studio.say(t("Micro indisponible : {e}", { e: String(e) }));
    }
  }

  const FX_ICON: Record<FxKind, typeof Vibrate> = {
    shake: Vibrate, flash: Zap, glitch: ScanLine, bw: Contrast, invert: SunMoon, black: Moon, vignette: Aperture, bars: RectangleHorizontal,
  };

  const TEXTS: { id: "title" | "subtitle" | "caption" | "gothic"; label: string; hint: string }[] = [
    { id: "gothic", label: "Edit", hint: "Gothique, comme sur les edits TikTok" },
    { id: "title", label: "TITRE", hint: "Gros titre avec contour, entrée « pop »" },
    { id: "subtitle", label: "Sous-titre", hint: "Texte sur un fond sombre, en bas" },
    { id: "caption", label: "Légende", hint: "Petit texte qui glisse" },
  ];
</script>

<aside class="media">
  <div class="tabs" role="tablist">
    {#each TABS as tb}
      <button role="tab" class:on={tab === tb.id} aria-selected={tab === tb.id} onclick={() => (tab = tb.id)} title={t(tb.label)}><tb.icon size={16} /><span>{t(tb.label)}</span></button>
    {/each}
  </div>

  <div class="body">
    {#if tab === "clips"}
      <label class="search"><Search size={14} /><input placeholder={t("Chercher un clip")} bind:value={q} /></label>
      {#if games.length > 1}
        <div class="chips">
          <button class="chip" class:on={!game} onclick={() => (game = "")}>{t("Tous")}</button>
          {#each games as g}<button class="chip" class:on={game === g} onclick={() => (game = g)}>{g}</button>{/each}
        </div>
      {/if}
      <div class="grid">
        {#each shown as c (c.id)}
          <div class="card" draggable="true" ondragstart={(e) => dragClip(e, c.id)} role="listitem" title={t("Glisse sur la frise, ou + pour ajouter à la fin")}>
            <div class="thumb">
              {#if c.thumb}<img use:fit src={fileUrl(c.thumb)} alt="" loading="lazy" draggable="false" />{/if}
              <span class="dur mono">{formatDuration(c.duration)}</span>
              <div class="acts">
                <button onclick={() => studio.insertClips([c])} title={t("Ajouter à la suite")} aria-label={t("Ajouter à la suite")}><Plus size={15} /></button>
                <button onclick={() => studio.addMedia(c.path)} title={t("Ajouter par-dessus (incrustation)")} aria-label={t("Ajouter par-dessus (incrustation)")}><Layers size={14} /></button>
              </div>
            </div>
            <span class="ctitle">{c.title}</span>
          </div>
        {:else}
          <p class="note">{t("Aucun clip ici pour l'instant.")}</p>
        {/each}
      </div>

    {:else if tab === "text"}
      <p class="note">{t("Le texte s'ajoute à la tête de lecture. Place-le ensuite directement sur l'aperçu.")}</p>
      {#each TEXTS as tx}
        <button class="preset {tx.id}" onclick={() => studio.addText(tx.id)}>
          <span class="sample">{t(tx.label)}</span>
          <small>{t(tx.hint)}</small>
        </button>
      {/each}

    {:else if tab === "media"}
      <button class="big" onclick={() => pick("media")}><Upload size={18} /><span><b>{t("Importer")}</b><small>{t("Image, GIF ou vidéo, posée par-dessus")}</small></span></button>
      <p class="note">{t("Astuce : dans l'onglet Clips, le bouton calques met un de tes clips par-dessus (réaction, autre angle).")}</p>

    {:else if tab === "audio"}
      <button class="big" onclick={() => pick("music")}><Music size={18} /><span><b>{t("Musique")}</b><small>{t("Démarre au début, baisse quand le jeu est fort")}</small></span></button>
      <button class="big" onclick={() => pick("sfx")}><Plus size={18} /><span><b>{t("Bruitage")}</b><small>{t("Posé à la tête de lecture")}</small></span></button>
      <button class="big rec" class:live={!!studio.recording} onclick={toggleRecord}>
        {#if studio.recording}<Square size={16} fill="currentColor" /><span><b>{t("Arrêter")} · <span class="mono">{fmtTime(recSecs)}</span></b><small>{t("La voix off sera posée à sa place")}</small></span>
        {:else}<Mic size={18} /><span><b>{t("Voix off")}</b><small>{t("Enregistre ton micro pendant la lecture")}</small></span>{/if}
      </button>
      {#if recent.length}
        <span class="mini">{t("Récents")}</span>
        {#each recent as r}
          <button class="recent" onclick={() => studio.addAudio(r.path, r.kind)} title={r.path}><Music size={13} /><span>{r.name}</span></button>
        {/each}
      {/if}

    {:else}
      <p class="note">{t("Un effet se pose à la tête de lecture, sur sa piste. Allonge-le ou déplace-le sur la frise : l'aperçu le montre tout de suite.")}</p>
      <span class="mini">{t("Sur toute l'image")}</span>
      <div class="fxgrid">
        {#each FX_LIST as f}
          {@const Icon = FX_ICON[f.fx]}
          <button class="fxb" onclick={() => studio.addFx(f.fx)} title={t(f.hint)}><Icon size={17} /><b>{t(f.label)}</b><small>{t(f.hint)}</small></button>
        {/each}
        <button class="fxb" onclick={() => studio.addBlur("blur")} title={t("Tout le cadre flou")}><Droplets size={17} /><b>{t("Image floutée")}</b><small>{t("Tout le cadre flou")}</small></button>
        <button class="fxb" onclick={() => studio.addBlur("pixel")} title={t("Tout le cadre en gros pixels")}><Grid3x3 size={17} /><b>{t("Image pixelisée")}</b><small>{t("Tout le cadre en gros pixels")}</small></button>
      </div>
      <span class="mini">{t("Sur le clip")}</span>
      <button class="big" onclick={() => studio.zoomPunch()} disabled={studio.indexAt(studio.t) < 0}><ZoomIn size={18} /><span><b>{t("Zoom rapide")}</b><small>{t("L'image plonge puis revient, à la tête de lecture")}</small></span></button>
      <button class="big" onclick={() => studio.addBlur()}><Droplets size={18} /><span><b>{t("Zone floutée")}</b><small>{t("Cache un pseudo, le chat, une notification")}</small></span></button>
      <button class="big" onclick={() => studio.freezeFrame()} disabled={studio.indexAt(studio.t) < 0}><Snowflake size={18} /><span><b>{t("Arrêt sur image")}</b><small>{t("Fige l'image à la tête de lecture pendant 2 s")}</small></span></button>
      <div class="tip"><ZoomIn size={16} /><span>{t("Zoom animé : choisis un morceau, puis « Zoom » à droite.")}</span></div>
      <div class="tip"><Gauge size={16} /><span>{t("Ralenti progressif : choisis un morceau, puis « Ralenti progressif » à droite.")}</span></div>
    {/if}
  </div>
</aside>

<style>
  .media { display: flex; min-height: 0; border-right: 1px solid var(--line); background: var(--bg-2); }
  .tabs { width: 62px; flex: none; display: flex; flex-direction: column; gap: 2px; padding: 8px 6px; border-right: 1px solid var(--line); }
  .tabs button { display: flex; flex-direction: column; align-items: center; gap: 3px; padding: 8px 0 6px; border-radius: 9px; color: var(--text-3); font-size: 10.5px; }
  .tabs button:hover { color: var(--text); }
  .tabs button.on { color: var(--text); background: var(--panel-3); }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 8px; padding: 12px; overflow-y: auto; }
  .body > :global(*) { flex-shrink: 0; }
  .search { display: flex; align-items: center; gap: 7px; height: 32px; padding: 0 10px; border-radius: 8px; background: var(--bg); border: 1px solid var(--line); color: var(--text-3); }
  .search input { flex: 1; min-width: 0; background: transparent; border: 0; outline: none; color: var(--text); font-size: 12.5px; user-select: text; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip { height: 24px; padding: 0 9px; border-radius: 7px; font-size: 11.5px; color: var(--text-2); background: var(--panel); border: 1px solid var(--line); }
  .chip.on { color: var(--text); border-color: #fff; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .card { display: flex; flex-direction: column; gap: 4px; cursor: grab; min-width: 0; }
  .thumb { position: relative; aspect-ratio: 16 / 9; border-radius: 8px; overflow: hidden; background: #0d0e12; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; }
  .dur { position: absolute; right: 4px; bottom: 4px; padding: 1px 5px; border-radius: 4px; background: rgba(0, 0, 0, 0.7); color: #fff; font-size: 10px; }
  .acts { position: absolute; left: 4px; top: 4px; display: flex; gap: 3px; opacity: 0; transition: opacity var(--dur) var(--ease); }
  .card:hover .acts { opacity: 1; }
  .acts button { width: 26px; height: 26px; border-radius: 7px; display: grid; place-items: center; background: rgba(0, 0, 0, 0.7); color: #fff; }
  .acts button:hover { background: #fff; color: #0b0c0f; }
  .ctitle { font-size: 11.5px; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .note { margin: 0; font-size: 12px; line-height: 1.5; color: var(--text-3); }
  .mini { font-size: 11px; color: var(--text-3); margin-top: 6px; }
  .preset { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 16px 10px 12px; border-radius: 12px; background: #0d0e12; border: 1px solid var(--line); transition: border-color var(--dur) var(--ease); }
  .preset:hover { border-color: var(--line-2); }
  .preset small { font-size: 11px; color: var(--text-3); }
  .sample { color: #fff; }
  .preset.gothic .sample { font: 400 34px "UnifrakturMaguntia"; text-shadow: 0 0 10px rgba(255, 255, 255, 0.35), 0 2px 6px rgba(0, 0, 0, 0.7); }
  .preset.title .sample { font: 400 30px "Anton"; letter-spacing: 0.02em; -webkit-text-stroke: 1.2px #000; paint-order: stroke fill; text-shadow: 0 2px 6px rgba(0, 0, 0, 0.6); }
  .preset.subtitle .sample { font: 900 15px "Montserrat"; padding: 4px 10px; border-radius: 5px; background: rgba(0, 0, 0, 0.55); }
  .preset.caption .sample { font: 800 14px "Inter"; text-shadow: 0 0 2px #000, 0 0 2px #000; }
  .big { display: flex; align-items: center; gap: 12px; padding: 12px; border-radius: 12px; text-align: left; background: var(--panel); border: 1px solid var(--line); color: var(--text-2); transition: border-color var(--dur) var(--ease); }
  .big:hover:not(:disabled) { border-color: var(--line-2); color: var(--text); }
  .big:disabled { opacity: 0.4; }
  .big span { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .big b { font-size: 13px; font-weight: 600; color: var(--text); }
  .big small { font-size: 11px; color: var(--text-3); }
  .big.rec.live { border-color: var(--rec); color: var(--rec); }
  .recent { display: flex; align-items: center; gap: 8px; height: 30px; padding: 0 8px; border-radius: 7px; font-size: 12px; color: var(--text-2); text-align: left; }
  .recent:hover { background: var(--panel); color: var(--text); }
  .recent span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tip { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: 10px; font-size: 12px; line-height: 1.45; color: var(--text-3); border: 1px dashed var(--line-2); }
  /* Effets : grille de deux colonnes, icône + nom + petite explication */
  .fxgrid { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .fxb { display: flex; flex-direction: column; align-items: flex-start; gap: 3px; min-width: 0; padding: 10px; border-radius: 10px; text-align: left; background: var(--panel); border: 1px solid var(--line); color: var(--text-2); transition: border-color var(--dur) var(--ease), color var(--dur) var(--ease); }
  .fxb:hover { border-color: var(--line-2); color: var(--text); }
  .fxb:active { transform: scale(0.98); }
  .fxb b { font-size: 12.5px; font-weight: 600; color: var(--text); margin-top: 3px; }
  .fxb small { font-size: 10.5px; line-height: 1.35; color: var(--text-3); display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
</style>
