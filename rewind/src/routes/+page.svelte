<script lang="ts">
  import "$lib/mock";
  import "$lib/styles.css";
  import { onMount } from "svelte";
  import { app } from "$lib/state.svelte";
  import { api } from "$lib/api";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Library from "$lib/components/Library.svelte";
  import Viewer from "$lib/components/Viewer.svelte";
  import Cs2View from "$lib/components/Cs2View.svelte";
  import FriendsView from "$lib/components/FriendsView.svelte";
  import { friends } from "$lib/friends.svelte";
  import Studio from "$lib/components/Studio.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import Toasts from "$lib/components/Toasts.svelte";
  import Welcome from "$lib/components/Welcome.svelte";
  import Intro from "$lib/components/Intro.svelte";

  let ready = $state(false);
  // Présence Discord : la page affichée (« Monte une vidéo dans le Studio »…)
  $effect(() => {
    const context = app.openClip ? "viewer" : app.view;
    api.discordContext(context).catch(() => {});
  });

  // Animation d'ouverture (au lancement ou « Revoir » dans les réglages) : on arrive toujours sur la page
  // Clips. Posée dès le début, pour que la page qui apparaît derrière la fin de l'animation soit déjà la bonne.
  $effect(() => {
    if (app.showIntro) app.view = "library";
  });

  onMount(async () => {
    await app.init();
    // Partage entre amis : pastille de la barre latérale et messages « X t'a envoyé un clip »
    friends.start();
    // Aperçu navigateur seulement (captures d'écran) : ?view=studio&open=m1&studio=1,3,4
    if ((window as any).__REWIND_MOCK__) {
      const q = new URLSearchParams(location.search);
      const v = q.get("view");
      if (v === "library" || v === "studio" || v === "friends" || v === "cs2" || v === "settings") app.view = v;
      const ids = q.get("studio");
      if (ids) await app.openStudio(ids.split(",").map((i) => app.clips.find((c) => c.id === "m" + i)!).filter(Boolean));
      const open = q.get("open");
      if (open) app.openClip = app.clips.find((c) => c.id === "m" + open) ?? null;
    }
    // Animation d'ouverture : seulement la première fois que la fenêtre s'ouvre après le lancement de
    // Rewind (Rust s'en souvient : la fenêtre est recréée à chaque ouverture). Coupée avec les animations.
    const s = app.settings;
    if (s && s.intro !== false && s.animations && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
      app.showIntro = await api.introClaim().catch(() => false);
    }
    ready = true;
    // Fenêtre rouverte sans intro : la barre latérale joue seule son arrivée (avec l'intro, c'est l'intro qui
    // la lance quand le logo se pose). Rien si les animations sont coupées.
    if (!app.showIntro && s?.animations && !matchMedia("(prefers-reduced-motion: reduce)").matches) {
      requestAnimationFrame(() => app.navArrive++);
    }
    // Premier démarrage : la fenêtre d'accueil (import Medal, dossier, raccourcis, qualité, options)
    if (app.settings && !app.settings.welcome_done) app.showWelcome = true;
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && app.openClip && !document.fullscreenElement) app.openClip = null;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="shell">
  <TitleBar />
  {#if ready}
    <div class="body appear">
      <Sidebar />
      <main>
        {#key app.view}
          <div class="view">
            {#if app.view === "library"}
              <Library />
            {:else if app.view === "studio"}
              <Studio />
            {:else if app.view === "friends"}
              <FriendsView />
            {:else if app.view === "cs2"}
              <Cs2View />
            {:else}
              <SettingsView />
            {/if}
          </div>
        {/key}
      </main>
    </div>
  {/if}
  {#if app.openClip}
    <Viewer clip={app.openClip} />
  {/if}
  {#if ready && app.showWelcome && app.settings}
    <Welcome onclose={() => (app.showWelcome = false)} />
  {/if}
  {#if app.showIntro}
    <Intro ondone={() => { app.showIntro = false; app.view = "library"; }} />
  {/if}
  <Toasts />
</div>

<style>
  .shell { height: 100vh; display: flex; flex-direction: column; background: var(--bg); }
  .body { flex: 1; min-height: 0; display: flex; }
  /* Changement de page : la nouvelle page monte doucement */
  .view { height: 100%; animation: view-in 220ms var(--ease) both; }
  /* Zone de contenu : à peine plus claire que le fond noir, détachée par un filet fin */
  main { flex: 1; min-width: 0; position: relative; background: var(--bg-2); border-top-left-radius: 12px; box-shadow: inset 1px 1px 0 var(--line); overflow: hidden; }
</style>
