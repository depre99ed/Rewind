<script lang="ts">
  // Planche d'essais des animations lettre par lettre (développement seulement) : /essais-anim?t=depre99ed
  // Le vrai dessin du Studio (drawText) à différents instants de l'animation.
  import { onMount } from "svelte";
  import { loadFonts } from "$lib/studio/fonts";
  import { newText, type TextAnimIn } from "$lib/studio/model";
  import { measureText, drawText } from "$lib/studio/render";
  import { LOOKS } from "$lib/studio/textfx";

  const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
  const text = params.get("t") || "depre99ed";
  const anims: TextAnimIn[] = ["letters", "drop", "rise", "spin", "blur", "glitch"];
  const times = [0.04, 0.12, 0.2, 0.28, 0.36, 0.44, 0.52, 0.7, 1.2];
  let ready = $state(false);

  function draw(canvas: HTMLCanvasElement, args: { anim: TextAnimIn; t: number }) {
    const l = { ...newText(0, "chrome"), text, animIn: args.anim, dur: 3, look: LOOKS.find((x) => x.id === "chrome")!.look };
    const ctx = canvas.getContext("2d")!;
    const k = 0.5;
    const b = measureText(ctx, l, k);
    canvas.width = b.w;
    canvas.height = b.h;
    drawText(ctx, l, k, b, 1, args.t);
  }
  onMount(async () => { if (!import.meta.env.DEV) return; await loadFonts(); ready = true; });
</script>

{#if ready}
  <div class="sheet">
    {#each anims as a}
      <div class="row"><b>{a}</b>{#each times as tt}<div class="cell"><canvas use:draw={{ anim: a, t: tt }}></canvas><span>{tt.toFixed(2)} s</span></div>{/each}</div>
    {/each}
  </div>
{/if}

<style>
  :global(html), :global(body) { background: #15161a; color: #ddd; margin: 0; overflow: auto !important; height: auto !important; }
  .sheet { padding: 16px; font: 12px Inter, sans-serif; display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; align-items: center; gap: 6px; }
  .row b { width: 60px; }
  .cell { display: flex; flex-direction: column; align-items: center; gap: 2px; background: #202228; border-radius: 6px; }
  canvas { width: 190px; height: auto; }
</style>
