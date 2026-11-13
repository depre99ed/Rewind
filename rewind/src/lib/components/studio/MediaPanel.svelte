<script lang="ts">
  import { Search, Plus, Music, Upload, X, Blend, Film, Clock3 } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { studio, TRANSITIONS, pickMusic, useMusic, recentMusic, fmtTime } from "$lib/studio.svelte";
  import { fileUrl, formatDuration, gameTone, badges, type Clip } from "$lib/api";
  import { t } from "$lib/i18n.svelte";

  let tab = $state<"clips" | "music" | "transitions">("clips");
  let query = $state("");
  let game = $state<string | null>(null);
  let recent = $state(recentMusic());

  const usable = $derived(app.clips.filter((c) => c.kind !== "screenshot" && c.duration > 0.5));
  const games = $derived.by(() => {
    const m = new Map<string, number>();
    for (const c of usable) m.set(c.game || "Bureau", (m.get(c.game || "Bureau") ?? 0) + 1);
    return [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, 6).map(([g]) => g);
  });
  const list = $derived(
    usable.filter(
      (c) =>
        (!game || (c.game || "Bureau") === game) &&
        (!query || `${c.title} ${c.game}`.toLowerCase().includes(query.toLowerCase())),
    ),
  );
  const inMontage = $derived(new Set(app.studio.map((p) => p.clipId)));

  function dragStart(e: DragEvent, c: Clip) {
    if (!e.dataTransfer) return;
    e.dataTransfer.setData("application/x-rewind-clip", c.id);
    e.dataTransfer.effectAllowed = "copy";
  }

  /** Jonction visée : celle choisie sur la frise, sinon celle après le morceau sélectionné. */
  const target = $derived(
    studio.junction ?? (studio.selIndex >= 0 && studio.selIndex < app.studio.length - 1 ? studio.selIndex + 1 : null),
  );

  async function importMusic() {
    await pickMusic();
    recent = recentMusic();
  }
</script>

