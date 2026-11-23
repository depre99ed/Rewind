<script lang="ts">
  // Choix de police en plein écran : chaque police est dessinée en grand, avec le vrai texte et le vrai rendu
  // (chrome, éclat…) du calque, comme dans la vidéo. Un clic choisit, Échap ferme.
  import { X, Search } from "@lucide/svelte";
  import { untrack } from "svelte";
  import { FONTS, FONT_GROUPS, fontOf, cssFont, loadFonts, type FontGroup } from "$lib/studio/fonts";
  import { paintLine, LOOKS, type TextLook } from "$lib/studio/textfx";
  import { t } from "$lib/i18n.svelte";

  let { text, current, look, color, onpick, onclose }: {
    text: string;
    current: string;
    look: TextLook | null | undefined;
    color: string;
    onpick: (id: string, look: TextLook | null) => void;
    onclose: () => void;
  } = $props();

  let group = $state<FontGroup | "all">(FONTS.find((f) => f.id === current)?.group ?? "all");
  // Rendu montré (et appliqué au clic) : celui du texte s'il correspond à un rendu proposé, sinon « Mon rendu »
  const same = (a: TextLook | null | undefined, b: TextLook | null) => JSON.stringify(a ?? null) === JSON.stringify(b);
  const custom = $derived(!!look && !LOOKS.some((l) => same(look, l.look)));
  // (valeur de départ seulement : ensuite, c'est le choix fait dans la fenêtre)
  let lookId = $state(untrack(() => (!look ? "flat" : (LOOKS.find((l) => same(look, l.look))?.id ?? "mine"))));
  const shownLook = $derived(lookId === "flat" ? null : lookId === "mine" ? (look ?? null) : (LOOKS.find((l) => l.id === lookId)?.look ?? null));
  let q = $state("");
  let ready = $state(false);
  let root: HTMLDivElement | undefined = $state();
  const sample = $derived((text || "").split("\n")[0].trim().slice(0, 24) || "depre99ed");
  const shown = $derived(FONTS.filter((f) => (group === "all" || f.group === group) && (!q.trim() || f.label.toLowerCase().includes(q.trim().toLowerCase()))));

  $effect(() => {
    if (!root) return;
    const el = root;
    document.body.appendChild(el); // au-dessus de tout, quel que soit le panneau qui l'ouvre
    return () => el.remove();
  });
  $effect(() => { loadFonts().then(() => (ready = true)); });

  /** Dessine l'aperçu d'une police à la taille de sa carte (net sur les écrans à haute densité). */
  function draw(canvas: HTMLCanvasElement, args: { font: string; ready: boolean; sample: string; look: TextLook | null }) {
    const render = () => {
      if (!args.ready) return;
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      const w = canvas.clientWidth, h = canvas.clientHeight;
      canvas.width = Math.max(1, Math.round(w * dpr));
      canvas.height = Math.max(1, Math.round(h * dpr));
      const ctx = canvas.getContext("2d")!;
      ctx.scale(dpr, dpr);
      ctx.clearRect(0, 0, w, h);
      const f = fontOf(args.font);
      let px = h * 0.5;
      ctx.font = cssFont(f, px);
      const mw = ctx.measureText(args.sample).width;
      if (mw > w * 0.86) { px = (px * w * 0.86) / mw; ctx.font = cssFont(f, px); }
      ctx.textAlign = "center";
      ctx.textBaseline = "alphabetic";
      const m = ctx.measureText(args.sample);
      const y = h / 2 + (m.actualBoundingBoxAscent - m.actualBoundingBoxDescent) / 2;
      const top = y - m.actualBoundingBoxAscent, bottom = y + m.actualBoundingBoxDescent;
      if (args.look) paintLine(ctx, args.sample, w / 2, y, px, args.look, { color: args.look.fill === "solid" ? color : "#ffffff", top, bottom });
      else { ctx.fillStyle = color; ctx.fillText(args.sample, w / 2, y); }
    };
    render();
    const ro = new ResizeObserver(render);
    ro.observe(canvas);
    return { update: (a: typeof args) => { args = a; render(); }, destroy: () => ro.disconnect() };
  }

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onclose(); }
  }
</script>

<svelte:window onkeydown={key} />

