<script lang="ts">
  import { Clapperboard, Crosshair, Settings2, Camera, Mic, MicOff, Pause, Play, Film, Scissors, Timer, Ellipsis, History, Send, Download, X, ShieldExclamationPoint } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { api, fileUrl, relativeDate } from "$lib/api";
  import { fit } from "$lib/fit";
  import { t } from "$lib/i18n.svelte";
  import { keyParts } from "$lib/keys";
  import { liquid, glint } from "$lib/glass";

  let micMuted = $state(false);
  let menu = $state(false);
  let menuEl = $state<HTMLDivElement>();
  let moreBtn = $state<HTMLButtonElement>();
  const s = $derived(app.settings);
  const st = $derived(app.status);
  const fill = $derived(st && st.buffer_target > 0 ? Math.min(1, st.buffered / Math.min(st.buffer_target, s?.clip_seconds ?? 30)) : 0);
  const counts = $derived({
    all: app.clips.filter((c) => c.kind !== "screenshot").length,
  });
  const last = $derived([...app.clips].sort((a, b) => b.created - a.created)[0] ?? null);

  async function clip(long = false) {
    if (!s) return;
    menu = false;
    app.clipping = true;
    await api.clipNow(long ? s.clip_long_seconds : s.clip_seconds);
  }

  function toggleMic() {
    micMuted = !micMuted;
    api.setMicMuted(micMuted);
  }

  const VIEWS = ["library", "studio", "cs2", "settings"];
  const navIndex = $derived(Math.max(0, VIEWS.indexOf(app.view)));

  /** Durée en m:ss ou h:mm:ss. */
  function clock(sec: number) {
    const v = Math.max(0, Math.floor(sec));
    const h = Math.floor(v / 3600), m = Math.floor((v % 3600) / 60), s2 = v % 60;
    return h ? `${h}:${String(m).padStart(2, "0")}:${String(s2).padStart(2, "0")}` : `${m}:${String(s2).padStart(2, "0")}`;
  }
  /** Temps écoulé depuis le début d'un clip sur mesure / d'une session. */
  function elapsed(since: number) {
    void st; // recalculé à chaque mise à jour de l'état (toutes les 0,4 s)
    return clock(Date.now() / 1000 - since);
  }

  // État de l'enregistrement en une ligne : la seule couleur est le point rouge quand ça enregistre.
  // En marche : la qualité réelle (définition, images par seconde, encodeur) ; la durée en mémoire est dans l'infobulle.
  const rec = $derived.by(() => {
    if (!s || !st) return { kind: "off", text: "" };
    if (!s.recording_enabled) return { kind: "paused", text: t("En pause") };
    if (st.error) return { kind: "error", text: t("Enregistrement arrêté") };
    if (st.starting || !st.recording) return { kind: "starting", text: t("Démarrage…") };
    return { kind: "rec", text: clock(Math.min(st.buffered, st.buffer_target)) };
  });
  /** « NVENC », « AMD », « QuickSync », « x264 » + le codec, à partir du nom FFmpeg (h264_nvenc…). */
  function encoderLabel(e: string) {
    const chip = e.includes("nvenc") ? "NVENC" : e.includes("amf") ? "AMD" : e.includes("qsv") ? "QuickSync" : e.includes("x264") ? "x264" : e.toUpperCase();
    const codec = e.startsWith("hevc") ? "H.265" : e.startsWith("av1") ? "AV1" : "H.264";
    return { chip, codec };
  }
  const quality = $derived.by(() => {
    if (!st?.encoder || !st.width || !st.height) return null;
    const enc = encoderLabel(st.encoder);
    return { res: `${Math.min(st.width, st.height)}p`, fps: `${st.fps} FPS`, enc: enc.chip, full: `${st.width}×${st.height} · ${st.fps} ${t("i/s")} · ${enc.chip} ${enc.codec}` };
  });

  // Jeu lancé en administrateur : ses touches sont invisibles pour Rewind tant que l'assistant n'est pas autorisé
  $effect(() => {
    const read = () => api.adminInputStatus().then((a) => (app.adminInput = a)).catch(() => {});
    read();
    const id = setInterval(read, 5000);
    return () => clearInterval(id);
  });
  // Medal détecté avec des clips pas encore importés (une fois l'accueil passé) : petite carte discrète
  let medalLeft = $state(0);
  $effect(() => {
    void app.clips.length; // relu après un import (la bibliothèque change)
    if (!s?.welcome_done || s.medal_prompt_dismissed) return;
    api.medalDetect().then((m) => (medalLeft = m.found ? m.clips - m.imported : 0)).catch(() => {});
  });
  const adminBlocked = $derived(!!app.adminInput?.game_admin && !app.adminInput.helper);

  function onKey(e: KeyboardEvent) {
    if (menu && e.key === "Escape") menu = false;
  }
  function onDown(e: MouseEvent) {
    if (!menu) return;
    const n = e.target as Node;
    if (menuEl?.contains(n) || moreBtn?.contains(n)) return;
    menu = false;
  }
  const run = (f: () => unknown) => () => {
    menu = false;
    f();
  };
