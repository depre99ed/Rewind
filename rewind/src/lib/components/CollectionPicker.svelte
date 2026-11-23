<script lang="ts">
  // Choix des collections pour un ou plusieurs clips : chercher, cocher / décocher, créer.
  // Chaque clic est enregistré tout de suite (aucun bouton « valider » à oublier).
  import { Check, Minus, Plus, Search, FolderOpen } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n.svelte";

  let { ids, onclose }: { ids: string[]; onclose: () => void } = $props();

  let q = $state("");
  let busy = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let root: HTMLDivElement | undefined = $state();
  let active = $state(0);

  const clean = (s: string) => s.split(/\s+/).filter(Boolean).join(" ").slice(0, 40);
  const chosen = $derived(app.clips.filter((c) => ids.includes(c.id)));
  /** Combien des clips choisis sont déjà dans chaque collection */
  const rows = $derived(
    app.collections
      .filter((c) => !q.trim() || c.name.toLowerCase().includes(q.trim().toLowerCase()))
      .map((c) => ({ ...c, inside: chosen.filter((x) => x.tags.some((t) => t.toLowerCase() === c.name.toLowerCase())).length })),
  );
  const canCreate = $derived(!!clean(q) && !app.collections.some((c) => c.name.toLowerCase() === clean(q).toLowerCase()));
  const total = $derived(rows.length + (canCreate ? 1 : 0));

  $effect(() => { if (active >= total) active = Math.max(0, total - 1); });

  // Position fixe près du bouton qui l'ouvre (le parent), toujours entière dans la fenêtre :
  // au-dessus s'il y a la place, sinon en dessous ; jamais coupée par un panneau qui défile.
  let pos = $state("visibility:hidden");
  $effect(() => {
    if (!root) return;
    const el = root;
    // Le parent sert d'ancre ; le menu lui-même va dans <body> (un parent transformé ou vitré
    // fausserait la position fixe et un panneau qui défile le couperait)
    const anchor = el.parentElement;
    document.body.appendChild(el);
    const place = () => {
      const a = anchor?.getBoundingClientRect();
      if (!a) return;
      const w = el.offsetWidth, h = el.offsetHeight, m = 8;
      const top = a.top - h - m >= m ? a.top - h - m : Math.min(a.bottom + m, innerHeight - h - m);
      const left = Math.max(m, Math.min(a.left, innerWidth - w - m));
      pos = `left:${Math.round(left)}px;top:${Math.round(Math.max(m, top))}px`;
    };
    place();
    // Focus une fois le menu visible (déplacé + positionné)
    requestAnimationFrame(() => input?.focus());
    const ro = new ResizeObserver(place);
    ro.observe(el);
    addEventListener("resize", place);
    addEventListener("scroll", place, true);
    return () => { ro.disconnect(); removeEventListener("resize", place); removeEventListener("scroll", place, true); el.remove(); };
  });

  // Fermeture : clic en dehors ou Échap
  $effect(() => {
    const down = (e: PointerEvent) => { if (root && !root.contains(e.target as Node) && !(e.target as HTMLElement).closest?.("[data-collection-toggle]")) onclose(); };
    window.addEventListener("pointerdown", down, true);
    return () => window.removeEventListener("pointerdown", down, true);
  });

  async function apply(name: string, add: boolean) {
    if (busy || !ids.length) return;
    busy = true;
    try {
      const changed = await api.setCollection(ids, name, add);
      app.updateClips(changed);
      const n = changed.length;
      const target = clean(name);
      if (add) app.toast("ok", t("Ajouté à « {x} »", { x: target }), n ? t(n > 1 ? "{n} clips" : "{n} clip", { n }) : t("Déjà dans cette collection."));
      else app.toast("info", t("Retiré de « {x} »", { x: target }), t(n > 1 ? "{n} clips" : "{n} clip", { n }));
      q = "";
    } catch (e) {
      app.toast("err", t("Collection non modifiée"), String(e));
    } finally {
      busy = false;
      input?.focus();
    }
  }
  function toggle(r: { name: string; inside: number }) { apply(r.name, r.inside < chosen.length); }
  function choose(i: number) {
    if (i < rows.length) toggle(rows[i]);
    else if (canCreate) apply(q, true);
  }
  function key(e: KeyboardEvent) {
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onclose(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); active = Math.min(total - 1, active + 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); active = Math.max(0, active - 1); }
    else if (e.key === "Enter") {
      e.preventDefault();
      // Nom tapé en entier : on ajoute toujours (jamais de retrait par surprise) ; sinon la ligne en surbrillance
      const exact = rows.find((r) => r.name.toLowerCase() === clean(q).toLowerCase());
      if (exact) apply(exact.name, true);
      else choose(q.trim() && !rows.length ? rows.length : active);
    }
  }
