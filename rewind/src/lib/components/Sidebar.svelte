<script lang="ts">
  import { Clapperboard, Crosshair, Settings2, Camera, Mic, MicOff, Pause, Play, Film, Scissors, Timer } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import { liquid, glint } from "$lib/glass";

  let micMuted = $state(false);
  const s = $derived(app.settings);
  const st = $derived(app.status);
  const fill = $derived(st && st.buffer_target > 0 ? Math.min(1, st.buffered / Math.min(st.buffer_target, s?.clip_seconds ?? 30)) : 0);
  const counts = $derived({
    all: app.clips.filter((c) => c.kind !== "screenshot").length,
  });

  function nice(keys: string) {
    return keys.split("+").map((k) => k.trim());
  }

  async function clip(long = false) {
    if (!s) return;
    app.clipping = true;
    await api.clipNow(long ? s.clip_long_seconds : s.clip_seconds);
  }

  function toggleMic() {
    micMuted = !micMuted;
    api.setMicMuted(micMuted);
  }

  const VIEWS = ["library", "studio", "cs2", "settings"];
  const navIndex = $derived(Math.max(0, VIEWS.indexOf(app.view)));

  /** Temps écoulé depuis le début d'un clip sur mesure / d'une session (mm:ss ou h:mm:ss). */
  function elapsed(since: number) {
    void st; // recalculé à chaque mise à jour de l'état (toutes les 0,4 s)
    const s = Math.max(0, Math.floor(Date.now() / 1000 - since));
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
    return h ? `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}` : `${m}:${String(sec).padStart(2, "0")}`;
  }
</script>

