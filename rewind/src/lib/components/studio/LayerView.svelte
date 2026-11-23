<script lang="ts">
  // Un calque dans l'aperçu, placé avec la même formule que l'export (layerState de model.ts).
  import { fileUrl } from "$lib/api";
  import { isFilterFx, layerState, letterActive, type Layer } from "$lib/studio/model";
  import { measureText, drawText, drawFx } from "$lib/studio/render";

  let { layer, local, fw, fh, onpick }: { layer: Layer; local: number; fw: number; fh: number; onpick?: (e: PointerEvent) => void } = $props();

  const st = $derived(layerState(layer, local));
  // Valeurs simples : le texte n'est redessiné que si sa taille ou sa frappe changent (pas à chaque image)
  const drawScale = $derived(Math.round(st.scale * 1000) / 1000);
  const reveal = $derived(st.reveal);
  // Lettre par lettre : redessiné 30 fois par seconde pendant l'animation seulement
  const letterT = $derived(layer.kind === "text" && letterActive(layer, local) ? Math.round(local * 30) / 30 : -1);
  let canvas: HTMLCanvasElement | undefined = $state();
  let fxCanvas: HTMLCanvasElement | undefined = $state();
  let box = $state({ w: 0, h: 0 });

  // Texte : dessiné sur canvas avec la même fonction que l'image envoyée à l'export
  $effect(() => {
    if (layer.kind !== "text" || !canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const k = (fh / 1080) * drawScale * dpr;
    const ctx = canvas.getContext("2d");
    if (!ctx || k <= 0) return;
    void layer.text, layer.font, layer.size, layer.color, layer.stroke, layer.shadow, layer.bg, layer.align, layer.look, fontsReady;
    const b = measureText(ctx, layer, k);
    canvas.width = b.w;
    canvas.height = b.h;
    drawText(ctx, layer, k, b, reveal, letterT);
    box = { w: b.w / dpr, h: b.h / dpr };
  });
  // Les polices arrivent un peu après le premier affichage : on redessine quand elles sont prêtes
  let fontsReady = $state(0);
  $effect(() => {
    if (layer.kind === "text") document.fonts?.ready.then(() => fontsReady++);
  });

  // Effet « voile » (flash, noir, vignette, bandes) : même dessin que l'image envoyée à l'export
  $effect(() => {
    if (layer.kind !== "fx" || !fxCanvas) return;
    const dpr = Math.min(1.5, window.devicePixelRatio || 1);
    const w = Math.max(2, Math.round(fw * dpr)), h = Math.max(2, Math.round(fh * dpr));
    void layer.fx, layer.strength;
    fxCanvas.width = w;
    fxCanvas.height = h;
    const ctx = fxCanvas.getContext("2d");
    if (ctx) drawFx(ctx, layer, w, h);
  });

  const isFx = $derived(layer.kind === "fx");
  const size = $derived.by(() => {
    if (layer.kind === "text") return box;
    if (layer.kind === "blur") return { w: layer.w * fw, h: layer.h * fh };
    if (layer.kind === "fx") return { w: fw, h: fh };
    const w = st.scale * fw;
    return { w, h: w * (layer.ih / Math.max(1, layer.iw)) };
  });
  // Un effet est toujours plein cadre, immobile
  const pos = $derived(isFx ? { x: 0.5, y: 0.5, rot: 0 } : { x: st.x, y: st.y, rot: st.rot });
</script>

<div
  class="layer {layer.kind}"
  class:fx={isFx}
  style="left:{pos.x * fw - size.w / 2}px;top:{pos.y * fh - size.h / 2}px;width:{size.w}px;height:{size.h}px;opacity:{st.opacity};transform:rotate({pos.rot}deg)"
  data-layer={layer.uid}
  onpointerdown={isFx ? undefined : onpick}
  role="presentation"
>
  {#if layer.kind === "text"}
    <canvas bind:this={canvas} style="width:{box.w}px;height:{box.h}px"></canvas>
  {:else if layer.kind === "image" || layer.kind === "gif"}
    <img src={fileUrl(layer.path)} alt="" draggable="false" />
  {:else if layer.kind === "video"}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video src={fileUrl(layer.path)} data-pip={layer.uid} preload="auto" playsinline crossorigin="anonymous"></video>
  {:else if layer.kind === "blur"}
    <!-- Redessinée à chaque image par Preview.svelte (drawBlurs) avec l'image qui est dessous -->
    <canvas class="blurcv" data-blur={layer.uid}></canvas>
  {:else if layer.kind === "fx" && !isFilterFx(layer)}
    <canvas class="fxcv" bind:this={fxCanvas}></canvas>
  {/if}
</div>

<style>
  .layer { position: absolute; transform-origin: 50% 50%; pointer-events: auto; cursor: move; }
  .layer.fx { pointer-events: none; }
  .layer canvas, .layer img, .layer video { display: block; width: 100%; height: 100%; object-fit: fill; user-select: none; -webkit-user-drag: none; }
</style>