</script>

<div class="picker" style={pos} bind:this={root} role="dialog" aria-label={t("Collections")} tabindex="-1" onkeydown={key}>
  <div class="head">
    <span>{t(chosen.length > 1 ? "Collections de {n} clips" : "Collections du clip", { n: chosen.length })}</span>
  </div>
  <label class="search"><Search size={14} /><input bind:this={input} bind:value={q} placeholder={t("Chercher ou créer une collection")} maxlength="40" oninput={() => (active = 0)} /></label>
  <div class="list" role="listbox">
    {#each rows as r, i (r.name)}
      {@const state = r.inside === 0 ? "off" : r.inside === chosen.length ? "on" : "some"}
      <button class="row" class:active={i === active} role="option" aria-selected={state === "on"} disabled={busy} onclick={() => toggle(r)} onpointerenter={() => (active = i)}>
        <i class="box {state}">{#if state === "on"}<Check size={12} strokeWidth={3} />{:else if state === "some"}<Minus size={12} strokeWidth={3} />{/if}</i>
        <span class="name">{r.name}</span>
        <span class="count mono">{r.count}</span>
      </button>
    {/each}
    {#if canCreate}
      <button class="row create" class:active={active === rows.length} role="option" aria-selected="false" disabled={busy} onclick={() => apply(q, true)} onpointerenter={() => (active = rows.length)}>
        <i class="box add"><Plus size={13} /></i>
        <span class="name">{t("Créer « {x} »", { x: clean(q) })}</span>
      </button>
    {/if}
    {#if !rows.length && !canCreate}
      <p class="empty"><FolderOpen size={14} />{t("Tape un nom pour créer ta première collection.")}</p>
    {/if}
  </div>
  <p class="hint">{t("Entrée pour choisir · Échap pour fermer")}</p>
</div>

<style>
  .picker { position: fixed; width: 280px; z-index: 60; padding: 8px; border-radius: 12px; background: var(--panel-2); border: 1px solid var(--line-2); box-shadow: var(--shadow); display: flex; flex-direction: column; gap: 6px; animation: pop-in 160ms var(--ease) both; }
  @keyframes pop-in { from { opacity: 0; scale: 0.97; } }
  @media (prefers-reduced-motion: reduce) { .picker { animation: none; } }
  .head { padding: 2px 4px; font-size: 12px; color: var(--text-3); }
  .search { display: flex; align-items: center; gap: 8px; height: 34px; padding: 0 10px; border-radius: 8px; background: var(--bg); border: 1px solid var(--line); color: var(--text-3); }
  .search:focus-within { border-color: var(--line-2); }
  .search input { flex: 1; min-width: 0; background: transparent; border: 0; outline: none; color: var(--text); font-size: 13px; user-select: text; }
  .list { display: flex; flex-direction: column; gap: 1px; max-height: 240px; overflow-y: auto; }
  .row { display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 8px; border-radius: 8px; font-size: 13px; text-align: left; color: var(--text); }
  .row.active { background: var(--panel-3); }
  .row:disabled { opacity: 0.6; }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { font-size: 11.5px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .box { flex: none; width: 18px; height: 18px; border-radius: 5px; display: grid; place-items: center; border: 1.5px solid var(--line-2); color: var(--hi-ink, #0b0c0f); }
  .box.on, .box.some { background: var(--hi); border-color: var(--hi); }
  .box.add { border-style: dashed; color: var(--text-2); }
  .create .name { color: var(--text-2); }
  .empty { display: flex; align-items: center; gap: 8px; padding: 8px; margin: 0; font-size: 12.5px; color: var(--text-3); }
  .hint { margin: 0; padding: 4px 4px 0; border-top: 1px solid var(--line); font-size: 11px; color: var(--text-3); }
</style>
