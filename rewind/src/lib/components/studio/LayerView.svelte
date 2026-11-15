<script lang="ts">
  // Un calque dans l'aperçu, placé avec la même formule que l'export (layerState de model.ts).
  import { fileUrl } from "$lib/api";
  import { layerState, type Layer } from "$lib/studio/model";
  import { measureText, drawText } from "$lib/studio/render";

  let { layer, local, fw, fh, onpick }: { layer: Layer; local: number; fw: number; fh: number; onpick?: (e: PointerEvent) => void } = $props();

  const st = $derived(layerState(layer, local));
  // Valeurs simples : le texte n'est redessiné que si sa taille ou sa frappe changent (pas à chaque image)
  const drawScale = $derived(Math.round(st.scale * 1000) / 1000);
  const reveal = $derived(st.reveal);
  let canvas: HTMLCanvasElement | undefined = $state();
  let box = $state({ w: 0, h: 0 });

  // Texte : dessiné sur canvas avec la même fonction que l'image envoyée à l'export
  $effect(() => {
    if (layer.kind !== "text" || !canvas) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const k = (fh / 1080) * drawScale * dpr;
    const ctx = canvas.getContext("2d");
    if (!ctx || k <= 0) return;
    void layer.text, layer.font, layer.size, layer.color, layer.stroke, layer.shadow, layer.bg, layer.align, fontsReady;
    const b = measureText(ctx, layer, k);
    canvas.width = b.w;
    canvas.height = b.h;
    drawText(ctx, layer, k, b, reveal);
    box = { w: b.w / dpr, h: b.h / dpr };
  });
  // Les polices arrivent un peu après le premier affichage : on redessine quand elles sont prêtes
  let fontsReady = $state(0);
  $effect(() => {
    if (layer.kind === "text") document.fonts?.ready.then(() => fontsReady++);
  });

  const size = $derived.by(() => {
    if (layer.kind === "text") return box;
    if (layer.kind === "blur") return { w: layer.w * fw, h: layer.h * fh };
    const w = st.scale * fw;
    return { w, h: w * (layer.ih / Math.max(1, layer.iw)) };
  });
</script>

<div
  class="layer {layer.kind}"
  style="left:{st.x * fw - size.w / 2}px;top:{st.y * fh - size.h / 2}px;width:{size.w}px;height:{size.h}px;opacity:{st.opacity};transform:rotate({st.rot}deg);--blur:{layer.kind === 'blur' ? (layer.strength * fh) / 1080 : 0}px"
  data-layer={layer.uid}
  onpointerdown={onpick}
  role="presentation"
>
  {#if layer.kind === "text"}
    <canvas bind:this={canvas} style="width:{box.w}px;height:{box.h}px"></canvas>
  {:else if layer.kind === "image" || layer.kind === "gif"}
    <img src={fileUrl(layer.path)} alt="" draggable="false" />
  {:else if layer.kind === "video"}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video src={fileUrl(layer.path)} data-pip={layer.uid} preload="auto" playsinline></video>
  {:else if layer.kind === "blur"}
    <div class="blurbox" class:pixel={layer.mode === "pixel"}></div>
  {/if}
</div>

<style>
  .layer { position: absolute; transform-origin: 50% 50%; pointer-events: auto; cursor: move; }
  .layer canvas, .layer img, .layer video { display: block; width: 100%; height: 100%; object-fit: fill; user-select: none; -webkit-user-drag: none; }
  .blurbox { width: 100%; height: 100%; backdrop-filter: blur(var(--blur)); -webkit-backdrop-filter: blur(var(--blur)); }
  /* Pixelisé : l'aperçu montre un flou plus une grille (l'export pixelise vraiment) */
  .blurbox.pixel { backdrop-filter: blur(calc(var(--blur) * 0.6)) saturate(1.1); background-image: linear-gradient(rgba(0,0,0,.18) 1px, transparent 1px), linear-gradient(90deg, rgba(0,0,0,.18) 1px, transparent 1px); background-size: var(--blur) var(--blur); }
</style>
