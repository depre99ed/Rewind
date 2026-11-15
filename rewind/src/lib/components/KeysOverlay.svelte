<script lang="ts" module>
  // Lettres de la disposition du clavier (Z Q S D en AZERTY), demandées une fois au moteur
  import { api } from "$lib/api";
  let labelsCache: Record<string, string> | null = null;
  let pending: Promise<Record<string, string>> | null = null;
  function loadLabels(): Promise<Record<string, string>> {
    if (labelsCache) return Promise.resolve(labelsCache);
    pending ??= api.keyLabels().then((l) => (labelsCache = l ?? {})).catch(() => (labelsCache = {}));
    return pending;
  }
</script>

<script lang="ts">
  // Touches clavier / souris allumées selon la position de lecture. Même dessin que celui incrusté à
  // l'export du Studio (studio/render.ts) : ce qu'on voit ici est ce qui sort dans la vidéo.
  import type { InputEvent } from "$lib/api";
  import { app } from "$lib/state.svelte";
  import { t } from "$lib/i18n.svelte";
  import { drawKeys, keysSize, keyIndex, layoutLabels, presetOf, type KeysPreset } from "$lib/studio/render";

  let { events, time, width, floating = true, preset, layout }: { events: InputEvent[]; time: number; width?: number; floating?: boolean; preset?: KeysPreset; layout?: string } = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let winLabels = $state<Record<string, string>>(labelsCache ?? {});
  loadLabels().then((l) => (winLabels = l));

  // Réglages › Superposition : nombre de touches et lettres (QWERTY / AZERTY / clavier de Windows)
  const P = $derived(preset ?? presetOf(app.settings?.overlay_keys));
  const labels = $derived(layoutLabels(layout ?? app.settings?.overlay_layout, winLabels));
  const isDown = $derived(keyIndex(events));
  const u = $derived((width ?? (P === "complet" ? 300 : P === "compact" ? 190 : 250)) / keysSize(1, P).w);
  const S = $derived(keysSize(u, P));

  $effect(() => {
    if (!canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const s = keysSize(u * dpr, P);
    canvas.width = s.w;
    canvas.height = s.h;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const tm = time;
    drawKeys(ctx, u * dpr, (k) => isDown(k, tm), labels, (x) => t(x), undefined, P);
  });
</script>

<canvas bind:this={canvas} class="keys" class:floating style="width:{S.w}px;height:{S.h}px" aria-hidden="true"></canvas>

<style>
  .keys { display: block; pointer-events: none; }
  .keys.floating { position: absolute; left: 14px; bottom: 14px; z-index: 4; }
</style>