</script>

<svelte:window onkeydown={onKey} onmousedown={onDown} />

<aside>
  <nav style="--i:{navIndex}">
    <button class:on={app.view === "library"} onclick={() => { app.openClip = null; app.view = "library"; }}>
      <Clapperboard size={17} strokeWidth={1.75} /> <span>{t("Clips")}</span> <em>{counts.all}</em>
    </button>
    <button class:on={app.view === "studio"} onclick={() => { app.openClip = null; app.view = "studio"; }}>
      <Film size={17} strokeWidth={1.75} /> <span>Studio</span>
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
      <!-- Clip sur mesure / session en cours : toujours visibles, avec de quoi les finir -->
      {#if st?.custom_since}
        <button class="hold" onclick={() => api.holdAction("custom")}>
          <i class="dot rec"></i><span>{t("Sur mesure")}</span><span class="mono">{elapsed(st.custom_since)}</span><b>{t("Finir")}</b>
        </button>
      {/if}
      {#if st?.session_since}
        <button class="hold" onclick={() => api.holdAction("session")}>
          <i class="dot rec"></i><span>{t("Session")}</span><span class="mono">{elapsed(st.session_since)}</span><b>{t("Finir")}</b>
        </button>
      {/if}

      {#if adminBlocked}
        <!-- Seul avertissement rouge de la colonne : les raccourcis ne marchent pas dans ce jeu -->
        <button class="adminwarn" onclick={() => { app.settingsSection = "keys"; app.view = "settings"; }}>
          <ShieldExclamationPoint size={15} strokeWidth={1.75} />
          <span><b>{t("Raccourcis bloqués en jeu")}</b><small>{t("Jeu en administrateur · Corriger")}</small></span>
        </button>
      {/if}

      {#if medalLeft > 0 && s.welcome_done && !s.medal_prompt_dismissed}
        <div class="medal">
          <button class="mopen" onclick={() => { app.settingsSection = "clips"; app.view = "settings"; }}>
            <Download size={15} strokeWidth={1.75} /><span><b>{t("Clips Medal trouvés")}</b><small>{t("{n} à importer", { n: medalLeft })}</small></span>
          </button>
          <button class="mx" onclick={() => app.patch((x) => (x.medal_prompt_dismissed = true))} aria-label={t("Masquer")} title={t("Masquer")}><X size={13} /></button>
        </div>
      {/if}

      <!-- Dernier clip : le retrouver d'un clic après un round -->
      {#if last}
        <div class="last" class:fresh={Date.now() / 1000 - last.created < 120}>
          <button class="lopen" onclick={() => { app.openClip = last; }} title={t("Ouvrir le dernier clip")}>
            <span class="lthumb">{#if last.thumb}<img use:fit src={fileUrl(last.thumb)} alt="" />{/if}</span>
            <span class="ltext"><small>{t("Dernier clip")} · {relativeDate(last.created)}</small><strong>{last.title}</strong></span>
          </button>
          {#if last.kind !== "screenshot"}
            <button class="ldisc" onclick={() => app.copyForDiscord(last)} disabled={app.discordBusy === last.id} title={t("Copier pour Discord")} aria-label={t("Copier pour Discord")}><Send size={14} strokeWidth={1.75} /></button>
          {/if}
        </div>
      {/if}

      <button use:glint class="big glass" class:busy={app.clipping} onclick={() => clip(false)} disabled={!st?.recording}>
        <span class="ring" style="--p:{fill}"></span>
        <span class="txt">
          <span class="line">
            <strong>{t("Clipper")}</strong>
            {#if s.hotkeys.clip}
              <span class="keys">{#each keyParts(s.hotkeys.clip) as k, i}{#if i}<span class="plus">+</span>{/if}<span class="kbd">{k}</span>{/each}</span>
            {/if}
          </span>
          <small>{t("{n} dernières secondes", { n: s.clip_seconds })}</small>
        </span>
      </button>

      <div class="status">
        <span class="state {rec.kind}" title={rec.kind === "rec" ? `${t("{t} en mémoire, prêtes à clipper", { t: rec.text })}${quality ? ` · ${quality.full}` : ""}` : rec.text}>
          <i class="dot"></i>
          {#if rec.kind === "rec" && quality}
            <span class="q mono">{quality.res}</span><i class="sep"></i><span class="q mono">{quality.fps}</span><i class="sep"></i><span class="q">{quality.enc}</span>
          {:else if rec.kind === "rec"}<span class="mono">{rec.text}</span><span class="faint">{t("en mémoire")}</span>
          {:else}<span>{rec.text}</span>{/if}
        </span>
        <button bind:this={moreBtn} class="more" class:on={menu} onclick={() => (menu = !menu)} aria-label={t("Plus d'actions")} aria-expanded={menu}>
          <Ellipsis size={16} strokeWidth={1.75} />
        </button>
      </div>

      {#if menu}
        <div class="menu glass" bind:this={menuEl} role="menu">
          <button role="menuitem" onclick={() => clip(true)} disabled={!st?.recording}>
            <History size={15} strokeWidth={1.75} /><span>{t("Clip long ({n} s)", { n: s.clip_long_seconds })}</span>
            <span class="hint">{keyParts(s.hotkeys.clip_long).join("+")}</span>
          </button>
          <button role="menuitem" onclick={run(() => api.screenshot())} disabled={!st?.recording}>
            <Camera size={15} strokeWidth={1.75} /><span>{t("Capture d'écran")}</span>
            <span class="hint">{keyParts(s.hotkeys.screenshot).join("+")}</span>
          </button>
          <button role="menuitem" onclick={run(() => api.holdAction("custom"))} disabled={!st?.recording && !st?.custom_since}>
            <Scissors size={15} strokeWidth={1.75} /><span>{st?.custom_since ? t("Finir le clip sur mesure") : t("Clip sur mesure")}</span>
            <span class="hint">{keyParts(s.hotkeys.clip_custom).join("+")}</span>
          </button>
          <button role="menuitem" onclick={run(() => api.holdAction("session"))} disabled={!st?.recording && !st?.session_since}>
            <Timer size={15} strokeWidth={1.75} /><span>{st?.session_since ? t("Finir la session") : t("Session complète")}</span>
            <span class="hint">{keyParts(s.hotkeys.session).join("+")}</span>
          </button>
          <hr />
          <button role="menuitem" onclick={run(toggleMic)}>
            {#if micMuted}<MicOff size={15} strokeWidth={1.75} /><span>{t("Réactiver le micro")}</span>{:else}<Mic size={15} strokeWidth={1.75} /><span>{t("Couper le micro dans les clips")}</span>{/if}
          </button>
          <button role="menuitem" onclick={run(() => app.patch((x) => (x.recording_enabled = !x.recording_enabled)))}>
            {#if s.recording_enabled}<Pause size={15} strokeWidth={1.75} /><span>{t("Mettre en pause")}</span>{:else}<Play size={15} strokeWidth={1.75} /><span>{t("Reprendre")}</span>{/if}
            <span class="hint">{keyParts(s.hotkeys.toggle_recording).join("+")}</span>
          </button>
          {#if st?.audio?.length}
            <hr />
            <div class="meters">
              {#each st.audio as a}
                <div class="meter" title={a.active ? a.label : `${a.label} (${t("inactif")})`}>
                  <span class="lab" class:faint={!a.active}>{t(a.label)}</span>
                  <span class="bar"><i style="transform:scaleX({Math.min(1, Math.sqrt(a.level) * 1.1)})"></i></span>
                </div>
              {/each}
            </div>
          {/if}
          {#if rec.kind === "rec"}
            <div class="tech mono">{t("{t} en mémoire, prêtes à clipper", { t: rec.text })}</div>
          {/if}
        </div>
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

  .dock { position: relative; display: flex; flex-direction: column; gap: 8px; }
  /* Bouton Clipper : verre plus épais, le seul gros bouton de la colonne */
  .big { display: flex; align-items: center; gap: 11px; padding: 11px 10px 11px 12px; border-radius: 14px; color: var(--text); text-align: left; transition: transform 140ms var(--ease), opacity var(--dur); }
  .big:active:not(:disabled) { transform: scale(0.975); }
  .big:disabled { opacity: 0.45; }
  .big .txt { position: relative; z-index: 2; flex: 1; display: flex; flex-direction: column; gap: 2px; line-height: 1.2; min-width: 0; }
  .big .line { display: flex; align-items: center; justify-content: space-between; gap: 6px; }
  .big strong { font-weight: 600; font-size: 14px; letter-spacing: -0.01em; }
  .big small { font-size: 11.5px; color: var(--text-3); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .keys { position: relative; z-index: 2; display: flex; align-items: center; gap: 3px; flex: none; }
  .plus { color: var(--text-3); font-size: 10px; }
  .ring { position: relative; z-index: 2; width: 26px; height: 26px; border-radius: 50%; flex: none; background: conic-gradient(var(--text) calc(var(--p) * 360deg), color-mix(in srgb, var(--text) 14%, transparent) 0); -webkit-mask: radial-gradient(circle, transparent 9.5px, #000 10px); mask: radial-gradient(circle, transparent 9.5px, #000 10px); }
  .big.busy .ring { animation: spin 0.9s linear infinite; background: conic-gradient(var(--text) 90deg, color-mix(in srgb, var(--text) 14%, transparent) 0); }
  @keyframes spin { to { transform: rotate(360deg); } }

  /* Dernier clip : vignette + titre, discret (pas de carte dans une carte) */
  .last { display: flex; align-items: center; gap: 4px; padding: 4px; border-radius: 12px; transition: background var(--dur) var(--ease); }
  .last:hover { background: color-mix(in srgb, var(--text) 5%, transparent); }
  .lopen { flex: 1; min-width: 0; display: flex; align-items: center; gap: 9px; text-align: left; }
  .lthumb { position: relative; width: 58px; height: 34px; flex: none; border-radius: 7px; overflow: hidden; background: #0d0e12; box-shadow: 0 0 0 1px var(--line); }
  .lthumb img { width: 100%; height: 100%; object-fit: cover; }
  .fresh .lthumb { box-shadow: 0 0 0 1px var(--line), 0 0 0 3px color-mix(in srgb, var(--rec) 0%, transparent); animation: freshin 600ms var(--ease) both; }
  @keyframes freshin { from { transform: scale(0.9); opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .fresh .lthumb { animation: none; } }
  .ltext { display: flex; flex-direction: column; min-width: 0; line-height: 1.25; }
  .ltext small { font-size: 10.5px; color: var(--text-3); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ltext strong { font-size: 12px; font-weight: 500; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ldisc { width: 28px; height: 28px; flex: none; border-radius: 8px; display: grid; place-items: center; color: var(--text-3); opacity: 0; transition: opacity var(--dur) var(--ease), color var(--dur) var(--ease), background var(--dur) var(--ease); }
  .last:hover .ldisc { opacity: 1; }
  .ldisc:hover { color: var(--text); background: color-mix(in srgb, var(--text) 8%, transparent); }

  /* Ligne d'état : texte discret, un seul point de couleur */
  .status { display: flex; align-items: center; justify-content: space-between; padding: 0 2px 0 6px; height: 28px; }
  .state { display: flex; align-items: center; gap: 7px; font-size: 12px; color: var(--text-3); min-width: 0; }
  .state .mono { color: var(--text-2); font-variant-numeric: tabular-nums; }
  .state .q { color: var(--text-2); font-size: 11px; white-space: nowrap; }
  .state:has(.q) { gap: 6px; }
  .state .sep { width: 3px; height: 3px; border-radius: 50%; background: var(--text-3); opacity: 0.6; flex: none; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--text-3); flex: none; }
  .state.rec .dot, .dot.rec { background: var(--rec); box-shadow: 0 0 0 3px color-mix(in srgb, var(--rec) 16%, transparent); }
  .state.error { color: var(--rec); }
  .more { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; color: var(--text-3); transition: color var(--dur) var(--ease), background var(--dur) var(--ease); }
  .more:hover, .more.on { color: var(--text); background: color-mix(in srgb, var(--text) 6%, transparent); }

  .medal { display: flex; align-items: center; gap: 2px; border-radius: 10px; border: 1px solid var(--line-2); background: var(--panel); }
  .mopen { flex: 1; min-width: 0; display: flex; align-items: center; gap: 9px; padding: 8px 4px 8px 10px; text-align: left; color: var(--text-2); }
  .mopen span { display: flex; flex-direction: column; min-width: 0; }
  .mopen b { font-size: 12px; font-weight: 600; color: var(--text); }
  .mopen small { font-size: 11px; color: var(--text-3); }
  .mx { width: 26px; height: 26px; margin-right: 4px; border-radius: 7px; display: grid; place-items: center; color: var(--text-3); }
  .mx:hover { color: var(--text); background: var(--panel-3); }
  .adminwarn { display: flex; align-items: flex-start; gap: 9px; padding: 9px 10px; border-radius: 10px; text-align: left; color: var(--rec); background: color-mix(in srgb, var(--rec) 9%, transparent); border: 1px solid color-mix(in srgb, var(--rec) 28%, transparent); transition: background var(--dur) var(--ease); }
  .adminwarn:hover { background: color-mix(in srgb, var(--rec) 14%, transparent); }
  .adminwarn span { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .adminwarn b { font-size: 12px; font-weight: 600; color: var(--text); }
  .adminwarn small { font-size: 11px; color: var(--text-2); }

  /* Clip sur mesure / session en cours */
  .hold { display: flex; align-items: center; gap: 8px; height: 34px; padding: 0 10px; border-radius: 10px; font-size: 12px; color: var(--text-2); background: color-mix(in srgb, var(--rec) 8%, transparent); border: 1px solid color-mix(in srgb, var(--rec) 22%, transparent); transition: background var(--dur) var(--ease); }
  .hold:hover { background: color-mix(in srgb, var(--rec) 13%, transparent); }
  .hold span:first-of-type { flex: 1; text-align: left; }
  .hold .mono { font-variant-numeric: tabular-nums; color: var(--text); }
  .hold b { font-weight: 600; color: var(--text); }
  .hold .rec { animation: pulse 1.2s infinite; }

  /* Menu « … » : flotte au-dessus du contenu (le verre est permis ici) */
  .menu { position: absolute; left: 0; right: 0; bottom: calc(100% + 8px); z-index: 30; display: flex; flex-direction: column; padding: 6px; border-radius: 14px; transform-origin: bottom center; animation: menuIn 180ms var(--ease); }
  @keyframes menuIn { from { opacity: 0; transform: translateY(6px) scale(0.98); } }
  @media (prefers-reduced-motion: reduce) { .menu { animation: none; } }
  .menu button { position: relative; z-index: 2; display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 9px; border-radius: 9px; font-size: 12.5px; color: var(--text-2); text-align: left; transition: background var(--dur) var(--ease), color var(--dur) var(--ease); }
  .menu button span:not(.hint) { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .menu button:hover:not(:disabled) { color: var(--text); background: color-mix(in srgb, var(--text) 7%, transparent); }
  .menu button:disabled { opacity: 0.4; }
  .hint { font-family: var(--mono); font-size: 10.5px; color: var(--text-3); white-space: nowrap; }
  .menu hr { position: relative; z-index: 2; border: 0; height: 1px; background: var(--line); margin: 5px 6px; }

  /* Niveaux sonores : sans boîte, filets fins */
  .meters { position: relative; z-index: 2; display: flex; flex-direction: column; gap: 5px; padding: 4px 9px 2px; }
  .meter { display: grid; grid-template-columns: 70px 1fr; align-items: center; gap: 8px; font-size: 11px; color: var(--text-3); }
  .lab { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .bar { height: 2px; border-radius: 2px; background: var(--panel-3); overflow: hidden; }
  .bar i { display: block; height: 100%; width: 100%; background: var(--text-2); transform-origin: left; transition: transform 120ms linear; }
  .tech { position: relative; z-index: 2; font-size: 10px; color: var(--text-3); text-align: center; padding: 6px 0 2px; }
</style>
