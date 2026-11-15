<script lang="ts">
  // Fenêtre d'accueil (premier démarrage, ou « Revoir l'accueil » dans Réglages › Système) :
  // import des clips Medal, dossier des clips, raccourcis, qualité, jeux et options. Tout est appliqué
  // à la fin ; « Passer » ne change rien.
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { FolderOpen, Check, Download, Keyboard, Gauge, Gamepad2, Sparkles, ArrowLeft } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { api, type MedalInfo, type Hardware } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import HotkeyInput from "./HotkeyInput.svelte";

  let { onclose }: { onclose: () => void } = $props();
  const s = $derived(app.settings!);

  let step = $state(0);
  let medal = $state<MedalInfo | null>(null);
  let hw = $state<Hardware | null>(null);
  // Choix (appliqués à la fin)
  let doImport = $state(true);
  let useMedal = $state(true);
  let folder = $state("");
  let hotkey = $state("");
  let hotkeyLong = $state("");
  let seconds = $state(30);
  let quality = $state<"eco" | "balanced" | "high" | "medal">("balanced");
  let cs2 = $state(true);
  let startup = $state(true);
  let overlay = $state(true);
  let voice = $state(false);
  // Fin : import en cours / terminé
  let importing = $state(false);
  let imported = $state<{ copied: number; failed: number } | null>(null);

  onMount(async () => {
    folder = s.clip_folder;
    hotkey = s.hotkeys.clip;
    hotkeyLong = s.hotkeys.clip_long;
    seconds = s.clip_seconds;
    cs2 = s.cs2.enabled;
    startup = s.launch_at_startup;
    overlay = s.input_overlay;
    voice = s.voice_command;
    quality = s.eco_mode ? "eco" : s.resolution >= 1440 || s.resolution === 0 ? "high" : "balanced";
    [medal, hw] = await Promise.all([api.medalDetect().catch(() => null), api.hardware().catch(() => null)]);
    if (medal?.found) {
      doImport = medal.imported < medal.clips;
      applyMedal(true);
    }
  });

  const hasMedalSettings = $derived(!!medal?.found && !!(medal.hotkey || medal.clip_seconds || medal.fps));
  const medalSummary = $derived(
    medal ? [medal.hotkey, medal.clip_seconds && `${medal.clip_seconds} s`, medal.resolution && `${medal.resolution}p`, medal.fps && `${medal.fps} ${t("i/s")}`].filter(Boolean).join(" · ") : "",
  );
  function applyMedal(on: boolean) {
    useMedal = on;
    if (!medal || !on) return;
    if (medal.hotkey) hotkey = medal.hotkey;
    if (medal.clip_seconds) seconds = medal.clip_seconds;
    if (medal.fps || medal.resolution) quality = "medal";
  }

  const QUALITIES = $derived([
    ...(hasMedalSettings && (medal?.fps || medal?.resolution)
      ? [{ id: "medal" as const, label: "Comme Medal", hint: [medal?.resolution && `${medal.resolution}p`, medal?.fps && `${medal.fps} ${t("i/s")}`, medal?.bitrate && `${medal.bitrate} Mb/s`].filter(Boolean).join(" · ") }]
      : []),
    { id: "eco" as const, label: "Économie", hint: `720p · 30 ${t("i/s")} · 8 Mb/s` },
    { id: "balanced" as const, label: "Équilibrée", hint: `1080p · 60 ${t("i/s")} · 20 Mb/s` },
    { id: "high" as const, label: "Haute", hint: `1440p · 60 ${t("i/s")} · 40 Mb/s` },
  ]);
  const gpu = $derived.by(() => {
    const e = hw?.encoders ?? [];
    if (e.some((x) => x.includes("nvenc"))) return "NVIDIA";
    if (e.some((x) => x.includes("amf"))) return "AMD";
    if (e.some((x) => x.includes("qsv"))) return "Intel";
    return "";
  });

  async function chooseFolder() {
    const dir = await openDialog({ directory: true, defaultPath: folder, title: t("Dossier des clips") });
    if (typeof dir === "string") folder = dir;
  }

  const STEPS = [
    { icon: Download, label: "Clips" },
    { icon: Keyboard, label: "Raccourcis" },
    { icon: Gauge, label: "Qualité" },
    { icon: Gamepad2, label: "Jeux" },
    { icon: Check, label: "Prêt" },
  ];

  /** Applique tous les choix, puis lance l'import Medal si demandé. */
  async function finish() {
    await app.patch((x) => {
      x.clip_folder = folder || x.clip_folder;
      x.hotkeys.clip = hotkey;
      x.hotkeys.clip_long = hotkeyLong;
      x.clip_seconds = seconds;
      x.eco_mode = quality === "eco";
      if (quality === "eco") Object.assign(x, { resolution: 720, fps: 30, bitrate_mbps: 8 });
      else if (quality === "balanced") Object.assign(x, { resolution: 1080, fps: 60, bitrate_mbps: 20 });
      else if (quality === "high") Object.assign(x, { resolution: 1440, fps: 60, bitrate_mbps: 40 });
      else if (medal) Object.assign(x, { resolution: medal.resolution ?? 1080, fps: medal.fps ?? 60, bitrate_mbps: Math.max(8, medal.bitrate ?? 20) });
      x.cs2.enabled = cs2;
      x.launch_at_startup = startup;
      x.input_overlay = overlay;
      x.voice_command = voice;
      x.welcome_done = true;
    });
    step = 4;
    if (medal?.found && doImport && medal.imported < medal.clips) {
      importing = true;
      try {
        const r = await api.medalImport();
        imported = { copied: r.copied, failed: r.failed };
      } catch (e) {
        app.toast("err", t("Import Medal impossible"), String(e));
      }
      importing = false;
    }
  }
  async function skip() {
    await app.patch((x) => (x.welcome_done = true));
    onclose();
  }
  const pct = $derived(app.progress?.op === "medal" ? Math.round(app.progress.value * 100) : importing ? 0 : 100);
