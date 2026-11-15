<script lang="ts">
  import { Smartphone, MonitorPlay, MessageCircle, SlidersHorizontal, Download, X, Check, FolderOpen, Play, Clipboard, TriangleAlert } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { studio, media, fmtTime } from "$lib/studio/store.svelte";
  import { compile, mainGame } from "$lib/studio/export";
  import { frameSize } from "$lib/studio/model";
  import { fit } from "$lib/fit";
  import { api, fileUrl, formatSize, type Clip } from "$lib/api";
  import { t } from "$lib/i18n.svelte";

  let { onClose }: { onClose: () => void } = $props();

  const P = $derived(studio.project);
  type Preset = { id: string; label: string; desc: string; vertical: boolean | null; height: number; fps: number; maxMb: number };
  const PRESETS: Preset[] = [
    { id: "tiktok", label: "TikTok · Shorts · Reels", desc: "Vertical 1080×1920, fluide", vertical: true, height: 1080, fps: 0, maxMb: 0 },
    { id: "youtube", label: "YouTube", desc: "16:9 en 1080p, qualité maximale", vertical: false, height: 1080, fps: 0, maxMb: 0 },
    { id: "discord10", label: "Discord 10 Mo", desc: "Compte gratuit : passe partout", vertical: null, height: 1080, fps: 0, maxMb: 10 },
    { id: "discord20", label: "Discord 20 Mo", desc: "Plus net, si ton serveur l'accepte", vertical: null, height: 1080, fps: 0, maxMb: 20 },
    { id: "custom", label: "Personnalisé", desc: "Tu choisis tout", vertical: null, height: 1080, fps: 0, maxMb: 0 },
  ];

  let presetId = $state(studio.project.format === "9:16" ? "tiktok" : "youtube");
  let title = $state(studio.project.name || "Montage");
  let height = $state(1080);
  let fps = $state(0);
  let maxMb = $state(0);
  let busy = $state(false);
  let step = $state("");
  let error = $state("");
  let done = $state<Clip | null>(null);
  let started = 0;
  let elapsed = $state(0);

  function choose(p: Preset) {
    presetId = p.id;
    if (p.id === "custom") return;
    height = p.height;
    fps = p.fps;
    maxMb = p.maxMb;
    // TikTok / YouTube : le format du projet suit (annulable avec Ctrl+Z)
    if (p.vertical === true && P.format !== "9:16") studio.setFormat("9:16");
    if (p.vertical === false && P.format !== "16:9") studio.setFormat("16:9");
  }
  choose(PRESETS.find((p) => p.id === presetId)!);

  const total = $derived(studio.total);
  const hasKeys = $derived(P.pieces.some((p) => (media.inputs[p.clipId]?.length ?? 0) > 0));
  const music = $derived(P.tracks.some((tr) => tr.kind === "audio" && tr.items.length));
  const layersCount = $derived(P.tracks.reduce((a, tr) => a + (tr.kind === "layer" ? tr.items.length : 0), 0));
  /** Même règle que le moteur (editor.rs, size_plan). */
  const plan = $derived.by(() => {
    if (maxMb <= 0) return null;
    const v = (maxMb * 8192 * 0.92) / Math.max(total, 0.5) - 128;
    const ladder: [number, number][] = [[1080, 4500], [720, 2000], [480, 850]];
    const hit = ladder.find(([h, min]) => h <= Math.max(height, 480) && v >= min);
    if (!hit) return { ok: false as const, maxSecs: Math.floor((maxMb * 8192 * 0.92) / (850 + 128)) };
    return { ok: true as const, height: hit[0], fps: v < 3500 ? 30 : fps || 60 };
  });
  const outRes = $derived.by(() => {
    const s = frameSize(P.format, plan?.ok ? plan.height : height);
    return `${s.w}×${s.h}`;
  });

  async function run() {
    if (busy || (plan && !plan.ok)) return;
    busy = true;
    error = "";
    started = Date.now();
    const timer = setInterval(() => (elapsed = (Date.now() - started) / 1000), 250);
    try {
      await studio.saveNow();
      const name = title.trim() || "Montage";
      const spec = await compile($state.snapshot(P) as any, { title: name, short: height, fps, maxMb }, (s) => (step = s));
      step = "";
      const clip = await api.studioRender(spec, mainGame(P));
      if (name !== P.name) studio.rename(name);
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
      <span class="faint mono">{fmtTime(total, true)} · {P.format}{layersCount ? ` · ${t("{n} calques", { n: layersCount })}` : ""}</span>
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
          <span><b>{t("Touches clavier / souris")}</b><small class="faint">{t("Incrustées à l'endroit choisi dans l'aperçu")}</small></span>
          <span class="switch"><input type="checkbox" checked={P.keys.on} onchange={(e) => studio.setKeys({ on: e.currentTarget.checked })} disabled={busy} /><span></span></span>
        </label>
      {/if}
      <div class="summary">
        <div><span class="faint">{t("Sortie")}</span><b class="mono">{outRes}</b></div>
        <div><span class="faint">{t("Images / s")}</span><b class="mono">{plan?.ok ? plan.fps : fps || t("source")}</b></div>
        <div><span class="faint">{t("Taille")}</span><b class="mono">{maxMb ? `≤ ${maxMb} ${t("Mo")}` : t("libre")}</b></div>
        <div><span class="faint">{t("Musique")}</span><b>{music ? t("oui") : t("non")}</b></div>
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
          <span class="mono">{step || `${Math.round(progress * 100)} %${eta !== null ? ` · ${t("encore ~{n} s", { n: Math.ceil(eta) })}` : ""}`}</span>
        </div>
      {/if}
      <footer>
        <span class="faint">{t("Un nouveau fichier est créé ; tes clips d'origine ne bougent pas.")}</span>
        <button class="btn primary go" onclick={run} disabled={busy || (!P.pieces.length && !P.tracks.length) || (plan !== null && !plan.ok)}>
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
