<script lang="ts">
  // Touches clavier / souris à l'écran, allumées selon la position de lecture.
  // Même disposition que celle incrustée par l'export du Studio (editor.rs).
  import type { InputEvent } from "$lib/api";
  import { t } from "$lib/i18n.svelte";

  let { events, time, compact = false }: { events: InputEvent[]; time: number; compact?: boolean } = $props();

  /** Code virtuel → libellé. Disposition QWERTY physique (en AZERTY : Z, Q, S, D). */
  const ROWS: { k: number; label: string; w?: number }[][] = [
    [{ k: 0x09, label: "Tab", w: 1.4 }, { k: 0x51, label: "Q" }, { k: 0x57, label: "W" }, { k: 0x45, label: "E" }, { k: 0x52, label: "R" }],
    [{ k: 0xa0, label: "Maj", w: 1.4 }, { k: 0x41, label: "A" }, { k: 0x53, label: "S" }, { k: 0x44, label: "D" }, { k: 0x46, label: "F" }],
    [{ k: 0xa2, label: "Ctrl", w: 1.4 }, { k: 0x20, label: "Espace", w: 3.1 }, { k: 0x43, label: "C" }],
  ];

  // Par touche : appuis / relâchements triés (calculé une fois par clip)
  const byKey = $derived.by(() => {
    const m = new Map<number, InputEvent[]>();
    for (const e of [...events].sort((a, b) => a.t - b.t)) {
      if (!m.has(e.key)) m.set(e.key, []);
      m.get(e.key)!.push(e);
    }
    return m;
  });

  function down(k: number, t: number): boolean {
    const list = byKey.get(k);
    if (!list) return false;
    // Dernier événement avant t (recherche dichotomique)
    let lo = 0, hi = list.length - 1, ans = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (list[mid].t <= t) { ans = mid; lo = mid + 1; } else hi = mid - 1;
    }
    return ans >= 0 && list[ans].down;
  }
</script>

<div class="keys" class:compact aria-hidden="true">
  <div class="board">
    {#each ROWS as row}
      <div class="row">
        {#each row as key}
          <span class="key" class:on={down(key.k, time)} style="--w:{key.w ?? 1}">{t(key.label)}</span>
        {/each}
      </div>
    {/each}
  </div>
  <svg class="mouse" viewBox="0 0 40 60">
    <rect x="2" y="2" width="36" height="56" rx="18" class="body" />
    <path d="M20 2 A18 18 0 0 0 2 20 L2 24 L20 24 Z" class="btn" class:on={down(0x01, time)} />
    <path d="M20 2 A18 18 0 0 1 38 20 L38 24 L20 24 Z" class="btn" class:on={down(0x02, time)} />
    <rect x="17" y="8" width="6" height="11" rx="3" class="wheel" class:on={down(0x04, time)} />
  </svg>
</div>

<style>
  .keys { position: absolute; left: 14px; bottom: 14px; display: flex; align-items: flex-end; gap: 10px; padding: 10px; border-radius: 14px; background: rgba(8, 9, 12, 0.45); backdrop-filter: blur(6px); pointer-events: none; z-index: 4; --u: 30px; }
  .keys.compact { --u: 22px; padding: 7px; gap: 7px; }
  .board { display: flex; flex-direction: column; gap: 4px; }
  .row { display: flex; gap: 4px; }
  .key { width: calc(var(--u) * var(--w)); height: var(--u); border-radius: 7px; display: grid; place-items: center; font: 700 calc(var(--u) * 0.36) / 1 var(--font); color: rgba(255, 255, 255, 0.85); background: rgba(255, 255, 255, 0.1); border: 1px solid rgba(255, 255, 255, 0.16); transition: background 60ms, color 60ms, transform 60ms; }
  .key.on { background: #fff; color: #0b0c0f; transform: translateY(1px); box-shadow: 0 0 12px rgba(255, 255, 255, 0.45); }
  .mouse { width: calc(var(--u) * 1.5); height: calc(var(--u) * 2.25); }
  .mouse .body { fill: rgba(255, 255, 255, 0.1); stroke: rgba(255, 255, 255, 0.25); stroke-width: 1.5; }
  .mouse .btn { fill: transparent; stroke: rgba(255, 255, 255, 0.25); stroke-width: 1.2; transition: fill 60ms; }
  .mouse .btn.on { fill: #fff; }
  .mouse .wheel { fill: rgba(255, 255, 255, 0.3); }
  .mouse .wheel.on { fill: #fff; }
</style>
