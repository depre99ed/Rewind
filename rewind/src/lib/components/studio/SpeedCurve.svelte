<script lang="ts">
  // Courbe de vitesse d'un morceau : points à tirer (instant du clip, vitesse 0,25× à 4×, échelle « ×2 / ÷2 »).
  // Clic sur le fond : ajoute un point sur la courbe ; double-clic (ou Suppr) sur un point : le retire.
  import { curveAt, type Piece, type SpeedPt } from "$lib/studio/model";
  import { t } from "$lib/i18n.svelte";

  let { piece, src = null, onstart, onchange }: { piece: Piece; src?: number | null; onstart: () => void; onchange: (c: SpeedPt[], record: boolean) => void } = $props();

  const W = 280, H = 128, L = 34, R = 8, T = 8, B = 18;
  const PW = W - L - R, PH = H - T - B;
  const MIN = 0.25, MAX = 4;
  const GRID = [0.25, 0.5, 1, 2, 4];
  const span = $derived(Math.max(0.05, piece.end - piece.start));
  const X = (s: number) => L + ((s - piece.start) / span) * PW;
  const Y = (v: number) => T + ((Math.log2(MAX) - Math.log2(v)) / (Math.log2(MAX) - Math.log2(MIN))) * PH;
  const fmt = (v: number) => `${(Math.round(v * 100) / 100).toString().replace(".", ",")}×`;

  const path = $derived.by(() => {
    const n = 96;
    let d = "";
    for (let i = 0; i <= n; i++) {
      const s = piece.start + (span * i) / n;
      d += `${i ? "L" : "M"}${X(s).toFixed(1)},${Y(curveAt(piece.curve, s)).toFixed(1)}`;
    }
    return d;
  });
  const visible = $derived(piece.curve.map((k, i) => ({ k, i })).filter(({ k }) => k.t >= piece.start - 1e-3 && k.t <= piece.end + 1e-3));

  let svg: SVGSVGElement | undefined = $state();
  let drag = $state<number | null>(null);
  let focus = $state<number | null>(null);

  function at(e: PointerEvent | MouseEvent) {
    const r = svg!.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * W, y = ((e.clientY - r.top) / r.height) * H;
    const s = piece.start + Math.min(1, Math.max(0, (x - L) / PW)) * span;
    let v = Math.pow(2, Math.log2(MAX) - Math.min(1, Math.max(0, (y - T) / PH)) * (Math.log2(MAX) - Math.log2(MIN)));
    if (Math.abs(Math.log2(v)) < 0.07) v = 1; // aimanté sur la vitesse normale
    return { s, v: Math.round(v * 100) / 100 };
  }
  /** Déplace le point i (gardé entre ses voisins) */
  function place(list: SpeedPt[], i: number, s: number, v: number): SpeedPt[] {
    const lo = i > 0 ? list[i - 1].t + 0.02 : piece.start;
    const hi = i < list.length - 1 ? list[i + 1].t - 0.02 : piece.end;
    const out = list.map((k) => ({ ...k }));
    out[i] = { t: Math.min(Math.max(lo, hi), Math.max(lo, s)), v: Math.min(MAX, Math.max(MIN, v)) };
    return out;
  }
  function pointDown(e: PointerEvent, i: number) {
    e.stopPropagation();
    if (e.button !== 0) return;
    onstart();
    drag = i;
    focus = i;
    svg!.setPointerCapture(e.pointerId);
  }
  function backDown(e: PointerEvent) {
    if (e.button !== 0) return;
    // Nouveau point posé sur la courbe, à l'endroit cliqué, puis tiré tout de suite
    const { s } = at(e);
    const list = piece.curve.map((k) => ({ ...k }));
    let i = list.findIndex((k) => k.t > s);
    if (i < 0) i = list.length;
    if ((i > 0 && s - list[i - 1].t < 0.03) || (i < list.length && list[i].t - s < 0.03)) return;
    list.splice(i, 0, { t: s, v: Math.round(curveAt(piece.curve, s) * 100) / 100 });
    onstart();
    onchange(list, false);
    drag = i;
    focus = i;
    svg!.setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (drag === null) return;
    const { s, v } = at(e);
    onchange(place(piece.curve, drag, s, v), false);
  }
  function up() {
    drag = null;
  }
  function remove(i: number) {
    if (piece.curve.length <= 2) return;
    onchange(piece.curve.filter((_, j) => j !== i), true);
    focus = null;
  }
  function key(e: KeyboardEvent, i: number) {
    const k = piece.curve[i];
    if (e.key === "Delete" || e.key === "Backspace") return remove(i);
    const step = e.shiftKey ? 0.5 : 0.1;
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      onchange(place(piece.curve, i, k.t, Math.pow(2, Math.log2(k.v) + (e.key === "ArrowUp" ? step : -step))), true);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      onchange(place(piece.curve, i, k.t + (e.key === "ArrowRight" ? 1 : -1) * span * 0.02, k.v), true);
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<svg
  class="curve"
  viewBox="0 0 {W} {H}"
  bind:this={svg}
  onpointerdown={backDown}
  onpointermove={move}
  onpointerup={up}
  onpointercancel={up}
  role="application"
  aria-label={t("Courbe de vitesse")}
>
  {#each GRID as g}
    <line class="grid" class:one={g === 1} x1={L} x2={W - R} y1={Y(g)} y2={Y(g)} />
    <text class="lab" x={L - 6} y={Y(g) + 3.5} text-anchor="end">{fmt(g)}</text>
  {/each}
  <text class="lab" x={L} y={H - 4}>0 s</text>
  <text class="lab" x={W - R} y={H - 4} text-anchor="end">{span.toFixed(1).replace(".", ",")} s</text>
  {#if src !== null && src >= piece.start && src <= piece.end}
    <line class="head" x1={X(src)} x2={X(src)} y1={T} y2={T + PH} />
  {/if}
  <path class="line" d={path} />
  {#each visible as { k, i } (i)}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <g
      class="pt"
      class:on={drag === i || focus === i}
      transform="translate({X(k.t)},{Y(k.v)})"
      onpointerdown={(e) => pointDown(e, i)}
      ondblclick={() => remove(i)}
      onkeydown={(e) => key(e, i)}
      onfocus={() => (focus = i)}
      tabindex="0"
      role="slider"
      aria-label={t("Point de vitesse")}
      aria-valuenow={k.v}
      aria-valuemin={MIN}
      aria-valuemax={MAX}
    >
      <circle r="9" class="hit" />
      <circle r={drag === i ? 5.5 : 4.5} class="dot" />
      {#if drag === i || focus === i}
        <text class="val" y={k.v > 2 ? 18 : -10} text-anchor="middle">{fmt(k.v)}</text>
      {/if}
    </g>
  {/each}
</svg>

<style>
  .curve { width: 100%; height: auto; display: block; background: var(--panel); border: 1px solid var(--line); border-radius: 8px; touch-action: none; cursor: crosshair; user-select: none; }
  .grid { stroke: var(--line); stroke-width: 1; }
  .grid.one { stroke: var(--line-2); stroke-dasharray: 3 3; }
  .lab { font-size: 9px; fill: var(--text-3); font-variant-numeric: tabular-nums; }
  .head { stroke: var(--text-3); stroke-width: 1; }
  .line { fill: none; stroke: var(--hi); stroke-width: 2; stroke-linejoin: round; }
  .pt { cursor: grab; outline: none; }
  .pt .hit { fill: transparent; }
  .pt .dot { fill: var(--text); stroke: var(--bg); stroke-width: 1.5; transition: r var(--dur) var(--ease); }
  .pt.on .dot { fill: var(--hi); }
  .pt:focus-visible .dot { stroke: var(--hi); stroke-width: 2.5; }
  .val { font-size: 10px; font-weight: 600; fill: var(--text); font-variant-numeric: tabular-nums; pointer-events: none; }
</style>