<aside>
  <nav style="--i:{navIndex}">
    <button class:on={app.view === "library"} onclick={() => { app.openClip = null; app.view = "library"; }}>
      <Clapperboard size={17} strokeWidth={1.75} /> <span>{t("Clips")}</span> <em>{counts.all}</em>
    </button>
    <button class:on={app.view === "studio"} onclick={() => { app.openClip = null; app.view = "studio"; }}>
      <Film size={17} strokeWidth={1.75} /> <span>Studio</span> {#if app.studio.length}<em>{app.studio.length}</em>{/if}
    </button>
    <button class:on={app.view === "cs2"} onclick={() => (app.view = "cs2")}>
      <Crosshair size={17} strokeWidth={1.75} /> <span>Counter-Strike 2</span>
      {#if app.cs2Map}<b class="live" title={t("Partie en cours")}></b>{/if}
    </button>
    <button class:on={app.view === "settings"} onclick={() => (app.view = "settings")}>
      <Settings2 size={17} strokeWidth={1.75} /> <span>{t("Réglages")}</span>
    </button>
    <!-- Lentille de verre posée AU-DESSUS de l'onglet actif : elle courbe vraiment l'icône et le texte sur ses bords -->
    <i class="pill glass" aria-hidden="true" use:liquid={{ bezel: 7, strength: 9, blur: 0, saturate: 1.2, chroma: 0 }}></i>
  </nav>

  <div class="dock">
    {#if s}
      <button use:glint class="big glass" class:busy={app.clipping} onclick={() => clip(false)} disabled={!st?.recording}>
        <span class="ring" style="--p:{fill}"></span>
        <span class="txt">
          <strong>{t("Clipper")}</strong>
          <small>{t("{n} dernières secondes", { n: s.clip_seconds })}</small>
        </span>
      </button>
      <div class="keys">
        {#each nice(s.hotkeys.clip) as k, i}{#if i}<span class="plus">+</span>{/if}<span class="kbd">{k}</span>{/each}
      </div>

      <div class="row">
        <button use:glint class="mini glass" onclick={() => clip(true)} disabled={!st?.recording} title={t("Clip long ({n} s)", { n: s.clip_long_seconds })}>
          {s.clip_long_seconds >= 60 ? `${Math.round(s.clip_long_seconds / 60)} min` : `${s.clip_long_seconds} s`}
        </button>
        <button use:glint class="mini glass" onclick={() => api.screenshot()} disabled={!st?.recording} title={t("Capture d'écran")}><Camera size={15} strokeWidth={1.75} /></button>
        <button use:glint class="mini glass" class:off={micMuted} onclick={toggleMic} title={micMuted ? t("Réactiver le micro") : t("Couper le micro dans les clips")}>
          {#if micMuted}<MicOff size={15} strokeWidth={1.75} />{:else}<Mic size={15} strokeWidth={1.75} />{/if}
        </button>
        <button use:glint class="mini glass" onclick={() => app.patch((x) => (x.recording_enabled = !x.recording_enabled))} title={s.recording_enabled ? t("Mettre en pause") : t("Reprendre")}>
          {#if s.recording_enabled}<Pause size={15} strokeWidth={1.75} />{:else}<Play size={15} strokeWidth={1.75} />{/if}
        </button>
      </div>

      <div class="holds">
        <button
          use:glint
          class="hold glass"
          class:live={!!st?.custom_since}
          onclick={() => api.holdAction("custom")}
          disabled={!st?.recording && !st?.custom_since}
          title={t("Clip sur mesure ({k}) : un appui pour commencer, un autre pour finir", { k: s.hotkeys.clip_custom })}
        >
          {#if st?.custom_since}<i class="rec"></i>{t("Finir")} · <span class="mono">{elapsed(st.custom_since)}</span>{:else}<Scissors size={14} strokeWidth={1.75} />{t("Sur mesure")}{/if}
        </button>
        <button
          use:glint
          class="hold glass"
          class:live={!!st?.session_since}
          onclick={() => api.holdAction("session")}
          disabled={!st?.recording && !st?.session_since}
          title={t("Session complète ({k}) : tout est gardé dans un seul fichier, avec tes kills repérés", { k: s.hotkeys.session })}
        >
          {#if st?.session_since}<i class="rec"></i>{t("Finir")} · <span class="mono">{elapsed(st.session_since)}</span>{:else}<Timer size={14} strokeWidth={1.75} />{t("Session")}{/if}
        </button>
      </div>

      {#if st?.audio?.length}
        <div class="meters">
          {#each st.audio as a}
            <div class="meter" title={a.active ? a.label : `${a.label} (${t("inactif")})`}>
              <span class="lab" class:faint={!a.active}>{t(a.label)}</span>
              <span class="bar"><i style="transform:scaleX({Math.min(1, Math.sqrt(a.level) * 1.1)})"></i></span>
            </div>
          {/each}
        </div>
      {/if}
      {#if st?.encoder}
        <div class="tech mono">{st.width}×{st.height} · {st.fps} {t("i/s")} · {st.encoder.replace("_", " ")}</div>
      {/if}
    {/if}
  </div>
</aside>

<style>
  aside { width: 228px; flex: none; display: flex; flex-direction: column; justify-content: space-between; padding: 6px 10px 12px; background: var(--bg); }

  nav { position: relative; display: flex; flex-direction: column; gap: 2px; }
  nav button { position: relative; display: flex; align-items: center; gap: 10px; height: 36px; padding: 0 11px; border-radius: 10px; color: var(--text-3); font-weight: 500; text-align: left; transition: background var(--dur) var(--ease), color var(--dur) var(--ease); }
  nav button span { flex: 1; }
  nav button em { font-style: normal; font-family: var(--mono); font-size: 11px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  nav button:hover { color: var(--text); background: color-mix(in srgb, var(--text) 4%, transparent); }
  nav button.on { color: var(--text); background: transparent; }
  /* Lentille : au-dessus des onglets, ne capte pas les clics, glisse avec un léger ressort */
  .pill { position: absolute; z-index: 2; left: 0; right: 0; top: 0; height: 36px; border-radius: 10px; pointer-events: none; transform: translateY(calc(var(--i) * 38px)); transition: transform 520ms var(--spring); }
  /* Thème clair : la lentille est au-dessus du texte, elle doit rester presque transparente */
  :global(:root[data-theme="light"]) .pill { background: linear-gradient(180deg, rgba(255, 255, 255, 0.35), rgba(255, 255, 255, 0.1)); }
  .live { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent); }

  .dock { display: flex; flex-direction: column; gap: 6px; }
  /* Bouton Clipper : verre plus épais, le seul gros bouton de la colonne */
  .big { display: flex; align-items: center; gap: 11px; padding: 11px 12px; border-radius: 14px; color: var(--text); text-align: left; transition: transform 140ms var(--ease), opacity var(--dur); }
  .big:active:not(:disabled) { transform: scale(0.975); }
  .big:disabled { opacity: 0.45; }
  .big .txt { position: relative; z-index: 2; flex: 1; display: flex; flex-direction: column; line-height: 1.2; min-width: 0; }
  .big strong { font-weight: 600; font-size: 14px; letter-spacing: -0.01em; }
  .big small { font-size: 11.5px; color: var(--text-3); }
  .keys { display: flex; align-items: center; justify-content: center; gap: 4px; margin: -1px 0 3px; }
  .plus { color: var(--text-3); font-size: 10.5px; }
  .ring { position: relative; z-index: 2; width: 26px; height: 26px; border-radius: 50%; flex: none; background: conic-gradient(var(--text) calc(var(--p) * 360deg), color-mix(in srgb, var(--text) 14%, transparent) 0); -webkit-mask: radial-gradient(circle, transparent 9.5px, #000 10px); mask: radial-gradient(circle, transparent 9.5px, #000 10px); }
  .big.busy .ring { animation: spin 0.9s linear infinite; background: conic-gradient(var(--text) 90deg, color-mix(in srgb, var(--text) 14%, transparent) 0); }
  @keyframes spin { to { transform: rotate(360deg); } }

  .row { display: grid; grid-template-columns: 1.3fr 1fr 1fr 1fr; gap: 6px; }
  .mini { height: 32px; border-radius: 10px; display: grid; place-items: center; font-size: 12px; font-weight: 500; color: var(--text-2); transition: color var(--dur) var(--ease), transform 140ms var(--ease), opacity var(--dur); }
  .mini:hover:not(:disabled) { color: var(--text); }
  .mini:active:not(:disabled) { transform: scale(0.94); }
  .mini.off { color: var(--rec); }
  .mini:disabled { opacity: 0.35; }
  .holds { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .hold { height: 32px; border-radius: 10px; display: flex; align-items: center; justify-content: center; gap: 6px; font-size: 12px; font-weight: 500; color: var(--text-2); transition: color var(--dur) var(--ease), transform 140ms var(--ease), opacity var(--dur); white-space: nowrap; }
  .hold:hover:not(:disabled) { color: var(--text); }
  .hold:active:not(:disabled) { transform: scale(0.96); }
  .hold:disabled { opacity: 0.35; }
  .hold.live { color: var(--rec); }
  .hold .rec { width: 6px; height: 6px; border-radius: 50%; background: var(--rec); animation: pulse 1.2s infinite; }

  /* Niveaux sonores : sans boîte, filets fins */
  .meters { display: flex; flex-direction: column; gap: 5px; padding: 8px 4px 2px; }
  .meter { display: grid; grid-template-columns: 70px 1fr; align-items: center; gap: 8px; font-size: 11px; color: var(--text-3); }
  .lab { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .bar { height: 2px; border-radius: 2px; background: var(--panel-3); overflow: hidden; }
  .bar i { display: block; height: 100%; width: 100%; background: var(--text-2); transform-origin: left; transition: transform 120ms linear; }
  .tech { font-size: 10px; color: var(--text-3); text-align: center; padding-top: 4px; }
</style>