<div class="picker" bind:this={root} role="dialog" aria-modal="true" aria-label={t("Choisir une police")}>
  <header>
    <h2>{t("Choisir une police")}</h2>
    <div class="groups" role="tablist">
      <button class="chip" class:on={group === "all"} onclick={() => (group = "all")}>{t("Toutes")} <b>{FONTS.length}</b></button>
      {#each FONT_GROUPS as g}<button class="chip" class:on={group === g.id} onclick={() => (group = g.id)}>{t(g.label)} <b>{FONTS.filter((f) => f.group === g.id).length}</b></button>{/each}
    </div>
    <div class="looks" role="radiogroup" aria-label={t("Rendu")}>
      <span class="lbl">{t("Rendu")}</span>
      <button class="chip" class:on={lookId === "flat"} onclick={() => (lookId = "flat")}>{t("Plat")}</button>
      {#each LOOKS as l (l.id)}<button class="chip" class:on={lookId === l.id} onclick={() => (lookId = l.id)}>{t(l.label)}</button>{/each}
      {#if custom}<button class="chip" class:on={lookId === "mine"} onclick={() => (lookId = "mine")}>{t("Mon rendu")}</button>{/if}
    </div>
    <label class="search"><Search size={15} /><input placeholder={t("Chercher")} bind:value={q} /></label>
    <button class="btn icon ghost" onclick={onclose} aria-label={t("Fermer")} title={t("Fermer (Échap)")}><X size={18} /></button>
  </header>
  <div class="grid">
    {#each shown as f (f.id)}
      <button class="card" class:on={f.id === current} onclick={() => { onpick(f.id, shownLook ? structuredClone($state.snapshot(shownLook)) : null); onclose(); }}>
        <canvas use:draw={{ font: f.id, ready, sample, look: shownLook }}></canvas>
        <span class="name">{f.label}</span>
      </button>
    {:else}
      <p class="none">{t("Aucune police ne correspond.")}</p>
    {/each}
  </div>
</div>

<style>
  .picker { position: fixed; inset: 0; z-index: 80; display: flex; flex-direction: column; background: rgba(6, 7, 9, 0.97); animation: fp-in 180ms var(--ease) both; }
  @keyframes fp-in { from { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .picker { animation: none; } }
  header { display: flex; align-items: center; flex-wrap: wrap; gap: 12px 16px; padding: 18px 28px; border-bottom: 1px solid var(--line); }
  .looks { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; width: 100%; order: 5; }
  .looks .lbl { font-size: 12px; color: var(--text-3); margin-right: 4px; }
  h2 { margin: 0; font-size: 18px; font-weight: 600; white-space: nowrap; }
  .groups { display: flex; gap: 6px; flex-wrap: wrap; flex: 1; }
  .chip { display: inline-flex; align-items: center; gap: 6px; height: 30px; padding: 0 12px; border-radius: 99px; font-size: 13px; color: var(--text-2); border: 1px solid var(--line); }
  .chip b { font-weight: 500; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .chip.on { color: var(--text); border-color: var(--line-2); background: var(--panel-2); }
  .search { display: flex; align-items: center; gap: 8px; width: 220px; height: 34px; padding: 0 12px; border-radius: 9px; background: var(--bg); border: 1px solid var(--line); color: var(--text-3); }
  .search input { flex: 1; min-width: 0; background: transparent; border: 0; outline: none; color: var(--text); font-size: 13px; }
  .grid { flex: 1; overflow-y: auto; display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); gap: 14px; padding: 22px 28px 40px; align-content: start; }
  .card { display: flex; flex-direction: column; gap: 8px; padding: 10px 10px 12px; border-radius: 14px; background: var(--panel); border: 1px solid var(--line); text-align: left; transition: border-color var(--dur) var(--ease), transform var(--dur) var(--ease); }
  .card:hover { border-color: var(--line-2); transform: translateY(-1px); }
  .card.on { border-color: var(--hi); }
  .card canvas { width: 100%; height: 130px; border-radius: 10px; background: radial-gradient(120% 140% at 30% 20%, #23252b 0%, #121317 60%, #0c0d10 100%); }
  .name { padding: 0 4px; font-size: 13px; color: var(--text-2); }
  .card.on .name { color: var(--text); }
  .none { color: var(--text-3); font-size: 13px; }
</style>
