<script lang="ts">
  // Studio v2 : disposition façon CapCut (médias à gauche, aperçu au centre, réglages à droite, pistes en bas).
  import { onMount } from "svelte";
  import { Download, FolderOpen, Plus, Trash2, CloudCheck, ChevronDown, Film } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { studio, fmtTime } from "$lib/studio/store.svelte";
  import { fileUrl, relativeDate } from "$lib/api";
  import { fit } from "$lib/fit";
  import { t } from "$lib/i18n.svelte";
  import "$lib/studio/fonts";
  import MediaPanel from "./studio/MediaPanel.svelte";
  import Preview from "./studio/Preview.svelte";
  import Inspector from "./studio/Inspector.svelte";
  import Timeline from "./studio/Timeline.svelte";
  import ExportDialog from "./studio/ExportDialog.svelte";

  let exportOpen = $state(typeof window !== "undefined" && !!(window as any).__REWIND_MOCK__ && new URLSearchParams(location.search).get("export") === "1");
  let projectsOpen = $state(false);
  let menuEl = $state<HTMLDivElement>();
  let menuBtn = $state<HTMLButtonElement>();
  const P = $derived(studio.project);

  onMount(() => {
    studio.init();
    return () => {
      studio.playing = false;
      studio.saveNow();
    };
  });

  async function openProjects() {
    projectsOpen = !projectsOpen;
    if (projectsOpen) await studio.saveNow().then(() => studio.refreshList());
  }
  async function del(id: string, name: string) {
    if (!confirm(t("Supprimer le projet « {x} » ? Les clips ne sont pas touchés.", { x: name }))) return;
    await studio.remove(id);
  }

  function key(e: KeyboardEvent) {
    if (app.view !== "studio" || exportOpen || app.openClip) return;
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (e.key === " ") {
      e.preventDefault();
      if (!studio.playing && studio.t >= studio.total - 0.05) studio.t = 0;
      studio.playing = !studio.playing && studio.total > 0;
    } else if (ctrl && k === "z") { e.preventDefault(); e.shiftKey ? studio.redo() : studio.undo(); }
    else if (ctrl && k === "y") { e.preventDefault(); studio.redo(); }
    else if (ctrl && k === "d") { e.preventDefault(); studio.duplicate(); }
    else if (ctrl && k === "s") { e.preventDefault(); studio.saveNow(); }
    else if (!ctrl && k === "s") studio.split();
    else if (e.key === "Delete" || e.key === "Backspace") studio.removeSel();
    else if (e.key === "ArrowLeft") { e.preventDefault(); studio.playing = false; studio.seek(studio.t - (e.shiftKey ? 1 : 1 / 30)); }
    else if (e.key === "ArrowRight") { e.preventDefault(); studio.playing = false; studio.seek(studio.t + (e.shiftKey ? 1 : 1 / 30)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); studio.jumpCut(-1); }
    else if (e.key === "ArrowDown") { e.preventDefault(); studio.jumpCut(1); }
    else if (e.key === "Home") studio.seek(0);
    else if (e.key === "End") studio.seek(studio.total);
    else if (e.key === "+" || e.key === "=") studio.zoom = Math.min(60, studio.zoom * 1.3);
    else if (e.key === "-") studio.zoom = Math.max(1, studio.zoom / 1.3);
    else if (e.key === "Escape") { studio.junction = null; studio.select(null); projectsOpen = false; }
  }
  function outside(e: MouseEvent) {
    if (!projectsOpen) return;
    const n = e.target as Node;
    if (menuEl?.contains(n) || menuBtn?.contains(n)) return;
    projectsOpen = false;
  }
</script>

<svelte:window onkeydown={key} onmousedown={outside} />

