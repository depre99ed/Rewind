<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Minus, Square, X, Copy, ArrowDownCircle } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { formatDuration } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import Logo from "./Logo.svelte";

  const win = getCurrentWindow();
  let maximized = $state(false);
  $effect(() => {
    win.isMaximized().then((m) => (maximized = m));
    const un = win.onResized(async () => (maximized = await win.isMaximized()));
    return () => { un.then((f) => f()); };
  });

  const st = $derived(app.status);
  const paused = $derived(app.settings ? !app.settings.recording_enabled : false);
  const label = $derived.by(() => {
    if (paused) return t("En pause");
    if (!st) return t("Démarrage…");
    if (st.error && !st.recording) return t("Erreur");
    if (st.starting || st.buffered < 0.5) return t("Démarrage…");
    return `REC · ${formatDuration(st.buffered)}`;
  });
  const recState = $derived(paused ? "paused" : st?.error && !st?.recording ? "error" : st?.recording ? "rec" : "idle");
</script>

<header data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <Logo size={20} />
    <span class="word">rewind</span>
  </div>

  <div class="center" data-tauri-drag-region>
    <button class="rec glass {recState}" title={st?.error || t("Activer / mettre en pause l'enregistrement")} onclick={() => app.settings && app.patch((s) => (s.recording_enabled = !s.recording_enabled))}>
      <i></i>{label}
    </button>
    {#if st?.app && recState === "rec"}
      <span class="app" data-tauri-drag-region>{st.app}</span>
    {/if}
  </div>

  <div class="controls">
    {#if app.update?.available}
      <button class="upd" onclick={() => { app.settingsSection = "general"; app.view = "settings"; }} title={t("Une nouvelle version de Rewind est prête (Réglages › Système)")}>
        <ArrowDownCircle size={14} />{t("Mise à jour")} {app.update.available}
      </button>
    {/if}
    <button aria-label={t("Réduire")} onclick={() => win.minimize()}><Minus size={15} /></button>
    <button aria-label={t("Agrandir")} onclick={() => win.toggleMaximize()}>
      {#if maximized}<Copy size={13} />{:else}<Square size={12} />{/if}
    </button>
    <button class="close" aria-label={t("Fermer")} onclick={() => win.close()}><X size={16} /></button>
  </div>
</header>

<style>
  header { height: 42px; flex: none; display: flex; align-items: center; justify-content: space-between; padding-left: 14px; background: var(--bg); }
  .brand { display: flex; align-items: center; gap: 9px; width: 220px; }
  .word { font-weight: 600; font-size: 14px; letter-spacing: -0.02em; }
  .center { flex: 1; display: flex; align-items: center; justify-content: center; gap: 10px; height: 100%; }
  .rec { display: inline-flex; align-items: center; gap: 8px; height: 24px; padding: 0 11px; border-radius: 99px; font-family: var(--mono); font-size: 11.5px; font-weight: 500; letter-spacing: 0.02em; color: var(--text-2); font-variant-numeric: tabular-nums; transition: color var(--dur) var(--ease); }
  .rec i { position: relative; z-index: 2; width: 6px; height: 6px; border-radius: 50%; background: var(--text-3); }
  .rec.rec { color: var(--text); }
  .rec.rec i { background: var(--rec); box-shadow: 0 0 8px color-mix(in srgb, var(--rec) 70%, transparent); animation: pulse 1.6s ease-in-out infinite; }
  .rec.error { color: var(--rec); }
  .rec.error i { background: var(--rec); }
  .rec:hover { color: var(--text); }
  .app { font-size: 12px; color: var(--text-3); }
  .controls { display: flex; height: 100%; }
  .controls button { width: 46px; height: 100%; display: grid; place-items: center; color: var(--text-2); transition: background 120ms; }
  .controls button { color: var(--text-3); }
  .controls button:hover { background: var(--panel-2); color: var(--text); }
  .controls .close:hover { background: #c42b1c; color: #fff; }
  .controls .upd { width: auto; padding: 0 11px; margin: 9px 6px; height: 24px; border-radius: 99px; display: inline-flex; gap: 6px; font-size: 12px; font-weight: 500; background: var(--hi); color: var(--hi-ink); }
  .controls .upd:hover { background: color-mix(in srgb, var(--hi) 85%, var(--bg)); color: var(--hi-ink); }
</style>