</script>

<div class="back" role="presentation">
  <div class="win" role="dialog" aria-modal="true" aria-label={t("Bienvenue sur Rewind")}>
    <header>
      <ol class="steps">
        {#each STEPS as st, i}
          <li class:on={i === step} class:done={i < step}><span class="dot">{#if i < step}<Check size={12} strokeWidth={2.5} />{:else}<st.icon size={13} />{/if}</span><span class="lab">{t(st.label)}</span></li>
        {/each}
      </ol>
      {#if step < 4}<button class="btn small ghost" onclick={skip}>{t("Passer")}</button>{/if}
    </header>

    {#key step}
      <section class="body appear">
        {#if step === 0}
          <h2>{t("Bienvenue sur Rewind")}</h2>
          <p class="lead">{t("Quelques réglages et c'est parti. Tout se change plus tard dans les Réglages.")}</p>
          {#if medal?.found}
            <div class="card">
              <div class="row">
                <div class="txt"><strong>{t("Medal trouvé sur ce PC")}</strong><span>{t("{n} clips · {g} Go", { n: medal.clips, g: (medal.size_mb / 1024).toFixed(1) })}{medal.imported ? ` · ${t("{n} déjà importés", { n: medal.imported })}` : ""}</span></div>
              </div>
              <label class="row opt">
                <span class="txt"><b>{t("Importer mes clips Medal")}</b><small>{t("Ils sont copiés dans Rewind ; Medal garde les siens.")}</small></span>
                <span class="switch"><input type="checkbox" bind:checked={doImport} disabled={medal.imported >= medal.clips} /><span></span></span>
              </label>
              {#if hasMedalSettings}
                <label class="row opt">
                  <span class="txt"><b>{t("Reprendre mes réglages Medal")}</b><small>{medalSummary}</small></span>
                  <span class="switch"><input type="checkbox" checked={useMedal} onchange={(e) => applyMedal(e.currentTarget.checked)} /><span></span></span>
                </label>
              {/if}
            </div>
          {/if}
          <div class="card">
            <div class="row">
              <div class="txt"><strong>{t("Dossier des clips")}</strong><span>{medal?.found ? t("Tes futurs clips et ceux importés de Medal seront rangés ici.") : t("Tes futurs clips seront rangés ici.")}</span><code>{folder}</code></div>
              <button class="btn" onclick={chooseFolder}><FolderOpen size={15} />{t("Changer")}</button>
            </div>
          </div>
        {:else if step === 1}
          <h2>{t("Ton raccourci pour clipper")}</h2>
          <p class="lead">{t("Il marche partout, même en jeu. Une touche seule (F8), un bouton de souris ou une combinaison.")}</p>
          <div class="card">
            <div class="row"><div class="txt"><strong>{t("Clipper")}</strong><span>{t("Garde les dernières secondes.")}</span></div><HotkeyInput value={hotkey} onchange={(v) => (hotkey = v)} /></div>
            <div class="row">
              <div class="txt"><strong>{t("Durée du clip")}</strong></div>
              <div class="seg">{#each [15, 30, 60, 120] as v}<button class:on={seconds === v} onclick={() => (seconds = v)}>{v < 60 ? `${v} s` : `${v / 60} min`}</button>{/each}</div>
            </div>
            <div class="row"><div class="txt"><strong>{t("Clip long")}</strong><span>{t("Second raccourci, pour une action plus longue.")}</span></div><HotkeyInput value={hotkeyLong} onchange={(v) => (hotkeyLong = v)} /></div>
          </div>
        {:else if step === 2}
          <h2>{t("Qualité de l'enregistrement")}</h2>
          <p class="lead">{gpu ? t("Tout est encodé par ta carte graphique {g} : l'effet sur tes jeux est minime.", { g: gpu }) : t("Plus la qualité est haute, plus les clips sont lourds.")}</p>
          <div class="qual">
            {#each QUALITIES as q}
              <button class:on={quality === q.id} onclick={() => (quality = q.id)}>
                <b>{t(q.label)}{#if q.id === "balanced"}<em>{t("Conseillé")}</em>{/if}</b>
                <small class="mono">{q.hint}</small>
              </button>
            {/each}
          </div>
        {:else if step === 3}
          <h2>{t("Jeux et options")}</h2>
          <p class="lead">{t("Active ce qui te sert. Tout se change plus tard.")}</p>
          <div class="card">
            <label class="row opt"><span class="txt"><b>{t("Clips automatiques Counter-Strike 2")}</b><small>{t("Kills, multi-kills et clutchs clippés tout seuls.")}</small></span><span class="switch"><input type="checkbox" bind:checked={cs2} /><span></span></span></label>
            <label class="row opt"><span class="txt"><b>{t("Touches à l'écran")}</b><small>{t("Les touches du clavier et de la souris sur tes clips.")}</small></span><span class="switch"><input type="checkbox" bind:checked={overlay} /><span></span></span></label>
            <label class="row opt"><span class="txt"><b>{t("Commande vocale")}</b><small>{t("Dire « Rewind clip » pour clipper.")}</small></span><span class="switch"><input type="checkbox" bind:checked={voice} /><span></span></span></label>
            <label class="row opt"><span class="txt"><b>{t("Lancer avec Windows")}</b><small>{t("Rewind démarre discrètement dans la barre des tâches.")}</small></span><span class="switch"><input type="checkbox" bind:checked={startup} /><span></span></span></label>
          </div>
        {:else}
          <div class="finish">
            <span class="big"><Sparkles size={22} /></span>
            <h2>{t("C'est prêt")}</h2>
            <p class="lead">{t("Appuie sur {k} en jeu pour garder les {n} dernières secondes.", { k: hotkey || "—", n: seconds })}</p>
            {#if importing || imported}
              <div class="card prog">
                <div class="row"><div class="txt"><strong>{importing ? t("Import des clips Medal…") : t("Clips Medal importés")}</strong><span>{importing ? `${pct} %` : t("{n} clips copiés", { n: imported?.copied ?? 0 })}{imported?.failed ? ` · ${t("{n} en échec", { n: imported.failed })}` : ""}</span></div></div>
                <div class="bar"><i style="transform:scaleX({pct / 100})"></i></div>
              </div>
            {/if}
          </div>
        {/if}
      </section>
    {/key}

    <footer>
      {#if step > 0 && step < 4}<button class="btn ghost" onclick={() => step--}><ArrowLeft size={15} />{t("Retour")}</button>{:else}<span></span>{/if}
      {#if step < 3}<button class="btn primary" onclick={() => step++}>{t("Continuer")}</button>
      {:else if step === 3}<button class="btn primary" onclick={finish}>{t("Terminer")}</button>
      {:else}<button class="btn primary" onclick={onclose}>{importing ? t("Continuer pendant l'import") : t("Commencer")}</button>{/if}
    </footer>
  </div>
</div>

<style>
  .back { position: fixed; inset: 0; z-index: 200; display: grid; place-items: center; padding: 24px; background: var(--overlay); animation: fade-in 200ms var(--ease) both; }
  .win { width: min(680px, 100%); max-height: calc(100vh - 48px); display: flex; flex-direction: column; border-radius: 18px; background: var(--panel); border: 1px solid var(--line-2); box-shadow: var(--shadow); overflow: hidden; }
  header { display: flex; align-items: center; gap: 12px; padding: 14px 16px 14px 20px; border-bottom: 1px solid var(--line); }
  .steps { flex: 1; display: flex; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .steps li { display: flex; flex: none; white-space: nowrap; align-items: center; gap: 7px; padding: 4px 10px 4px 4px; border-radius: 99px; font-size: 12px; color: var(--text-3); transition: color var(--dur) var(--ease), background var(--dur) var(--ease); }
  .steps li.on { color: var(--text); background: var(--panel-3); }
  .steps li.done { color: var(--text-2); }
  .dot { width: 22px; height: 22px; border-radius: 50%; display: grid; place-items: center; background: var(--panel-2); border: 1px solid var(--line-2); }
  .on .dot { background: var(--hi); color: var(--hi-ink); border-color: var(--hi); }
  .done .dot { color: var(--text); }
  @media (max-width: 700px) { .steps .lab { display: none; } .steps li.on .lab { display: inline; } }
  .body { flex: 1; overflow-y: auto; padding: 26px 28px 8px; display: flex; flex-direction: column; gap: 12px; }
  h2 { margin: 0; font-size: 22px; font-weight: 600; letter-spacing: -0.02em; }
  .lead { margin: -4px 0 6px; font-size: 13.5px; line-height: 1.55; color: var(--text-2); }
  .card { display: flex; flex-direction: column; border-radius: 14px; background: var(--bg-2); border: 1px solid var(--line); padding: 2px 16px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 18px; min-height: 58px; padding: 10px 0; }
  .row + .row { border-top: 1px solid var(--line); }
  .opt { cursor: pointer; }
  .txt { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .txt strong, .txt b { font-size: 13.5px; font-weight: 550; }
  .txt span, .txt small { font-size: 12.5px; color: var(--text-3); }
  code { margin-top: 4px; font-family: var(--mono); font-size: 11.5px; color: var(--text-2); word-break: break-all; }
  .seg { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; padding: 3px; border-radius: 10px; background: var(--bg); border: 1px solid var(--line); }
  .seg button { height: 30px; padding: 0 14px; border-radius: 7px; font-size: 13px; color: var(--text-2); white-space: nowrap; }
  .seg button.on { background: var(--panel-3); color: var(--text); }
  .qual { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 8px; }
  .qual button { display: flex; flex-direction: column; align-items: flex-start; gap: 6px; padding: 14px; border-radius: 12px; text-align: left; background: var(--bg-2); border: 1px solid var(--line); color: var(--text-2); transition: border-color var(--dur) var(--ease); }
  .qual button:hover { border-color: var(--line-2); }
  .qual button.on { border-color: var(--hi); color: var(--text); }
  .qual b { display: flex; align-items: center; gap: 6px; font-size: 14px; font-weight: 600; color: var(--text); }
  .qual em { font-style: normal; font-size: 10.5px; font-weight: 500; padding: 1px 6px; border-radius: 5px; background: var(--panel-3); color: var(--text-2); }
  .qual small { font-size: 11.5px; color: var(--text-3); }
  .finish { display: flex; flex-direction: column; align-items: center; text-align: center; gap: 10px; padding: 16px 0; }
  .finish .card { align-self: stretch; text-align: left; }
  .big { width: 48px; height: 48px; border-radius: 14px; display: grid; place-items: center; background: var(--panel-3); }
  .prog { padding-bottom: 14px; }
  .bar { height: 4px; border-radius: 4px; background: var(--panel-3); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--hi); transform-origin: left; transition: transform 200ms linear; }
  footer { display: flex; justify-content: space-between; align-items: center; gap: 8px; padding: 14px 20px; border-top: 1px solid var(--line); }
</style>