<section class="studio">
  <header class="top">
    <button bind:this={menuBtn} class="btn small ghost proj" class:on={projectsOpen} onclick={openProjects} aria-expanded={projectsOpen}><FolderOpen size={15} />{t("Projets")}<ChevronDown size={13} /></button>
    <input class="title" value={P.name} onchange={(e) => studio.rename(e.currentTarget.value.trim())} placeholder={t("Nom du montage")} aria-label={t("Nom du montage")} />
    <span class="save faint" title={t("Le montage est gardé même si tu fermes Rewind")}>
      {#if studio.saved}<CloudCheck size={14} />{t("Enregistré")}{:else}{t("Enregistrement…")}{/if}
    </span>
    <span class="spacer"></span>
    <span class="faint mono dur">{fmtTime(studio.total)}</span>
    <button class="btn primary exp" onclick={() => (exportOpen = true)} disabled={!P.pieces.length && !P.tracks.length}><Download size={16} />{t("Exporter")}</button>

    {#if projectsOpen}
      <div class="projects glass" bind:this={menuEl}>
        <button class="newp" onclick={() => { studio.create(); projectsOpen = false; }}><Plus size={15} />{t("Nouveau projet")}</button>
        <div class="plist">
          {#each studio.projects as pr (pr.id)}
            <div class="pitem" class:on={pr.id === P.id}>
              <button class="popen" onclick={() => { studio.open(pr.id); projectsOpen = false; }}>
                <span class="pthumb">{#if pr.thumb}<img use:fit src={fileUrl(pr.thumb)} alt="" />{:else}<Film size={16} />{/if}</span>
                <span class="ptext"><strong>{pr.name}</strong><small class="faint">{fmtTime(pr.duration)} · {relativeDate(pr.updated)}</small></span>
              </button>
              <button class="pdel" onclick={() => del(pr.id, pr.name)} aria-label={t("Supprimer le projet")} title={t("Supprimer le projet")}><Trash2 size={14} /></button>
            </div>
          {:else}
            <p class="note">{t("Aucun projet enregistré pour l'instant.")}</p>
          {/each}
        </div>
      </div>
    {/if}
  </header>

  <div class="work">
    <MediaPanel />
    <Preview />
    <Inspector />
  </div>

  <div class="bottom">
    <Timeline />
  </div>
</section>

{#if exportOpen}
  <ExportDialog onClose={() => (exportOpen = false)} />
{/if}

<style>
  .studio { height: 100%; display: grid; grid-template-rows: auto minmax(0, 1fr) 270px; grid-template-columns: minmax(0, 1fr); background: var(--bg); }
  .studio > * { min-width: 0; }
  .top { position: relative; display: flex; align-items: center; gap: 10px; padding: 8px 14px; border-bottom: 1px solid var(--line); background: var(--bg-2); }
  .proj.on { color: var(--text); background: var(--panel-3); }
  .title { flex: 0 1 320px; min-width: 80px; height: 32px; padding: 0 10px; border-radius: 8px; border: 1px solid transparent; background: transparent; outline: none; font-weight: 600; font-size: 14px; color: var(--text); user-select: text; transition: border-color var(--dur), background var(--dur); }
  .title:hover { border-color: var(--line); }
  .title:focus { border-color: var(--hi); background: var(--bg); }
  .save { display: inline-flex; align-items: center; gap: 5px; font-size: 11.5px; white-space: nowrap; }
  .spacer { flex: 1; }
  .dur { font-size: 12px; }
  .exp { height: 34px; padding: 0 16px; }
  .work { display: grid; grid-template-columns: 300px minmax(0, 1fr) 290px; min-height: 0; }
  @media (max-width: 1280px) { .work { grid-template-columns: 270px minmax(0, 1fr) 260px; } }
  .bottom { min-height: 0; }
  /* Liste des projets : flotte au-dessus du contenu */
  .projects { position: absolute; left: 12px; top: calc(100% + 6px); z-index: 40; width: 340px; max-height: 420px; display: flex; flex-direction: column; gap: 6px; padding: 8px; border-radius: 14px; animation: pin 180ms var(--ease); }
  @keyframes pin { from { opacity: 0; transform: translateY(-4px); } }
  @media (prefers-reduced-motion: reduce) { .projects { animation: none; } }
  .projects > * { position: relative; z-index: 2; }
  .newp { display: flex; align-items: center; gap: 8px; height: 36px; padding: 0 10px; border-radius: 9px; font-size: 13px; font-weight: 500; color: var(--text); background: color-mix(in srgb, var(--text) 7%, transparent); }
  .newp:hover { background: color-mix(in srgb, var(--text) 11%, transparent); }
  .plist { display: flex; flex-direction: column; gap: 2px; overflow-y: auto; }
  .pitem { display: flex; align-items: center; border-radius: 9px; }
  .pitem:hover, .pitem.on { background: color-mix(in srgb, var(--text) 6%, transparent); }
  .popen { flex: 1; min-width: 0; display: flex; align-items: center; gap: 10px; padding: 6px; text-align: left; }
  .pthumb { width: 64px; height: 36px; flex: none; border-radius: 6px; overflow: hidden; display: grid; place-items: center; background: #0d0e12; color: var(--text-3); }
  .pthumb img { width: 100%; height: 100%; object-fit: cover; }
  .ptext { display: flex; flex-direction: column; min-width: 0; }
  .ptext strong { font-size: 12.5px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ptext small { font-size: 11px; }
  .pdel { width: 28px; height: 28px; margin-right: 4px; border-radius: 7px; display: grid; place-items: center; color: var(--text-3); opacity: 0; }
  .pitem:hover .pdel { opacity: 1; }
  .pdel:hover { color: var(--rec); background: color-mix(in srgb, var(--rec) 12%, transparent); }
  .note { margin: 6px; font-size: 12px; color: var(--text-3); }
</style>
