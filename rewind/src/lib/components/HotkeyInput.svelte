<script lang="ts">
  let { value, onchange }: { value: string; onchange: (v: string) => void } = $props();
  let listening = $state(false);

  const names: Record<string, string> = {
    " ": "Space", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    Escape: "Escape", Enter: "Enter", Backspace: "Backspace", Tab: "Tab", Delete: "Delete", Insert: "Insert",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown", PrintScreen: "PrintScreen", Pause: "Pause",
  };

  function key(e: KeyboardEvent) {
    if (!listening) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") { listening = false; return; }
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    let k = names[e.key] ?? "";
    if (!k) {
      if (/^F\d{1,2}$/.test(e.key)) k = e.key;
      else if (e.code.startsWith("Key")) k = e.code.slice(3);
      else if (e.code.startsWith("Digit")) k = e.code.slice(5);
      else if (e.code.startsWith("Numpad")) k = "Num" + e.code.slice(6);
      else k = e.code;
    }
    const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift"].filter(Boolean) as string[];
    if (mods.length === 0 && !/^F\d{1,2}$/.test(k) && !["Pause", "PrintScreen", "Insert"].includes(k)) {
      return; // une touche seule (lettre) gênerait la frappe : on exige un modificateur
    }
    listening = false;
    onchange([...mods, k].join("+"));
  }
</script>

<div class="hk" class:on={listening}>
  <button class="value" onclick={() => (listening = !listening)} onkeydown={key} onblur={() => (listening = false)}>
    {#if listening}
      <span class="wait">Appuie sur une combinaison…</span>
    {:else if value}
      {#each value.split("+") as k, i}{#if i}<span class="plus">+</span>{/if}<span class="kbd">{k}</span>{/each}
    {:else}
      <span class="faint">Aucun</span>
    {/if}
  </button>
  {#if value && !listening}<button class="clear faint" onclick={() => onchange("")} aria-label="Effacer">Effacer</button>{/if}
</div>

<style>
  .hk { display: flex; align-items: center; gap: 8px; }
  .value { min-width: 190px; height: 36px; padding: 0 10px; border-radius: 8px; background: var(--bg-2); border: 1px solid var(--line); display: inline-flex; align-items: center; gap: 4px; transition: border-color var(--dur); }
  .value:hover { border-color: var(--line-2); }
  .on .value { border-color: var(--hi); box-shadow: 0 0 0 3px color-mix(in srgb, var(--hi) 20%, transparent); }
  .wait { font-size: 12.5px; color: var(--hi); }
  :root[data-theme="light"] .wait { color: color-mix(in srgb, var(--hi) 55%, #000); }
  .plus { color: var(--text-3); font-size: 11px; }
  .clear { font-size: 12px; }
  .clear:hover { color: var(--text); }
</style>
