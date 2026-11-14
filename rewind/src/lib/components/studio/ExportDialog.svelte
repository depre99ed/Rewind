<script lang="ts">
  import { Smartphone, MonitorPlay, MessageCircle, SlidersHorizontal, Download, X, Check, FolderOpen, Play, Clipboard, TriangleAlert } from "@lucide/svelte";
  import { app, type StudioFormat } from "$lib/state.svelte";
  import { studio, media, clampSpeed, fmtTime } from "$lib/studio.svelte";
  import { fit } from "$lib/fit";
  import { api, fileUrl, formatSize, type Clip } from "$lib/api";
  import { t, lang } from "$lib/i18n.svelte";

  let { onClose }: { onClose: () => void } = $props();

  type Preset = { id: string; label: string; desc: string; format: StudioFormat; height: number; fps: number; maxMb: number };
  const PRESETS: Preset[] = [
    { id: "tiktok", label: "TikTok · Shorts · Reels", desc: "Vertical 1080×1920, fluide", format: "9:16-blur", height: 1080, fps: 0, maxMb: 0 },
    { id: "youtube", label: "YouTube", desc: "16:9 en 1080p, qualité maximale", format: "16:9", height: 1080, fps: 0, maxMb: 0 },
    { id: "discord10", label: "Discord 10 Mo", desc: "Compte gratuit : passe partout", format: "16:9", height: 1080, fps: 0, maxMb: 10 },
    { id: "discord20", label: "Discord 20 Mo", desc: "Plus net, si ton serveur l'accepte", format: "16:9", height: 1080, fps: 0, maxMb: 20 },
    { id: "custom", label: "Personnalisé", desc: "Tu choisis tout", format: "16:9", height: 1080, fps: 0, maxMb: 0 },
  ];

  let presetId = $state(app.studioFormat !== "16:9" ? "tiktok" : "youtube");
  let title = $state(app.studioTitle || "Montage");
  let format = $state<StudioFormat>(app.studioFormat);
  let height = $state(1080);
  let fps = $state(0);
  let maxMb = $state(0);
  let busy = $state(false);
  let error = $state("");
  let done = $state<Clip | null>(null);
  let started = 0;
  let elapsed = $state(0);

  function choose(p: Preset) {
    presetId = p.id;
    if (p.id === "custom") return;
    // Le vertical garde le choix flou / recadré de l'aperçu
    format = p.format.startsWith("9:16") && app.studioFormat.startsWith("9:16") ? app.studioFormat : p.format;
    height = p.height;
    fps = p.fps;
    maxMb = p.maxMb;
  }
  choose(PRESETS.find((p) => p.id === presetId)!);

  const total = $derived(studio.lay.total);
  const hasKeys = $derived(app.studio.some((p) => (media.inputs[p.clipId]?.length ?? 0) > 0));
  /** Même règle que le moteur (editor.rs, size_plan). */
  const plan = $derived.by(() => {
    if (maxMb <= 0) return null;
    const v = (maxMb * 8192 * 0.92) / Math.max(total, 0.5) - 128;
    const ladder: [number, number][] = [[1080, 4500], [720, 2000], [480, 850]];
    const hit = ladder.find(([h, min]) => h <= Math.max(height, 480) && v >= min);
    if (!hit) return { ok: false as const, maxSecs: Math.floor((maxMb * 8192 * 0.92) / (850 + 128)) };
    return { ok: true as const, height: hit[0], fps: v < 3500 ? 30 : fps || 60 };
  });
  const vertical = $derived(format.startsWith("9:16"));
  const outRes = $derived.by(() => {
    const h = plan?.ok ? plan.height : height;
    const long = Math.floor((h * 16) / 9 / 2) * 2;
    return vertical ? `${h}×${long}` : `${long}×${h}`;
  });

  async function run() {
    if (!app.studio.length || busy) return;
    if (plan && !plan.ok) return;
    busy = true;
    error = "";
    started = Date.now();
    const timer = setInterval(() => (elapsed = (Date.now() - started) / 1000), 250);
    try {
      const pieces = app.studio.map((p) => ({
        path: p.path,
        start: p.start,
        end: p.end,
        speed: clampSpeed(p.speed),
        volume: p.volume,
        track_volumes: media.separateTracks(p.path).length ? p.trackVolumes : [],
        transition: p.transition,
        transition_dur: p.transitionDur,
        clip_id: p.clipId,
      }));
      const m = app.studioMusic;
      const games = new Map<string, number>();
      for (const p of app.studio) games.set(p.game, (games.get(p.game) ?? 0) + (p.end - p.start));
      const game = [...games.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "";
      const clip = await api.studioExport(
        pieces,
        {
          title: title.trim() || "Montage",
          format,
          height,
          fps,
          max_mb: maxMb,
          music: m ? { path: m.path, volume: m.volume, offset: m.offset, at: m.at, fade_in: m.fadeIn, fade_out: m.fadeOut, duck: m.duck } : null,
          keys: hasKeys && app.studioKeys,
          lang: lang(),
        },
        game,
      );
      app.studioTitle = title.trim() || "Montage";
      app.clips = [clip, ...app.clips.filter((c) => c.id !== clip.id)];
      done = clip;
    } catch (e) {
      error = String(e);
    }
    clearInterval(timer);
    busy = false;
  }

  const progress = $derived(app.progress?.op === "studio" ? app.progress.value : busy ? 0.01 : 0);
  const eta = $derived(progress > 0.05 && busy ? Math.max(0, (elapsed / progress) * (1 - progress)) : null);
</script>

<div class="overlay" role="presentation" onpointerdown={(e) => { if (e.target === e.currentTarget && !busy) onClose(); }}>
  <div class="dialog appear" role="dialog" aria-modal="true" aria-label={t("Exporter le montage")}>
    <header>
      <h2>{done ? t("Montage prêt") : t("Exporter le montage")}</h2>
      <span class="faint mono">{fmtTime(total, true)} · {t(app.studio.length > 1 ? "{n} morceaux" : "{n} morceau", { n: app.studio.length })}</span>
      <button class="x" onclick={onClose} disabled={busy} aria-label={t("Fermer")}><X size={18} /></button>
    </header>

    {#if done}
      <div class="done">
        <div class="dthumb">{#if done.thumb}<img use:fit src={fileUrl(done.thumb)} alt="" />{/if}<span class="ok"><Check size={22} /></span></div>
        <div class="dinfo">
          <strong>{done.title}</strong>
          <span class="faint">{fmtTime(done.duration, true)} · {formatSize(done.size)} · {t("exporté en {n} s", { n: elapsed.toFixed(0) })}</span>
          <div class="dact">
            <button class="btn primary small" onclick={() => { app.openClip = done; onClose(); }}><Play size={14} />{t("Voir le clip")}</button>
            <button class="btn small" onclick={() => done && app.copyForDiscord(done)} disabled={!!app.discordBusy}><Clipboard size={14} />{app.discordBusy ? t("Préparation…") : t("Copier pour Discord")}</button>
            <button class="btn small" onclick={() => done && api.reveal(done.id)}><FolderOpen size={14} />{t("Afficher le fichier")}</button>
          </div>
        </div>
      </div>
      <footer><span class="faint">{t("Les clips d'origine n'ont pas été modifiés.")}</span><button class="btn" onclick={onClose}>{t("Fermer")}</button></footer>
    {:else}
      <div class="presets">
        {#each PRESETS as p}
          <button class="preset" class:on={presetId === p.id} onclick={() => choose(p)} disabled={busy}>
            <span class="pic">
              {#if p.id === "tiktok"}<Smartphone size={20} />{:else if p.id === "youtube"}<MonitorPlay size={20} />{:else if p.id.startsWith("discord")}<MessageCircle size={20} />{:else}<SlidersHorizontal size={20} />{/if}
            </span>
            <b>{t(p.label)}</b>
            <small>{t(p.desc)}</small>
          </button>
        {/each}
      </div>

      <div class="opts">
        <label class="fld wide"><span>{t("Titre")}</span><input class="field" bind:value={title} disabled={busy} /></label>
        {#if presetId === "tiktok" || presetId === "custom"}
          <div class="fld">
            <span>{t("Cadrage")}</span>
            <div class="seg">
              {#if presetId === "custom"}<button class:on={format === "16:9"} onclick={() => (format = "16:9")}>16:9</button>{/if}
              <button class:on={format === "9:16-blur"} onclick={() => (format = "9:16-blur")}>{t("Vertical flou")}</button>
              <button class:on={format === "9:16-crop"} onclick={() => (format = "9:16-crop")}>{t("Vertical recadré")}</button>
            </div>
          </div>
        {/if}
        {#if presetId === "custom"}
          <div class="fld">
            <span>{t("Qualité")}</span>
            <div class="seg"><button class:on={height === 1080} onclick={() => (height = 1080)}>1080p</button><button class:on={height === 720} onclick={() => (height = 720)}>720p</button></div>
          </div>
          <div class="fld">
            <span>{t("Images / s")}</span>
            <div class="seg"><button class:on={fps === 0} onclick={() => (fps = 0)}>{t("Source")}</button><button class:on={fps === 60} onclick={() => (fps = 60)}>60</button><button class:on={fps === 30} onclick={() => (fps = 30)}>30</button></div>
          </div>
          <div class="fld">
            <span>{t("Taille max")}</span>
            <div class="seg"><button class:on={maxMb === 0} onclick={() => (maxMb = 0)}>{t("Aucune")}</button><button class:on={maxMb === 10} onclick={() => (maxMb = 10)}>10 Mo</button><button class:on={maxMb === 25} onclick={() => (maxMb = 25)}>25 Mo</button><button class:on={maxMb === 50} onclick={() => (maxMb = 50)}>50 Mo</button></div>
          </div>
        {/if}
      </div>

      {#if hasKeys}
        <label class="keysopt">
          <span><b>{t("Touches clavier / souris")}</b><small class="faint">{t("Incrustées en bas à gauche, comme dans l'aperçu")}</small></span>
          <span class="switch"><input type="checkbox" bind:checked={app.studioKeys} disabled={busy} /><span></span></span>
        </label>
      {/if}
      <div class="summary">
        <div><span class="faint">{t("Sortie")}</span><b class="mono">{outRes}</b></div>
        <div><span class="faint">{t("Images / s")}</span><b class="mono">{plan?.ok ? plan.fps : fps || t("source")}</b></div>
        <div><span class="faint">{t("Taille")}</span><b class="mono">{maxMb ? `≤ ${maxMb} ${t("Mo")}` : t("libre")}</b></div>
        <div><span class="faint">{t("Musique")}</span><b>{app.studioMusic ? t("oui") : t("non")}</b></div>
      </div>
      {#if plan && !plan.ok}
        <p class="warn"><TriangleAlert size={15} />{t("Trop long pour {mb} Mo avec une image correcte : raccourcis le montage à {s} s maximum, ou choisis une taille plus grande.", { mb: maxMb, s: plan.maxSecs })}</p>
      {:else if plan?.ok && plan.height < 1080}
        <p class="info faint">{t("Pour tenir dans {mb} Mo, l'export passe en {h}p{fps} (image plus nette qu'un 1080p trop compressé).", { mb: maxMb, h: plan.height, fps: plan.fps <= 30 ? t(" à 30 i/s") : "" })}</p>
      {/if}
      {#if error}<p class="warn"><TriangleAlert size={15} />{error}</p>{/if}

      {#if busy}
        <div class="prog">
          <div class="bar"><i style="transform:scaleX({Math.max(0.02, progress)})"></i></div>
          <span class="mono">{Math.round(progress * 100)} %{eta !== null ? ` · ${t("encore ~{n} s", { n: Math.ceil(eta) })}` : ""}</span>
        </div>
      {/if}
      <footer>
        <span class="faint">{t("Un nouveau fichier est créé ; tes clips d'origine ne bougent pas.")}</span>
        <button class="btn primary go" onclick={run} disabled={busy || !app.studio.length || (plan !== null && !plan.ok)}>
          <Download size={16} />{busy ? t("Export en cours…") : t("Exporter")}
        </button>
      </footer>
    {/if}
  </div>
</div>

<style>
  .overlay { position: fixed; inset: 0; z-index: 200; display: grid; place-items: center; background: var(--overlay); backdrop-filter: blur(6px); animation: fade-in 180ms var(--ease) both; }
  .dialog { width: min(720px, calc(100vw - 48px)); max-height: calc(100vh - 80px); overflow-y: auto; border-radius: 20px; background: var(--panel); border: 1px solid var(--line-2); box-shadow: var(--shadow); padding: 20px; display: flex; flex-direction: column; gap: 16px; }
  header { display: flex; align-items: baseline; gap: 12px; }
  h2 { margin: 0; font-family: var(--display); font-size: 19px; letter-spacing: -0.02em; }
  .x { margin-left: auto; width: 32px; height: 32px; border-radius: 9px; display: grid; place-items: center; color: var(--text-3); align-self: center; }
  .x:hover:not(:disabled) { background: var(--panel-3); color: var(--text); }
  .presets { display: grid; grid-template-columns: repeat(5, 1fr); gap: 8px; }
  .preset { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; padding: 12px 10px; border-radius: 14px; border: 1px solid var(--line); text-align: left; transition: all var(--dur); min-width: 0; }
  .preset:hover:not(:disabled) { border-color: var(--line-2); background: var(--panel-2); }
  .preset.on { border-color: var(--hi); background: color-mix(in srgb, var(--hi) 9%, transparent); }
  .pic { width: 36px; height: 36px; border-radius: 10px; display: grid; place-items: center; background: var(--panel-3); color: var(--text-2); margin-bottom: 4px; }
  .preset.on .pic { background: var(--hi); color: var(--hi-ink); }
  .preset b { font-size: 12.5px; line-height: 1.25; }
  .preset small { font-size: 11px; color: var(--text-3); line-height: 1.3; }
  .opts { display: flex; flex-wrap: wrap; gap: 12px; }
  .fld { display: flex; flex-direction: column; gap: 6px; font-size: 12px; color: var(--text-2); }
  .fld.wide { flex: 1 1 100%; }
  .fld .field { width: 100%; }
  .seg { display: flex; padding: 3px; border-radius: 10px; background: var(--bg-2); border: 1px solid var(--line); gap: 2px; }
  .seg button { height: 28px; padding: 0 10px; border-radius: 7px; font-size: 12.5px; color: var(--text-2); white-space: nowrap; }
  .seg button.on { background: var(--panel-3); color: var(--text); }
  .keysopt { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 12px; border-radius: 12px; background: var(--bg-2); border: 1px solid var(--line); }
  .keysopt > span:first-child { display: flex; flex-direction: column; font-size: 13px; }
  .keysopt small { font-size: 11.5px; }
  .summary { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; }
  .summary div { display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border-radius: 12px; background: var(--bg-2); border: 1px solid var(--line); font-size: 11.5px; }
  .summary b { font-size: 14px; }
  .warn { display: flex; align-items: flex-start; gap: 8px; margin: 0; padding: 10px 12px; border-radius: 10px; font-size: 12.5px; color: var(--rec); background: color-mix(in srgb, var(--rec) 10%, transparent); }
  .info { margin: 0; font-size: 12.5px; }
  .prog { display: flex; align-items: center; gap: 12px; font-size: 12.5px; }
  .bar { flex: 1; height: 8px; border-radius: 8px; background: var(--panel-3); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--hi); transform-origin: left; transition: transform 250ms linear; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: 12px; font-size: 12px; }
  .go { height: 42px; padding: 0 22px; font-size: 14px; }
  .done { display: flex; gap: 16px; align-items: center; }
  .dthumb { position: relative; width: 220px; aspect-ratio: 16/9; border-radius: 12px; overflow: hidden; background: var(--panel-3); flex: none; }
  .dthumb img { width: 100%; height: 100%; object-fit: cover; }
  .ok { position: absolute; right: 8px; bottom: 8px; width: 38px; height: 38px; border-radius: 50%; display: grid; place-items: center; background: var(--hi); color: var(--hi-ink); animation: rise 300ms var(--ease-out) both; }
  .dinfo { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .dinfo strong { font-size: 15px; }
  .dinfo span { font-size: 12.5px; }
  .dact { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
</style>
