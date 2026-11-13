<script lang="ts">
  // Fenêtre affichée au lancement quand une nouvelle version existe : mettre à jour, ou plus tard.
  import "$lib/mock";
  import "$lib/styles.css";
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { ArrowRight, ShieldCheck } from "@lucide/svelte";
  import Logo from "$lib/components/Logo.svelte";
  import { EN } from "$lib/i18n-en";

  type Info = { current: string; available: string | null; notes: string };
  let info = $state<Info | null>(null);
  let en = $state(false);
  let installing = $state(false);
  let progress = $state(0);
  let error = $state("");

  // Fenêtre à part : pas d'état d'appli chargé, la langue vient des réglages lus ici
  function t(fr: string, vars: Record<string, string> = {}) {
    let s = en ? (EN[fr] ?? fr) : fr;
    for (const [k, v] of Object.entries(vars)) s = s.replace(`{${k}}`, v);
    return s;
  }

  // Notes de version : « français --- anglais », première ligne = résumé, lignes « - » = points
  const notes = $derived.by(() => {
    const parts = (info?.notes ?? "").split(/\r?\n---\r?\n/);
    const text = (en && parts[1] ? parts[1] : parts[0]).trim();
    const lines = text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
    return {
      summary: lines.find((l) => !l.startsWith("- ")) ?? "",
      points: lines.filter((l) => l.startsWith("- ")).map((l) => l.slice(2)).slice(0, 4),
    };
  });

  onMount(() => {
    const demo = (window as any).__REWIND_MOCK__;
    (async () => {
      const s: any = demo ? null : await invoke("get_settings").catch(() => null);
      const lang = s?.language ?? "auto";
      en = lang === "en" || (lang === "auto" && !navigator.language.toLowerCase().startsWith("fr"));
      document.documentElement.dataset.theme = s?.theme === "light" ? "light" : "dark";
      if (s?.animations === false) document.documentElement.dataset.motion = "off";
      info = demo
        ? { current: "0.2.2", available: "0.2.3", notes: "Fenêtre de mise à jour au lancement.\n\n- Point 1\n- Point 2" }
        : await invoke<Info | null>("pending_update");
    })();
    const un = listen<{ op: string; value: number }>("edit-progress", (e) => {
      if (e.payload.op === "update") progress = e.payload.value;
    });
    return () => { un.then((f) => f()); };
  });

  async function update() {
    if (installing) return;
    installing = true;
    error = "";
    try {
      await invoke("install_update"); // Rewind se relance tout seul une fois installé
    } catch (e) {
      error = String(e);
      installing = false;
    }
  }

  function later() {
    if (!installing) invoke("update_later");
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") later();
    if (e.key === "Enter") update();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="win" data-tauri-drag-region>
  {#if info}
    <div class="in">
      <header data-tauri-drag-region>
        <Logo size={40} />
        <div class="head" data-tauri-drag-region>
          <span class="kicker"><i></i>{t("Mise à jour disponible")}</span>
          <h1>Rewind {info.available}</h1>
          <p class="sub">{t("Tu as la version {v}.", { v: info.current })}</p>
        </div>
      </header>

      {#if notes.summary || notes.points.length}
        <section class="notes">
          {#if notes.summary}<p class="summary">{notes.summary}</p>{/if}
          {#if notes.points.length}
            <ul>
              {#each notes.points as p}<li>{p}</li>{/each}
            </ul>
          {/if}
        </section>
      {/if}

      <footer>
        {#if installing}
          <div class="progress">
            <div class="bar"><i style="transform: scaleX({Math.max(0.03, progress)})"></i></div>
            <span>{progress >= 1 ? t("Installation…") : `${t("Téléchargement…")} ${Math.round(progress * 100)} %`}</span>
          </div>
        {:else}
          <p class="safe">
            {#if error}<span class="err">{t("Mise à jour impossible")} : {error}</span>
            {:else}<ShieldCheck size={14} />{t("Signée et vérifiée avant l'installation")}{/if}
          </p>
          <div class="actions">
            <button class="btn ghost" onclick={later}>{t("Plus tard")}</button>
            <button class="btn primary" onclick={update}>{error ? t("Réessayer") : t("Mettre à jour")}<ArrowRight size={15} /></button>
          </div>
        {/if}
      </footer>
    </div>
  {/if}
</div>

<style>
  .win {
    height: 100vh;
    background: var(--bg-2);
    box-shadow: inset 0 0 0 1px var(--line);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .in {
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 28px 28px 22px;
    animation: rise 260ms var(--ease-out) both;
  }
  @keyframes rise {
    from { opacity: 0; transform: translateY(6px) scale(0.99); }
  }
  header { display: flex; gap: 16px; align-items: center; }
  header :global(svg) { flex: none; border-radius: 10px; box-shadow: 0 8px 24px -10px rgba(0, 0, 0, 0.9); }
  .kicker {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  /* Le seul point de couleur : il y a du nouveau */
  .kicker i { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  h1 { margin: 3px 0 0; font-size: 22px; line-height: 1.15; }
  .sub { margin: 3px 0 0; color: var(--text-2); font-size: 13px; }

  .notes {
    margin-top: 22px;
    padding: 14px 16px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .summary { margin: 0; font-weight: 500; }
  ul { margin: 8px 0 0; padding: 0; list-style: none; display: grid; gap: 6px; }
  li { position: relative; padding-left: 14px; color: var(--text-2); font-size: 13px; line-height: 1.45; }
  li::before { content: ""; position: absolute; left: 2px; top: 0.62em; width: 4px; height: 1px; background: var(--text-3); }

  footer { margin-top: 20px; display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 36px; }
  .safe { margin: 0; display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-3); min-width: 0; }
  .err { color: var(--rec); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; gap: 8px; flex: none; }
  .actions .btn { height: 36px; padding: 0 16px; }
  .actions .btn.primary { gap: 8px; }

  .progress { flex: 1; display: flex; align-items: center; gap: 12px; font-size: 12.5px; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .bar { flex: 1; height: 3px; border-radius: 3px; background: var(--panel-3); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--hi); transform-origin: left; transition: transform 200ms var(--ease); }
</style>
