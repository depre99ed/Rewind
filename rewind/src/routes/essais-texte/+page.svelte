<script lang="ts">
  // Planche d'essais des styles de texte (développement seulement) : /essais-texte?t=depre99ed
  // Utilise le vrai rendu du Studio (textfx.ts) : ce qui est ici est ce qui sort à l'export.
  import { onMount } from "svelte";
  import { fontOf, cssFont, loadFonts } from "$lib/studio/fonts";
  import { paintLine, LOOKS, type TextLook } from "$lib/studio/textfx";

  const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
  const text = params.get("t") || "depre99ed";
  const fontIds = (params.get("f") || "oldenglish,fraktur,pirata,grenze,jacquard,germania,fruktur,rocker,metal").split(",");
  const chrome = LOOKS.find((l) => l.id === "chrome")!.look;
  const variants: { label: string; look: TextLook }[] = (params.get("v") === "colors"
    ? [
        { label: "Chrome 3D", look: chrome },
        { label: "Or 3D", look: LOOKS.find((l) => l.id === "gold")!.look },
        { label: "Métal rouge", look: LOOKS.find((l) => l.id === "metal")!.look },
        { label: "Néon (police au choix)", look: LOOKS.find((l) => l.id === "neon")!.look },
      ]
    : [
        { label: "A · Chrome net", look: { ...chrome, depth: 0, glow: null, sparkle: 0 } },
        { label: "B · Chrome 3D", look: chrome },
        { label: "C · Chrome 3D + éclat", look: { ...chrome, depth: 0.7, glow: { color: "#ffffff", strength: 0.7, size: 0.6 }, sparkle: 0.9 } },
      ]);
  let ready = $state(false);
  const W = 560, H = 190, DPR = 2;

  function draw(canvas: HTMLCanvasElement, args: { font: string; look: TextLook }) {
    const render = () => {
      const ctx = canvas.getContext("2d")!;
      canvas.width = W * DPR;
      canvas.height = H * DPR;
      ctx.scale(DPR, DPR);
      // fond : image de jeu sombre stylisée (dégradé), pour juger le rendu comme sur une vidéo
      const bg = ctx.createLinearGradient(0, 0, W, H);
      bg.addColorStop(0, "#2a2620");
      bg.addColorStop(0.5, "#141518");
      bg.addColorStop(1, "#1d2430");
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, W, H);
      const f = fontOf(args.font);
      let px = 92;
      ctx.font = cssFont(f, px);
      const w = ctx.measureText(text).width;
      if (w > W * 0.86) { px = (px * W * 0.86) / w; ctx.font = cssFont(f, px); }
      ctx.textAlign = "center";
      ctx.textBaseline = "alphabetic";
      const m = ctx.measureText(text);
      const y = H / 2 + (m.actualBoundingBoxAscent - m.actualBoundingBoxDescent) / 2;
      paintLine(ctx, text, W / 2, y, px, args.look, { color: "#ffffff", top: y - m.actualBoundingBoxAscent, bottom: y + m.actualBoundingBoxDescent });
    };
    render();
    return { update: (a: typeof args) => { args = a; render(); } };
  }

  // Page de développement seulement (planche d'essais) : vide dans l'appli publiée
  onMount(async () => { if (!import.meta.env.DEV) return; await loadFonts(); ready = true; });
</script>

<div class="sheet">
  <h1>Essais : « {text} »</h1>
  {#if ready}
    <div class="grid" style="grid-template-columns: 140px repeat({variants.length}, {W}px)">
      <span></span>
      {#each variants as v}<span class="head">{v.label}</span>{/each}
      {#each fontIds as id}
        <span class="font">{fontOf(id).label}</span>
        {#each variants as v}<canvas style="width:{W}px;height:{H}px" use:draw={{ font: id, look: v.look }}></canvas>{/each}
      {/each}
    </div>
  {/if}
</div>

<style>
  :global(html), :global(body) { background: #0b0c0f; color: #ededef; margin: 0; overflow: auto !important; height: auto !important; }
  .sheet { padding: 24px; font-family: Inter, sans-serif; }
  h1 { font-size: 18px; font-weight: 600; margin: 0 0 16px; }
  .grid { display: grid; gap: 10px; align-items: center; }
  .head { font-size: 13px; color: #a0a0a6; }
  .font { font-size: 14px; font-weight: 600; }
  canvas { border-radius: 8px; display: block; }
</style>