<aside class="panel">
  <nav class="tabs">
    <button class:on={tab === "clips"} onclick={() => (tab = "clips")}><Film size={15} />{t("Clips")}</button>
    <button class:on={tab === "music"} onclick={() => { tab = "music"; recent = recentMusic(); }}><Music size={15} />{t("Musique")}</button>
    <button class:on={tab === "transitions"} onclick={() => (tab = "transitions")}><Blend size={15} />{t("Transitions")}</button>
  </nav>

  {#if tab === "clips"}
    <div class="head">
      <label class="search"><Search size={14} /><input placeholder={t("Rechercher un clip…")} bind:value={query} /></label>
      {#if games.length > 1}
        <div class="games">
          <button class="g" class:on={!game} onclick={() => (game = null)}>{t("Tous")}</button>
          {#each games as g}
            <button class="g" class:on={game === g} onclick={() => (game = game === g ? null : g)}><i style="background:{gameTone(g)}"></i>{g}</button>
          {/each}
        </div>
      {/if}
    </div>
    <div class="grid">
      {#each list as c (c.id)}
        <div class="card" draggable="true" ondragstart={(e) => dragStart(e, c)} role="listitem" title={t("Glisse sur la frise, ou clique sur +")}>
          <div class="thumb" style="--c:{gameTone(c.game)}">
            {#if c.thumb}<img src={fileUrl(c.thumb)} alt="" loading="lazy" draggable="false" />{/if}
            <em class="mono">{formatDuration(c.duration)}</em>
            {#if inMontage.has(c.id)}<span class="used">{t("Dans le montage")}</span>{/if}
            <button class="add" onclick={() => studio.insertClips([c])} aria-label={t("Ajouter au montage")}><Plus size={16} /></button>
          </div>
          <strong>{c.title}</strong>
          <small>
            <i style="background:{gameTone(c.game)}"></i>{c.game || t("Bureau")}
            {#each badges(c).slice(0, 2) as b}<span class="tag" class:hot={b.hot}>{b.label}</span>{/each}
          </small>
        </div>
      {:else}
        <p class="empty faint">{t("Aucun clip trouvé.")}</p>
      {/each}
    </div>
  {:else if tab === "music"}
    <div class="pad">
      <button class="btn primary wide" onclick={importMusic}><Upload size={16} />{t("Importer une musique…")}</button>
      <p class="faint note">{t("MP3, WAV, OGG, M4A, FLAC. Elle se répète si elle est plus courte que le montage, et baisse toute seule quand le jeu ou la voix sont forts (réglable).")}</p>
      {#if app.studioMusic}
        <div class="current">
          <span class="disc"><Music size={16} /></span>
          <div class="ctext"><strong>{app.studioMusic.name}</strong><small class="faint mono">{fmtTime(app.studioMusic.duration)} · {Math.round(app.studioMusic.volume * 100)} %</small></div>
          <button class="btn small ghost" onclick={() => { studio.musicSelected = true; studio.selected = null; }}>{t("Régler")}</button>
          <button class="btn small icon ghost" onclick={() => studio.setMusic(null)} aria-label={t("Retirer la musique")}><X size={15} /></button>
        </div>
      {/if}
      {#if recent.length}
        <h4><Clock3 size={13} />{t("Récentes")}</h4>
        {#each recent as m (m.path)}
          <button class="recent" class:on={app.studioMusic?.path === m.path} onclick={() => useMusic(m.path)}>
            <Music size={14} /><span>{m.name}</span><small class="mono faint">{fmtTime(m.duration)}</small>
          </button>
        {/each}
      {/if}
    </div>
  {:else}
    <div class="pad">
      <p class="faint note">
        {#if target !== null}
          {t("Choisis la transition entre les morceaux {a} et {b}.", { a: target, b: target + 1 })}
        {:else}
          {t("Sélectionne un morceau sur la frise (ou clique sur un rond entre deux morceaux), puis choisis une transition.")}
        {/if}
      </p>
      <div class="tgrid">
        {#each TRANSITIONS as tr}
          <button
            class="tcard"
            class:on={target !== null && app.studio[target - 1]?.transition === tr.id}
            disabled={target === null}
            onclick={() => target !== null && studio.setTransition(target, tr.id)}
          >
            <span class="tp {tr.id}"><i></i><i></i></span>
            <b>{t(tr.label)}</b>
            <small>{t(tr.hint)}</small>
          </button>
        {/each}
      </div>
      {#if app.studio.length > 1}
        <div class="all">
          <span class="faint">{t("Partout :")}</span>
          {#each TRANSITIONS.slice(0, 4) as tr}
            <button class="chip small" onclick={() => studio.setTransitionAll(tr.id)}>{t(tr.label)}</button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</aside>

<style>
  .panel { display: flex; flex-direction: column; min-height: 0; background: var(--bg-2); border-right: 1px solid var(--line); }
  .tabs { display: grid; grid-template-columns: repeat(3, 1fr); gap: 2px; padding: 8px; border-bottom: 1px solid var(--line); }
  .tabs button { height: 34px; border-radius: 9px; display: flex; align-items: center; justify-content: center; gap: 6px; font-size: 12.5px; font-weight: 600; color: var(--text-3); transition: all var(--dur); }
  .tabs button:hover { color: var(--text); background: var(--panel-2); }
  .tabs button.on { color: var(--text); background: var(--panel-3); }
  .head { padding: 10px 10px 6px; display: flex; flex-direction: column; gap: 8px; }
  .search { display: flex; align-items: center; gap: 8px; height: 34px; padding: 0 10px; border-radius: 9px; background: var(--panel); border: 1px solid var(--line); color: var(--text-3); }
  .search input { flex: 1; border: 0; outline: 0; background: transparent; user-select: text; min-width: 0; }
  .games { display: flex; flex-wrap: wrap; gap: 4px; }
  .g { height: 24px; padding: 0 8px; border-radius: 99px; font-size: 11.5px; color: var(--text-2); border: 1px solid var(--line); display: inline-flex; align-items: center; gap: 5px; max-width: 140px; overflow: hidden; white-space: nowrap; }
  .g i { display: none; }
  .g.on { background: var(--text); color: var(--bg); border-color: var(--text); }
  .grid { flex: 1; overflow-y: auto; padding: 4px 10px 16px; display: grid; grid-template-columns: 1fr 1fr; gap: 10px 8px; align-content: start; }
  .card { display: flex; flex-direction: column; gap: 4px; min-width: 0; cursor: grab; }
  .card:active { cursor: grabbing; }
  .thumb { position: relative; aspect-ratio: 16/9; border-radius: 9px; overflow: hidden; background: var(--panel-3); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 30%, transparent); }
  .thumb img { width: 100%; height: 100%; object-fit: cover; transition: transform 300ms var(--ease); }
  .card:hover .thumb img { transform: scale(1.05); }
  .thumb em { position: absolute; right: 5px; bottom: 4px; font-style: normal; font-size: 10px; padding: 1px 5px; border-radius: 4px; background: rgba(0,0,0,.72); color: #fff; }
  .used { position: absolute; left: 5px; top: 5px; font-size: 9.5px; font-weight: 700; padding: 1px 5px; border-radius: 4px; background: var(--hi); color: var(--hi-ink); }
  .add { position: absolute; right: 5px; top: 5px; width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; background: var(--hi); color: var(--hi-ink); opacity: 0; transform: scale(.85); transition: all 160ms var(--ease); box-shadow: 0 4px 12px rgba(0,0,0,.4); }
  .card:hover .add { opacity: 1; transform: none; }
  .card strong { font-size: 12px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .card small { display: flex; align-items: center; gap: 4px; font-size: 11px; color: var(--text-3); overflow: hidden; white-space: nowrap; }
  .card small i { display: none; }
  .card .tag { height: 16px; font-size: 9.5px; padding: 0 4px; }
  .empty { grid-column: 1 / -1; padding: 12px 2px; font-size: 13px; }
  .pad { padding: 12px; display: flex; flex-direction: column; gap: 10px; overflow-y: auto; }
  .wide { width: 100%; }
  .note { font-size: 12px; margin: 0; line-height: 1.5; }
  .current { display: flex; align-items: center; gap: 10px; padding: 10px; border-radius: 12px; background: color-mix(in srgb, #7b86ff 12%, var(--panel)); border: 1px solid color-mix(in srgb, #7b86ff 40%, transparent); }
  .disc { width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; background: #7b86ff; color: #fff; flex: none; }
  .ctext { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .ctext strong { font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ctext small { font-size: 11px; }
  h4 { margin: 6px 0 0; font-size: 11.5px; text-transform: uppercase; letter-spacing: .05em; color: var(--text-3); display: flex; align-items: center; gap: 6px; }
  .recent { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-radius: 9px; text-align: left; font-size: 12.5px; color: var(--text-2); }
  .recent span { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .recent:hover { background: var(--panel-2); color: var(--text); }
  .recent.on { background: color-mix(in srgb, #7b86ff 14%, transparent); color: var(--text); }
  .tgrid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .tcard { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; padding: 10px; border-radius: 12px; border: 1px solid var(--line); text-align: left; transition: all var(--dur); }
  .tcard:hover:not(:disabled) { border-color: var(--line-2); background: var(--panel-2); }
  .tcard.on { border-color: var(--hi); background: color-mix(in srgb, var(--hi) 8%, transparent); }
  .tcard:disabled { opacity: .5; cursor: default; }
  .tcard b { font-size: 12.5px; }
  .tcard small { font-size: 11px; color: var(--text-3); line-height: 1.3; }
  .tp { position: relative; width: 100%; aspect-ratio: 16/8; border-radius: 7px; overflow: hidden; margin-bottom: 4px; }
  .tp i { position: absolute; inset: 0; }
  .tp i:first-child { background: linear-gradient(135deg, #f0a23b, #b8612a); }
  .tp i:last-child { background: linear-gradient(135deg, #5a9bff, #2b4f8f); opacity: 0; }
  .tcard:hover .tp.fade i:last-child { animation: tfade 1.3s ease-in-out infinite; }
  .tcard:hover .tp.black i:last-child { animation: tblack 1.3s ease-in-out infinite; }
  .tcard:hover .tp.white i:last-child { animation: twhite 1.3s ease-in-out infinite; }
  .tcard:hover .tp.zoom i:first-child { animation: tzoom 1.3s ease-in infinite; }
  .tcard:hover .tp.zoom i:last-child { animation: tfade 1.3s ease-in-out infinite; }
  .tcard:hover .tp.slide i:last-child { opacity: 1; animation: tslide 1.3s ease-in-out infinite; }
  .tcard:hover .tp.wipe i:last-child { opacity: 1; animation: twipe 1.3s ease-in-out infinite; }
  @keyframes tfade { 0%, 20% { opacity: 0; } 70%, 100% { opacity: 1; } }
  @keyframes tblack { 0%, 15% { opacity: 0; background: #000; } 45% { opacity: 1; background: #000; } 50% { opacity: 0; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } 100% { opacity: 1; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } }
  @keyframes twhite { 0%, 15% { opacity: 0; background: #fff; } 45% { opacity: 1; background: #fff; } 50% { opacity: 1; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } 100% { opacity: 1; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } }
  @keyframes tzoom { 0% { transform: scale(1); } 100% { transform: scale(2.2); } }
  @keyframes tslide { 0% { transform: translateX(100%); } 70%, 100% { transform: translateX(0); } }
  @keyframes twipe { 0% { clip-path: inset(0 0 0 100%); } 70%, 100% { clip-path: inset(0 0 0 0); } }
  .all { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; font-size: 12px; }
  .chip.small { height: 26px; padding: 0 10px; font-size: 12px; }
</style>
