<script lang="ts">
  // Curseur du Studio : libellé, barre et valeur sur une ligne. On glisse n'importe où sur la barre,
  // double-clic = valeur par défaut, clic sur le nombre = le taper, flèches du clavier (Maj = ×10).
  import { t } from "$lib/i18n.svelte";
  let {
    label,
    value,
    min,
    max,
    step = 0.01,
    def,
    format = (v: number) => String(Math.round(v * 100) / 100),
    parse = (s: string) => parseFloat(s.replace(",", ".")),
    onstart,
    oninput,
    onend,
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    def?: number;
    format?: (v: number) => string;
    parse?: (s: string) => number;
    onstart?: () => void;
    oninput: (v: number) => void;
    onend?: () => void;
  } = $props();

  let rail: HTMLDivElement | undefined = $state();
  let dragging = $state(false);
  let editing = $state(false);
  let text = $state("");

  const pct = $derived(Math.max(0, Math.min(1, (value - min) / (max - min || 1))));
  const snap = (v: number) => {
    const s = Math.round((v - min) / step) * step + min;
    return Math.max(min, Math.min(max, +s.toFixed(6)));
  };
  function at(e: PointerEvent) {
    const r = rail!.getBoundingClientRect();
    return snap(min + ((e.clientX - r.left) / r.width) * (max - min));
  }
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragging = true;
    onstart?.();
    oninput(at(e));
  }
  function move(e: PointerEvent) {
    if (dragging) oninput(at(e));
  }
  function up() {
    if (!dragging) return;
    dragging = false;
    onend?.();
  }
  function key(e: KeyboardEvent) {
    const d = (e.shiftKey ? 10 : 1) * step;
    if (e.key === "ArrowRight" || e.key === "ArrowUp") { e.preventDefault(); onstart?.(); oninput(snap(value + d)); onend?.(); }
    else if (e.key === "ArrowLeft" || e.key === "ArrowDown") { e.preventDefault(); onstart?.(); oninput(snap(value - d)); onend?.(); }
  }
  function reset() {
    if (def === undefined) return;
    onstart?.();
    oninput(def);
    onend?.();
  }
  function edit() {
    text = format(value).replace(/[^\d.,-]/g, "");
    editing = true;
  }
  function commit() {
    const v = parse(text);
    editing = false;
    if (isFinite(v)) { onstart?.(); oninput(Math.max(min, Math.min(max, v))); onend?.(); }
  }
</script>

<div class="sl" class:dragging>
  <span class="lab">{label}</span>
  <div
    class="rail"
    bind:this={rail}
    role="slider"
    tabindex="0"
    aria-label={label}
    aria-valuemin={min}
    aria-valuemax={max}
    aria-valuenow={value}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    ondblclick={reset}
    onkeydown={key}
  >
    <i class="track"><i class="fill" style="width:{pct * 100}%"></i></i>
    {#if def !== undefined && def > min && def < max}<i class="mark" style="left:{((def - min) / (max - min)) * 100}%"></i>{/if}
    <i class="thumb" style="left:{pct * 100}%"></i>
  </div>
  {#if editing}
    <!-- svelte-ignore a11y_autofocus -->
    <input class="val edit" bind:value={text} autofocus onblur={commit} onkeydown={(e) => { if (e.key === "Enter") commit(); else if (e.key === "Escape") editing = false; }} />
  {:else}
    <button class="val mono" onclick={edit} title={t("Clique pour taper une valeur")}>{format(value)}</button>
  {/if}
</div>

<style>
  .sl { display: grid; grid-template-columns: 84px minmax(0, 1fr) 54px; align-items: center; gap: 10px; height: 30px; }
  .lab { font-size: 12px; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .rail { position: relative; height: 30px; cursor: pointer; touch-action: none; outline: none; }
  .track { position: absolute; left: 0; right: 0; top: 50%; height: 4px; margin-top: -2px; border-radius: 4px; background: var(--panel-3); overflow: hidden; transition: height 120ms var(--ease), margin 120ms var(--ease); }
  .fill { position: absolute; left: 0; top: 0; bottom: 0; background: color-mix(in srgb, var(--text) 78%, transparent); }
  .rail:hover .track, .dragging .track { height: 6px; margin-top: -3px; }
  .mark { position: absolute; top: 50%; width: 2px; height: 10px; margin: -5px 0 0 -1px; border-radius: 1px; background: var(--line-2); pointer-events: none; }
  .thumb { position: absolute; top: 50%; width: 14px; height: 14px; margin: -7px 0 0 -7px; border-radius: 50%; background: #fff; box-shadow: 0 1px 4px rgba(0, 0, 0, 0.55), 0 0 0 0 color-mix(in srgb, #fff 0%, transparent); transition: transform 140ms var(--spring), box-shadow 140ms var(--ease); pointer-events: none; }
  .rail:hover .thumb { transform: scale(1.1); }
  .dragging .thumb { transform: scale(1.2); box-shadow: 0 1px 4px rgba(0, 0, 0, 0.55), 0 0 0 5px color-mix(in srgb, #fff 14%, transparent); }
  .rail:focus-visible .thumb { box-shadow: 0 0 0 3px color-mix(in srgb, var(--hi) 45%, transparent); }
  .val { justify-self: end; min-width: 0; max-width: 54px; height: 24px; padding: 0 6px; border-radius: 6px; font-size: 11.5px; color: var(--text); text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; transition: background var(--dur) var(--ease); }
  .val:hover { background: var(--panel-3); }
  .val.edit { width: 54px; background: var(--bg); border: 1px solid var(--hi); outline: none; user-select: text; font-family: var(--mono); }
</style>
