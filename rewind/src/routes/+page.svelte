<script lang="ts">
  import "$lib/mock";
  import "$lib/styles.css";
  import { onMount } from "svelte";
  import { app } from "$lib/state.svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import Library from "$lib/components/Library.svelte";
  import Viewer from "$lib/components/Viewer.svelte";
  import Cs2View from "$lib/components/Cs2View.svelte";
  import Studio from "$lib/components/Studio.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import Toasts from "$lib/components/Toasts.svelte";

  let ready = $state(false);
  onMount(async () => {
    await app.init();
    // Aperçu navigateur seulement (captures d'écran) : ?view=studio&open=m1&studio=1,3,4
    if ((window as any).__REWIND_MOCK__) {
      const q = new URLSearchParams(location.search);
      const v = q.get("view");
      if (v === "library" || v === "studio" || v === "cs2" || v === "settings") app.view = v;
      const ids = q.get("studio");
      if (ids) await app.openStudio(ids.split(",").map((i) => app.clips.find((c) => c.id === "m" + i)!).filter(Boolean));
      const open = q.get("open");
      if (open) app.openClip = app.clips.find((c) => c.id === "m" + open) ?? null;
    }
    ready = true;
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
