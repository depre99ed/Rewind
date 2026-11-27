<script lang="ts">
  // Notification en jeu (fenêtre transparente qui ne prend jamais le focus, toast.rs).
  // Refaite le 2026-11-16 dans le style de l'appli : capsule noire, un seul vert (la coche), police de l'appli.
  // Mouvement (2026-11-16, 2e passe) : entrée qui « se pose » (légère montée + échelle), sortie plus courte,
  // contenu qui se fond quand l'état change (enregistrement → enregistré), miniature qui se pose, fine ligne
  // de temps restant, point « en direct » qui pulse 3 fois. Rien qui clignote ni ne boucle sans fin.
  // L'ombre doit tenir dans la marge transparente de la fenêtre (16 px, toast.rs) : sinon Windows la coupe
  // et dessine un rectangle sombre autour de la capsule.
  import "$lib/styles.css";
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { listen } from "@tauri-apps/api/event";
  import { fileUrl } from "$lib/api";
  import { keyParts } from "$lib/keys";
  import Logo from "$lib/components/Logo.svelte";
  import { EN } from "$lib/i18n-en";

  type Toast = { state: string; title: string; subtitle: string; game: string; thumb: string; badges: string[]; seconds: number; keys?: [string, string][]; lang?: string };
  let t = $state<Toast | null>(null);
  let visible = $state(false);
  let key = $state(0);

  // Entrée : la capsule doit d'abord être dessinée cachée, PUIS recevoir « in », sinon elle naît visible
  // et la transition ne se joue jamais (défaut de la 0.4.3). Deux images d'attente = une image peinte cachée.
  function appear() {
    visible = false;
    requestAnimationFrame(() => requestAnimationFrame(() => (visible = true)));
  }

  // Durée d'affichage (mêmes valeurs que toast.rs) : la ligne de temps restant suit la vraie durée
  const lifetime = (s: string) => (s === "saving" ? 0 : s === "welcome" ? 6000 : 3600);

  onMount(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const params = new URLSearchParams(location.search);
    const demo = params.get("demo");
    if (demo) {
      // Aperçu dans un navigateur (captures) : ?demo=saved|saving|shot|failed|marked|welcome|flow (&loop=1 : rejoue)
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
          keys: [["F8", en ? "Clip 30 s" : "Clip 30 s"], ["Alt+Shift+F10", en ? "Clip 2 min" : "Clip 2 min"], ["Alt+F11", en ? "Custom clip" : "Clip sur mesure"], ["Alt+F7", en ? "Full session" : "Session complète"]],
        },
      };
      // flow : enregistrement puis « Clip enregistré », comme dans le jeu
      const play = () => {
        key++;
        t = demo === "flow" ? demos.saving : (demos[demo] ?? demos.saved);
        appear();
        if (demo === "flow") setTimeout(() => { t = demos.saved; key++; }, 1400);
      };
      play();
      if (params.get("loop")) {
        const total = demo === "welcome" ? 6000 : demo === "flow" ? 5000 : 3600;
        const id = setInterval(() => { visible = false; setTimeout(play, 700); }, total + 700);
        return () => clearInterval(id);
      }
      return;
    }
    const un1 = listen<Toast>("toast", (e) => {
      const shown = visible;
      t = e.payload;
      key++;
      if (!shown) appear(); // déjà affichée (enregistrement → enregistré) : le contenu change sur place
    });
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
  const reduce = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const swap = { duration: reduce ? 0 : 160 };
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
    {#key key}<i class="timer" style:animation-duration="{lifetime(t.state)}ms"></i>{/key}
  </div>
{:else if t}
  <div class="toast" class:in={visible}>
    {#key key}
      <span class="media" class:mark={!t.thumb || t.state === "saving"} in:fade={swap}>
        {#if t.thumb && t.state !== "saving"}
          <img src={fileUrl(t.thumb)} alt="" />
        {:else}
          <Logo size={22} color={t.state === "failed" ? "#6b6b71" : "#c8f031"} />
        {/if}
      </span>
      <div class="text" in:fade={swap}>
        <div class="l1">
          {#if t.state === "saved" || t.state === "shot"}
            <svg class="icon check" viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-7" /></svg>
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
      {#if lifetime(t.state)}<i class="timer" style:animation-duration="{lifetime(t.state)}ms"></i>{/if}
    {/key}
  </div>
{/if}

<style>
  :global(html), :global(body) { background: transparent !important; overflow: hidden; }
  .toast {
    /* 16 px de marge = place de l'ombre (0 4px 12px : 16 px vers le bas, 8 vers le haut) */
    position: fixed; inset: 16px; display: flex; align-items: center; gap: 12px; padding: 8px 16px 8px 8px;
    border-radius: 14px; color: #ededef; font-family: var(--font); box-sizing: border-box; max-width: calc(100vw - 32px); overflow: hidden;
    background: rgba(11, 11, 12, 0.95); border: 1px solid rgba(255, 255, 255, 0.09);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    /* Sortie : courte et discrète */
    opacity: 0; transform: translateY(6px) scale(0.97);
    transition: opacity 160ms ease-in, transform 180ms ease-in;
  }
  /* Entrée : la capsule monte de quelques pixels et se pose (décélération longue, sans rebond) */
  .toast.in { opacity: 1; transform: none; transition: opacity 200ms ease-out, transform 340ms cubic-bezier(0.16, 1, 0.3, 1); }
  .media { width: 78px; height: 44px; flex: none; border-radius: 8px; overflow: hidden; background: #161618; display: grid; place-items: center; }
  .media.mark { width: 44px; }
  .media img { width: 100%; height: 100%; object-fit: cover; animation: settle 700ms cubic-bezier(0.16, 1, 0.3, 1) both; }
  .text { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 3px; }
  .l1 { display: flex; align-items: center; gap: 6px; font-size: 13.5px; font-weight: 600; line-height: 1.2; }
  .l2 { display: flex; align-items: center; gap: 8px; font-size: 12px; color: #a0a0a6; line-height: 1.25; }
  .ell { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dur { flex: none; margin-left: auto; font-family: var(--mono); font-size: 11.5px; font-variant-numeric: tabular-nums; color: #6b6b71; }
  .icon { width: 15px; height: 15px; flex: none; fill: none; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .check { stroke: #c8f031; stroke-dasharray: 16; animation: draw 280ms 120ms cubic-bezier(0.2, 0.8, 0.2, 1) both; }
  .cross { stroke: #ff453a; }
  .spin { stroke: #6b6b71; stroke-dasharray: 22 13; animation: turn 900ms linear infinite; transform-origin: center; }
  /* Temps restant avant que la notification disparaisse : filet neutre qui se vide */
  .timer {
    position: absolute; left: 0; right: 0; bottom: 0; height: 2px; background: rgba(255, 255, 255, 0.14);
    transform-origin: left; animation: drain linear both;
  }
  @keyframes draw { from { stroke-dashoffset: 16; } to { stroke-dashoffset: 0; } }
  @keyframes turn { to { transform: rotate(360deg); } }
  @keyframes settle { from { transform: scale(1.12); } to { transform: none; } }
  @keyframes drain { from { transform: scaleX(1); } to { transform: scaleX(0); } }

  /* Arrivée en jeu */
  .welcome { flex-direction: column; align-items: stretch; gap: 10px; padding: 10px 16px 12px 10px; }
  .whead { display: flex; align-items: center; gap: 12px; }
  .welcome .media { width: 40px; height: 40px; border-radius: 10px; }
  .live { position: relative; display: inline-block; width: 6px; height: 6px; margin-right: 2px; border-radius: 50%; background: #c8f031; flex: none; }
  /* « En direct » : une onde qui part du point, 3 fois puis s'arrête */
  .live::after { content: ""; position: absolute; inset: 0; border-radius: 50%; background: #c8f031; animation: ping 1.5s 300ms cubic-bezier(0, 0, 0.2, 1) 3 both; }
  @keyframes ping { from { transform: scale(1); opacity: 0.7; } to { transform: scale(2.8); opacity: 0; } }
  .keys { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 6px 12px; align-items: center; padding-left: 52px; min-width: 0; }
  .combo { display: inline-flex; align-items: center; gap: 3px; }
  .combo b { font-weight: 500; font-size: 10px; color: #6b6b71; }
  kbd { font-family: var(--mono); font-size: 10.5px; font-weight: 500; line-height: 1; padding: 3px 5px; border-radius: 5px; background: #19191b; border: 1px solid rgba(255, 255, 255, 0.1); color: #ededef; }
  .what { font-size: 12px; color: #a0a0a6; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  @media (prefers-reduced-motion: reduce) {
    .toast, .toast.in { transform: none; transition: opacity 120ms linear; }
    .check, .media img, .live::after { animation: none; }
    .spin { animation-duration: 2.4s; }
  }
</style>
