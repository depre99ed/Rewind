<script lang="ts">
  // Saisie d'un raccourci : touche seule (F8), combinaison (Alt+F10) ou bouton de souris (Souris 4).
  // Pendant la saisie, les raccourcis de Rewind sont en pause (sinon F8 lancerait un clip au lieu
  // d'être enregistré) ; le moteur ne réserve rien auprès de Windows, donc toutes les touches arrivent ici.
  import { onDestroy } from "svelte";
  import { api } from "../api";
  import { t } from "../i18n.svelte";
  import { keyParts } from "../keys";

  let { value, onchange }: { value: string; onchange: (v: string) => void } = $props();
  let listening = $state(false);
  let hint = $state("");

  const names: Record<string, string> = {
    " ": "Space", ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    Escape: "Escape", Enter: "Enter", Backspace: "Backspace", Tab: "Tab", Delete: "Delete", Insert: "Insert",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown", PrintScreen: "PrintScreen", Pause: "Pause",
    ScrollLock: "ScrollLock",
  };
  /** Touches acceptées seules : elles ne servent pas à écrire du texte. */
  const alone = /^(F\d{1,2}|Pause|PrintScreen|Insert|Home|End|PageUp|PageDown|ScrollLock|Num.+|Mouse4|Mouse5|MouseMiddle)$/;

  function mods(e: KeyboardEvent | MouseEvent) {
    return [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win"].filter(Boolean) as string[];
  }

  function finish(combo: string) {
    stop();
    onchange(combo);
  }

  function accept(e: KeyboardEvent | MouseEvent, k: string) {
    const m = mods(e);
    if (m.length === 0 && !alone.test(k)) {
      hint = t("Ajoute Ctrl, Alt ou Maj : une lettre seule gênerait la frappe");
      return;
    }
    finish([...m, k].join("+"));
  }

  function key(e: KeyboardEvent) {
    if (!listening) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) { stop(); return; }
    if (["Control", "Shift", "Alt", "Meta", "AltGraph"].includes(e.key)) return;
    let k = names[e.key] ?? "";
    if (!k) {
      if (/^F\d{1,2}$/.test(e.key)) k = e.key;
      else if (e.code.startsWith("Key")) k = e.code.slice(3);
      else if (e.code.startsWith("Digit")) k = e.code.slice(5);
      else if (e.code.startsWith("Numpad")) k = "Num" + e.code.slice(6);
      else k = e.code;
    }
    if (!k) return;
    accept(e, k);
  }

  // Boutons de souris : molette (1), latéraux (3 = précédent, 4 = suivant)
  function mouse(e: MouseEvent) {
    if (!listening) return;
    const k = e.button === 1 ? "MouseMiddle" : e.button === 3 ? "Mouse4" : e.button === 4 ? "Mouse5" : "";
    if (!k) return;
    e.preventDefault();
    e.stopPropagation();
    accept(e, k);
  }
  // Empêche « Précédent / Suivant » de la souris de changer de page pendant la saisie
  function block(e: MouseEvent) {
    if (listening && (e.button === 1 || e.button === 3 || e.button === 4)) e.preventDefault();
  }

  function start() {
    listening = true;
    hint = "";
    api.hotkeysPause(true).catch(() => {});
    window.addEventListener("mousedown", mouse, true);
    window.addEventListener("mouseup", block, true);
    window.addEventListener("auxclick", block, true);
  }
  function stop() {
    if (!listening) return;
    listening = false;
    hint = "";
    api.hotkeysPause(false).catch(() => {});
    window.removeEventListener("mousedown", mouse, true);
    window.removeEventListener("mouseup", block, true);
    window.removeEventListener("auxclick", block, true);
  }
  onDestroy(stop);

</script>

<div class="hk" class:on={listening}>
  <button class="value" onclick={() => (listening ? stop() : start())} onkeydown={key} onblur={stop}>
    {#if listening}
      <span class="wait">{hint || t("Appuie sur une touche ou un bouton de souris…")}</span>
    {:else if value}
      {#each keyParts(value) as k, i}{#if i}<span class="plus">+</span>{/if}<span class="kbd">{k}</span>{/each}
    {:else}
      <span class="faint">{t("Aucun")}</span>
    {/if}
  </button>
  {#if value && !listening}<button class="clear faint" onclick={() => onchange("")} aria-label={t("Effacer")}>{t("Effacer")}</button>{/if}
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
