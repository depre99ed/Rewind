<script lang="ts">
  // Notification en jeu (fenêtre transparente qui ne prend jamais le focus, toast.rs).
  // Refaite le 2026-11-16 dans le style de l'appli : capsule noire, un seul vert (la coche), police de l'appli,
  // fondu court à l'entrée et à la sortie, rien qui clignote. Bandeau d'arrivée en jeu : liste des raccourcis.
  import "$lib/styles.css";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { fileUrl } from "$lib/api";
  import { keyParts } from "$lib/keys";
  import Logo from "$lib/components/Logo.svelte";
  import { EN } from "$lib/i18n-en";

  type Toast = { state: string; title: string; subtitle: string; game: string; thumb: string; badges: string[]; seconds: number; keys?: [string, string][]; lang?: string };
  let t = $state<Toast | null>(null);
  let visible = $state(false);
  let key = $state(0);

  onMount(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const params = new URLSearchParams(location.search);
    const demo = params.get("demo");
    if (demo) {
      // Aperçu dans un navigateur (captures) : ?demo=saved|saving|shot|failed|marked|welcome
      const lang = params.get("lang") ?? undefined;
      const en = lang === "en";
      const demos: Record<string, Toast> = {
        saving: { state: "saving", title: en ? "Last 30 seconds" : "30 dernières secondes", subtitle: "Counter-Strike 2", game: "Counter-Strike 2", thumb: "", badges: [], seconds: 30, lang },
        saved: { state: "saved", title: "CS2 ACE · 2× One Tap", subtitle: "Counter-Strike 2 · 34 s", game: "Counter-Strike 2", thumb: "/mock/t1.jpg", badges: [], seconds: 34, lang },
        shot: { state: "shot", title: "Counter-Strike 2 2026-11-16 21h04", subtitle: "", game: "Counter-Strike 2", thumb: "/mock/t3.jpg", badges: [], seconds: 0, lang },
        failed: { state: "failed", title: en ? "Clip failed" : "Clip impossible", subtitle: en ? "The disk is full" : "Le disque est plein", game: "", thumb: "", badges: [], seconds: 0, lang },
        marked: { state: "marked", title: en ? "Marker added" : "Repère ajouté", subtitle: en ? "It will show on the clip timeline" : "Il apparaîtra sur la frise du clip", game: "", thumb: "", badges: [], seconds: 0, lang },
        welcome: {
          state: "welcome", title: "Counter-Strike 2", subtitle: en ? "Rewind is recording · your hotkeys" : "Rewind enregistre · tes raccourcis", game: "Counter-Strike 2", thumb: "", badges: [], seconds: 0, lang,
          keys: [["Alt+F10", en ? "Clip 30 s" : "Clip 30 s"], ["Alt+Shift+F10", en ? "Clip 2 min" : "Clip 2 min"], ["Alt+F11", en ? "Custom clip" : "Clip sur mesure"], ["Alt+F7", en ? "Full session" : "Session complète"]],
        },
      };
      t = demos[demo] ?? demos.saved;
      visible = true;
      return;
    }
    const un1 = listen<Toast>("toast", (e) => { t = e.payload; visible = true; key++; });
    const un2 = listen("toast-hide", () => (visible = false));
    return () => { un1.then((f) => f()); un2.then((f) => f()); };
  });

  // Fenêtre à part : la langue arrive avec chaque notification (réglage de l'appli)
  const L = (fr: string) => (t?.lang === "en" ? (EN[fr] ?? fr) : fr);
  // Nom de touche dans la langue de l'appli (cette fenêtre n'a pas les réglages : « Maj » / « Shift »)
  const keyName = (k: string) => (t?.lang === "en" ? (k === "Maj" ? "Shift" : k) : k === "Shift" ? "Maj" : k);
  const mmss = (s: number) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, "0")}`;
  // Première ligne (ce qui vient de se passer) et seconde ligne (de quoi il s'agit)
  const line1 = $derived(
    !t ? "" : t.state === "saving" ? L("Enregistrement…") : t.state === "saved" ? L("Clip enregistré") : t.state === "shot" ? L("Capture enregistrée") : t.title,
  );
  const line2 = $derived(
    !t ? "" : t.state === "saving" ? [t.title, t.game].filter(Boolean).join(" · ") : t.state === "saved" || t.state === "shot" ? t.title : t.subtitle,
  );
</script>

{#if t && t.state === "welcome"}
  <!-- Arrivée en jeu : jeu reconnu + raccourcis (6 s) -->
  <div class="toast welcome" class:in={visible}>
    <div class="whead">
      <span class="media mark"><Logo size={22} /></span>
      <div class="text">
        <div class="l1">{t.title}</div>
        <div class="l2"><i class="live"></i>{t.subtitle}</div>
      </div>
    </div>
    <div class="keys">
      {#each t.keys ?? [] as [k, what]}
        <span class="combo">{#each keyParts(k).map(keyName) as part, j}{#if j > 0}<b>+</b>{/if}<kbd>{part}</kbd>{/each}</span>
        <span class="what">{what}</span>
      {/each}
    </div>
  </div>
{:else if t}
  <div class="toast" class:in={visible}>
    <span class="media" class:mark={!t.thumb || t.state === "saving"}>
      {#if t.thumb && t.state !== "saving"}
        <img src={fileUrl(t.thumb)} alt="" />
      {:else}
        <Logo size={22} color={t.state === "failed" ? "#6b6b71" : "#c8f031"} />
      {/if}
    </span>
    <div class="text">
      <div class="l1">
        {#if t.state === "saved" || t.state === "shot"}
          {#key key}<svg class="icon check" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>{/key}
        {:else if t.state === "saving"}
          <svg class="icon spin" viewBox="0 0 16 16"><circle cx="8" cy="8" r="5.5" /></svg>
        {:else if t.state === "failed"}
          <svg class="icon cross" viewBox="0 0 16 16"><path d="M4.5 4.5l7 7M11.5 4.5l-7 7" /></svg>
        {/if}
        <span class="ell">{line1}</span>
      </div>
      <div class="l2">
        <span class="ell">{line2}</span>
        {#if t.state === "saved" && t.seconds > 0}<span class="dur">{mmss(t.seconds)}</span>{/if}
      </div>
    </div>
  </div>
{/if}

<style>
  :global(html), :global(body) { background: transparent !important; overflow: hidden; }
  .toast {
    position: fixed; inset: 6px; display: flex; align-items: center; gap: 12px; padding: 8px 16px 8px 8px;
    border-radius: 14px; color: #ededef; font-family: var(--font); box-sizing: border-box; max-width: calc(100vw - 12px); overflow: hidden;
    background: rgba(11, 11, 12, 0.94); border: 1px solid rgba(255, 255, 255, 0.09);
    box-shadow: 0 8px 22px rgba(0, 0, 0, 0.45);
    opacity: 0; transform: translateY(4px);
    transition: opacity 200ms ease, transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .toast.in { opacity: 1; transform: none; }
  .media { width: 78px; height: 44px; flex: none; border-radius: 8px; overflow: hidden; background: #161618; display: grid; place-items: center; }
  .media.mark { width: 44px; }
  .media img { width: 100%; height: 100%; object-fit: cover; }
  .text { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 3px; }
  .l1 { display: flex; align-items: center; gap: 6px; font-size: 13.5px; font-weight: 600; line-height: 1.2; }
  .l2 { display: flex; align-items: center; gap: 8px; font-size: 12px; color: #a0a0a6; line-height: 1.25; }
  .ell { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dur { flex: none; margin-left: auto; font-family: var(--mono); font-size: 11.5px; font-variant-numeric: tabular-nums; color: #6b6b71; }
  .icon { width: 15px; height: 15px; flex: none; fill: none; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .check { stroke: #c8f031; stroke-dasharray: 16; animation: draw 260ms 80ms cubic-bezier(0.2, 0.8, 0.2, 1) both; }
  .cross { stroke: #ff453a; }
  .spin { stroke: #6b6b71; stroke-dasharray: 22 13; animation: turn 900ms linear infinite; transform-origin: center; }
  @keyframes draw { from { stroke-dashoffset: 16; } to { stroke-dashoffset: 0; } }
  @keyframes turn { to { transform: rotate(360deg); } }

  /* Arrivée en jeu */
  .welcome { flex-direction: column; align-items: stretch; gap: 10px; padding: 10px 16px 12px 10px; }
  .whead { display: flex; align-items: center; gap: 12px; }
  .welcome .media { width: 40px; height: 40px; border-radius: 10px; }
  .live { display: inline-block; width: 6px; height: 6px; margin-right: 2px; border-radius: 50%; background: #c8f031; flex: none; }
  .keys { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 6px 12px; align-items: center; padding-left: 52px; min-width: 0; }
  .combo { display: inline-flex; align-items: center; gap: 3px; }
  .combo b { font-weight: 500; font-size: 10px; color: #6b6b71; }
  kbd { font-family: var(--mono); font-size: 10.5px; font-weight: 500; line-height: 1; padding: 3px 5px; border-radius: 5px; background: #19191b; border: 1px solid rgba(255, 255, 255, 0.1); color: #ededef; }
  .what { font-size: 12px; color: #a0a0a6; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  @media (prefers-reduced-motion: reduce) {
    .toast { transform: none; transition: opacity 120ms linear; }
    .check { animation: none; }
    .spin { animation-duration: 2.4s; }
  }
</style>
