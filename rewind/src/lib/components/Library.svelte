<script lang="ts">
  import { liquid } from "$lib/glass";
  import {
    Search, FolderOpen, Trash2, X, Keyboard, Crosshair, Play, Send, Film, FolderPlus, Star,
    CalendarDays, Gamepad2, CalendarClock, ArrowDownWideNarrow, Loader2,
  } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { fit } from "$lib/fit";
  import { api, dayGroup, fileUrl, formatDuration, formatSize, relativeDate, badges, gameTone, sessions, type Clip } from "$lib/api";
  import ClipCard from "./ClipCard.svelte";
  import { t } from "$lib/i18n.svelte";

  let query = $state("");
  let filter = $state<"all" | "fav" | "auto" | "manual" | "edit" | "screenshot">("all");
  let group = $state<"date" | "game" | "session">("date");
  let sort = $state<"new" | "old" | "long" | "fav">("new");
  let size = $state<"s" | "m" | "l">("m");
  let game = $state("");
  let collection = $state("");
  let selected = $state<string[]>([]);
  let heroIndex = $state(0);
  let heroVideo: HTMLVideoElement | undefined = $state();
  let collectionMenu = $state(false);
  let newCollection = $state("");

  const base = $derived(
    app.clips.filter((c) => {
      if (filter === "fav" && !c.favorite) return false;
      if (filter === "auto" && c.kind !== "auto") return false;
      if (filter === "manual" && c.kind !== "manual") return false;
      if (filter === "edit" && c.kind !== "edit") return false;
      if (filter === "screenshot" && c.kind !== "screenshot") return false;
      if (filter === "all" && c.kind === "screenshot") return false;
      if (collection && !c.tags.includes(collection)) return false;
      const q = query.trim().toLowerCase();
      if (q && !`${c.title} ${c.game} ${c.tags.join(" ")} ${c.markers.map((m) => m.label).join(" ")}`.toLowerCase().includes(q)) return false;
      return true;
    }),
  );
  const games = $derived.by(() => {
    const m = new Map<string, number>();
    for (const c of base) m.set(c.game || t("Bureau"), (m.get(c.game || t("Bureau")) ?? 0) + 1);
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  });
  const visible = $derived.by(() => {
    const list = base.filter((c) => !game || (c.game || t("Bureau")) === game);
    const by = {
      new: (a: Clip, b: Clip) => b.created - a.created,
      old: (a: Clip, b: Clip) => a.created - b.created,
      long: (a: Clip, b: Clip) => b.duration - a.duration,
      fav: (a: Clip, b: Clip) => Number(b.favorite) - Number(a.favorite) || b.created - a.created,
    }[sort];
    return [...list].sort(by);
  });
  const collections = $derived([...new Set(app.clips.flatMap((c) => c.tags))].sort());

  // À la une : les temps forts récents (auto avec actions, favoris)
  const featured = $derived.by(() => {
    const vids = app.clips.filter((c) => c.kind !== "screenshot" && c.duration > 0);
    const score = (c: Clip) => (c.favorite ? 3 : 0) + Math.min(4, c.markers.filter((m) => m.kind !== "manual").length) + (c.kind === "auto" ? 1 : 0) - (Date.now() / 1000 - c.created) / 86400;
    return vids.slice(0, 15).sort((a, b) => score(b) - score(a)).slice(0, 5);
  });
  const hero = $derived(featured[Math.min(heroIndex, featured.length - 1)] ?? null);
  const showHero = $derived(!!hero && !query && !collection && !game && filter === "all");

  const groups = $derived.by(() => {
    if (group === "session") {
      return sessions(visible).map((s) => ({ key: s.label + s.clips[0].id, title: s.label, sub: `${s.games.slice(0, 3).join(", ")} · ${t(s.clips.length > 1 ? "{n} clips" : "{n} clip", { n: s.clips.length })}`, color: gameTone(s.games[0]), clips: s.clips }));
    }
    const keyOf = (c: Clip) => (group === "game" ? c.game || t("Bureau") : dayGroup(c.created));
    const out: { key: string; title: string; sub: string; color: string; clips: Clip[] }[] = [];
    const index = new Map<string, number>();
    for (const c of visible) {
      const k = keyOf(c);
      if (!index.has(k)) { index.set(k, out.length); out.push({ key: k, title: k, sub: "", color: group === "game" ? gameTone(k) : "", clips: [] }); }
      out[index.get(k)!].clips.push(c);
    }
    for (const g of out) g.sub = `${t(g.clips.length > 1 ? "{n} clips" : "{n} clip", { n: g.clips.length })} · ${formatDuration(g.clips.reduce((a, c) => a + c.duration, 0))}`;
    return out;
  });

  $effect(() => {
    if (heroVideo && hero) {
      const start = hero.markers.find((m) => m.kind !== "manual")?.time;
      heroVideo.currentTime = start ? Math.max(0, start - 2.5) : Math.min(2, hero.duration * 0.2);
      heroVideo.play().catch(() => {});
    }
  });
  $effect(() => {
    if (featured.length < 2 || !app.settings?.animations) return;
    const t = setInterval(() => (heroIndex = (heroIndex + 1) % featured.length), 11000);
    return () => clearInterval(t);
  });

  function toggle(id: string) { selected = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id]; }
  async function fav(c: Clip) {
    try { app.updateClip(await api.updateClip(c.id, { favorite: !c.favorite })); } catch (e) { app.toast("err", t("Impossible"), String(e)); }
  }
  async function remove() {
    const n = selected.length;
    if (!confirm(`Envoyer ${n} élément${n > 1 ? "s" : ""} à la Corbeille ?`)) return;
    try {
      await api.deleteClips(selected);
      app.removeClips(selected);
      app.toast("info", t(n > 1 ? "{n} éléments dans la Corbeille" : "{n} élément dans la Corbeille", { n }), t("Récupérable depuis la Corbeille de Windows."));
      selected = [];
    } catch (e) { app.toast("err", t("Suppression impossible"), String(e)); }
  }
  function toStudio() {
    const list = app.clips.filter((c) => selected.includes(c.id)).sort((a, b) => a.created - b.created);
    selected = [];
    app.openStudio(list);
  }
  async function addTo(name: string) {
    const n = name.trim();
    if (!n) return;
    for (const id of selected) {
      const c = app.clips.find((x) => x.id === id);
      if (c && !c.tags.includes(n)) app.updateClip(await api.updateClip(id, { tags: [...c.tags, n] }));
    }
    app.toast("ok", t("Ajouté à « {x} »", { x: n }), t(selected.length > 1 ? "{n} clips" : "{n} clip", { n: selected.length }));
    collectionMenu = false;
    newCollection = "";
    selected = [];
  }
  const sizeList: ["s" | "m" | "l", number][] = [["s", 10], ["m", 13], ["l", 16]];
  const filters = [["all", "Tout"], ["fav", "Favoris"], ["auto", "Automatiques"], ["manual", "Manuels"], ["edit", "Montages"], ["screenshot", "Captures"]] as const; // traduits à l'affichage
  const totalSize = $derived(app.clips.reduce((a, c) => a + c.size, 0));
</script>

<section class="lib">
  <div class="scroll">
    {#if app.clips.length === 0}
      <div class="empty appear">
        <h2>{t("Rewind enregistre en continu.")}</h2>
        <p class="muted">{t("Un fou rire sur Discord, une action folle en jeu ? Appuie sur le raccourci : les dernières secondes sont gardées. Rien n'est envoyé sur Internet.")}</p>
        <div class="tips">
          <div class="tip"><Keyboard size={18} /><div><strong>{t("Clip instantané")}</strong><span class="muted"><span class="kbd">{app.settings?.hotkeys.clip}</span> {t("garde les {n} dernières secondes.", { n: app.settings?.clip_seconds ?? 30 })}</span></div></div>
          <div class="tip"><Keyboard size={18} /><div><strong>{t("Clip long")}</strong><span class="muted"><span class="kbd">{app.settings?.hotkeys.clip_long}</span> {t("garde {n} secondes.", { n: app.settings?.clip_long_seconds ?? 120 })}</span></div></div>
          <div class="tip"><Crosshair size={18} /><div><strong>Counter-Strike 2</strong><span class="muted">{t("Un clip par round : 2K, ACE, one tap, séries…")}</span></div></div>
        </div>
      </div>
    {:else}
      {#if showHero && hero}
        <div class="hero appear" style="--c:{gameTone(hero.game)}">
          <button class="hmedia" onclick={() => (app.openClip = hero)} aria-label={t("Lire")}>
            {#key hero.id}
              {#if hero.thumb}<img use:fit src={fileUrl(hero.thumb)} alt="" />{/if}
              <!-- svelte-ignore a11y_media_has_caption -->
              <video use:fit bind:this={heroVideo} src={fileUrl(hero.path)} muted loop playsinline preload="auto"></video>
            {/key}
            <span class="hplay"><Play size={22} fill="currentColor" /></span>
          </button>
          <div class="hinfo">
            <span class="kicker"><i></i>{t("À la une")} · {hero.game || t("Bureau")}</span>
            <h1>{hero.title}</h1>
            <div class="hmeta">
              <span>{relativeDate(hero.created)}</span><span class="mono">{formatDuration(hero.duration)}</span>
              {#each badges(hero) as b}<span class="tag" class:hot={b.hot}>{b.label}</span>{/each}
            </div>
            <div class="hactions">
              <button class="btn primary" onclick={() => (app.openClip = hero)}><Play size={15} fill="currentColor" />{t("Lire")}</button>
              <button class="btn" onclick={() => app.copyForDiscord(hero)} disabled={app.discordBusy === hero.id}>
                {#if app.discordBusy === hero.id}<Loader2 size={15} class="spin" />{t("Compression…")}{:else}<Send size={15} />{t("Copier pour Discord")}{/if}
              </button>
              <button class="btn" onclick={() => app.openStudio([hero])}><Film size={15} />Studio</button>
              <button class="btn icon" onclick={() => fav(hero)} title={t("Favori")}><Star size={15} fill={hero.favorite ? "currentColor" : "none"} /></button>
            </div>
          </div>
          {#if featured.length > 1}
            <div class="hdots">
              {#each featured as f, i}<button class:on={i === heroIndex} onclick={() => (heroIndex = i)} aria-label={f.title}></button>{/each}
            </div>
          {/if}
        </div>
      {/if}

      <div class="toolbar glass strong" use:liquid={{ bezel: 16, strength: 34, blur: 18, saturate: 1.8, chroma: 0.7 }}>
        <div class="seg">
          <button class:on={group === "date"} onclick={() => (group = "date")}><CalendarDays size={14} />{t("Date")}</button>
          <button class:on={group === "game"} onclick={() => (group = "game")}><Gamepad2 size={14} />{t("Jeu")}</button>
          <button class:on={group === "session"} onclick={() => (group = "session")}><CalendarClock size={14} />{t("Session")}</button>
        </div>
        <label class="search">
          <Search size={15} />
          <input placeholder={t("Rechercher : jeu, « ace », « one tap »…")} bind:value={query} />
          {#if query}<button class="clear" onclick={() => (query = "")} aria-label={t("Effacer")}><X size={14} /></button>{/if}
        </label>
        <label class="sort"><ArrowDownWideNarrow size={15} />
          <select bind:value={sort}>
            <option value="new">{t("Plus récents")}</option><option value="old">{t("Plus anciens")}</option><option value="long">{t("Plus longs")}</option><option value="fav">{t("Favoris d'abord")}</option>
          </select>
        </label>
        <div class="sizes" role="radiogroup" aria-label={t("Taille des vignettes")}>
          {#each sizeList as [k, px]}
            <button class:on={size === k} onclick={() => (size = k)} aria-label={`${t("Taille")} ${k}`}><i style="width:{px}px;height:{px - 3}px"></i></button>
          {/each}
        </div>
        <button class="btn icon ghost" title={t("Ouvrir le dossier des clips")} onclick={() => app.settings && api.openFolder(app.settings.clip_folder)}><FolderOpen size={16} /></button>
      </div>

      <div class="filters">
        <button class="gchip" class:on={!game} onclick={() => (game = "")}>{t("Tous les jeux")} <b>{base.length}</b></button>
        {#each games as [g, n]}
          <button class="gchip" class:on={game === g} style="--c:{gameTone(g)}" onclick={() => (game = game === g ? "" : g)}><i></i>{g} <b>{n}</b></button>
        {/each}
        <span class="sep"></span>
        {#each filters as [id, label]}
          <button class="kchip" class:on={filter === id} onclick={() => (filter = id)}>{t(label)}</button>
        {/each}
        {#each collections as c}
          <button class="kchip" class:on={collection === c} onclick={() => (collection = collection === c ? "" : c)}><FolderOpen size={12} />{c}</button>
        {/each}
      </div>

      {#if visible.length === 0}
        <div class="none faint">{t("Aucun clip ne correspond.")}</div>
      {:else}
        {#each groups as g (g.key)}
          <div class="group" style="--c:{g.color || 'var(--line-2)'}">
            <div class="ghead"><h3>{#if g.color}<i></i>{/if}{g.title}</h3><span class="faint">{g.sub}</span></div>
            <div class="grid size-{size}">
              {#each g.clips as clip (clip.id)}
                <ClipCard {clip} selected={selected.includes(clip.id)} selecting={selected.length > 0} onopen={() => (app.openClip = clip)} onselect={() => toggle(clip.id)} onfav={() => fav(clip)} />
              {/each}
            </div>
          </div>
        {/each}
        <p class="foot faint">{t("{n} éléments", { n: app.clips.length })} · {formatSize(totalSize)}</p>
      {/if}
    {/if}
  </div>

  {#if selected.length}
    <div class="bar glass strong appear" use:liquid={{ bezel: 16, strength: 30, blur: 20, saturate: 1.8 }}>
      <span><strong>{selected.length}</strong> {t(selected.length > 1 ? "sélectionnés" : "sélectionné")}</span>
      <button class="btn small primary" onclick={toStudio}><Film size={15} />{t("Monter dans le Studio")}</button>
      <div class="pop-wrap">
        <button class="btn small" onclick={() => (collectionMenu = !collectionMenu)}><FolderPlus size={15} />{t("Collection")}</button>
        {#if collectionMenu}
          <div class="pop appear">
            {#each collections as c}<button class="pop-item" onclick={() => addTo(c)}><FolderOpen size={14} />{c}</button>{/each}
            <form onsubmit={(e) => { e.preventDefault(); addTo(newCollection); }}><input class="field" placeholder={t("Nouvelle collection…")} bind:value={newCollection} /></form>
          </div>
        {/if}
      </div>
      <button class="btn small danger" onclick={remove}><Trash2 size={15} />{t("Corbeille")}</button>
      <button class="btn small icon ghost" onclick={() => (selected = [])} aria-label={t("Annuler la sélection")}><X size={15} /></button>
    </div>
  {/if}
</section>

<style>
  .lib { height: 100%; position: relative; overflow: hidden; isolation: isolate; }
  .scroll { height: 100%; overflow-y: auto; padding: 18px 28px 110px; }

  /* À la une, compacte */
  /* À la une : pas de cadre ni de lueur, la vidéo et le titre suffisent */
  .hero { position: relative; display: grid; grid-template-columns: minmax(0, 440px) minmax(0, 1fr); gap: 28px; align-items: center; padding: 6px 0 26px; margin-bottom: 18px; border-bottom: 1px solid var(--line); }
  .hmedia { position: relative; aspect-ratio: 16 / 9; border-radius: 12px; overflow: hidden; background: #000; box-shadow: 0 0 0 1px var(--line-2), 0 24px 50px -28px rgba(0, 0, 0, 0.9); }
  .hmedia img, .hmedia video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .hplay { position: absolute; left: 50%; top: 50%; translate: -50% -50%; width: 52px; height: 52px; border-radius: 50%; display: grid; place-items: center; background: rgba(255, 255, 255, 0.12); color: #fff; backdrop-filter: blur(14px) saturate(1.6); box-shadow: inset 0 1px 1px rgba(255, 255, 255, 0.35), 0 8px 24px rgba(0, 0, 0, 0.5); opacity: 0; scale: 0.92; transition: opacity var(--dur) var(--ease), scale 260ms var(--spring); }
  .hmedia:hover .hplay { opacity: 1; scale: 1; }
  .hinfo { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .kicker { display: flex; align-items: center; gap: 8px; font-size: 11px; font-weight: 500; letter-spacing: 0.06em; text-transform: uppercase; color: var(--text-3); }
  .kicker i { display: none; }
  .hinfo h1 { margin: 0; font-weight: 600; font-size: clamp(22px, 2.2vw, 28px); line-height: 1.12; letter-spacing: -0.028em; text-wrap: balance; }
  .hmeta { display: flex; align-items: center; flex-wrap: wrap; gap: 10px; font-size: 12.5px; color: var(--text-3); }
  .hactions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 4px; }
  .hdots { position: absolute; right: 0; top: 8px; display: flex; gap: 5px; }
  .hdots button { width: 6px; height: 6px; border-radius: 99px; background: var(--line-2); transition: width 300ms var(--ease-out), background var(--dur) var(--ease); }
  .hdots button.on { width: 18px; background: var(--text-2); }
  :global(.spin) { animation: spin 0.9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  /* Outils */
  /* Barre d'outils : capsule de verre collée en haut, les vignettes passent dessous (vraie réfraction) */
  .toolbar { position: sticky; top: 8px; z-index: 6; display: flex; align-items: center; gap: 6px; margin: 0 -6px 12px; padding: 5px; border-radius: 14px; }
  .toolbar > :global(*) { position: relative; z-index: 2; }
  .seg { display: flex; padding: 2px; border-radius: 10px; background: color-mix(in srgb, var(--bg) 40%, transparent); }
  .seg button { display: flex; align-items: center; gap: 6px; height: 28px; padding: 0 11px; border-radius: 8px; font-size: 12.5px; font-weight: 500; color: var(--text-3); transition: color var(--dur), background var(--dur); }
  .seg button:hover { color: var(--text); }
  .seg button.on { background: color-mix(in srgb, var(--text) 10%, transparent); color: var(--text); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08); }
  .search { flex: 1; display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 11px; border-radius: 10px; background: color-mix(in srgb, var(--bg) 40%, transparent); color: var(--text-3); max-width: 420px; transition: box-shadow var(--dur); }
  .search:focus-within { box-shadow: inset 0 0 0 1px var(--line-2); }
  .search input { flex: 1; border: 0; outline: 0; background: transparent; user-select: text; }
  .clear { color: var(--text-3); display: grid; }
  .sort { display: flex; align-items: center; gap: 6px; height: 32px; padding: 0 10px; border-radius: 10px; color: var(--text-3); margin-left: auto; }
  .sort select { background: transparent; border: 0; outline: 0; color: var(--text-2); font-size: 12.5px; }
  .sort select option { background: var(--panel); }
  .sizes { display: flex; padding: 2px; border-radius: 10px; background: color-mix(in srgb, var(--bg) 40%, transparent); }
  .sizes button { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; }
  .sizes button i { display: block; border-radius: 3px; border: 1.5px solid var(--text-3); }
  .sizes button.on { background: color-mix(in srgb, var(--text) 10%, transparent); }
  .sizes button.on i { border-color: var(--text); }
  .filters { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin-bottom: 20px; }
  /* Filtres : texte simple, pas de pastilles de couleur */
  .gchip, .kchip { display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 11px; border-radius: 99px; border: 1px solid transparent; color: var(--text-3); font-size: 12.5px; font-weight: 500; transition: color var(--dur) var(--ease), background var(--dur) var(--ease), border-color var(--dur) var(--ease); }
  .gchip:hover, .kchip:hover { color: var(--text); background: var(--panel-2); }
  .gchip i { display: none; }
  .gchip b { font-weight: 500; font-size: 11px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .gchip.on, .kchip.on { color: var(--text); border-color: var(--line-2); background: var(--panel-2); }
  .sep { width: 1px; height: 16px; background: var(--line-2); margin: 0 6px; }

  /* Grille */
  .group { margin-bottom: 30px; animation: fade-in 260ms var(--ease) both; }
  .ghead { display: flex; align-items: baseline; gap: 10px; margin-bottom: 12px; }
  .ghead h3 { display: flex; align-items: center; gap: 9px; margin: 0; font-size: 14px; font-weight: 600; letter-spacing: -0.01em; }
  .ghead h3 i { display: none; }
  .ghead span { font-size: 12px; font-variant-numeric: tabular-nums; }
  .grid { display: grid; gap: 22px 16px; }
  .grid.size-s { grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); }
  .grid.size-m { grid-template-columns: repeat(auto-fill, minmax(270px, 1fr)); }
  .grid.size-l { grid-template-columns: repeat(auto-fill, minmax(360px, 1fr)); }
  .none { padding: 60px 0; text-align: center; }
  .foot { text-align: center; font-size: 12px; margin-top: 10px; }
  .empty { max-width: 760px; margin: 70px auto; display: flex; flex-direction: column; gap: 14px; }
  .empty h2 { font-weight: 600; font-size: 28px; letter-spacing: -0.03em; margin: 0; }
  .empty p { margin: 0 0 12px; font-size: 15px; max-width: 560px; }
  .tips { display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; }
  .tip { display: flex; gap: 12px; padding: 16px; border-radius: var(--radius); background: var(--panel); border: 1px solid var(--line); }
  .tip :global(svg) { color: var(--text-2); flex: none; margin-top: 2px; }
  .tip div { display: flex; flex-direction: column; gap: 4px; font-size: 13px; }

  .bar { position: absolute; left: 50%; bottom: 20px; transform: translateX(-50%); display: flex; align-items: center; gap: 6px; padding: 6px 6px 6px 16px; border-radius: 16px; z-index: 7; }
  .bar > :global(*) { position: relative; z-index: 2; }
  .bar > span { margin-right: 8px; font-size: 13px; color: var(--text-2); }
  .bar strong { color: var(--text); }
  .pop-wrap { position: relative; }
  .pop { position: absolute; bottom: 42px; left: 0; width: 240px; padding: 6px; border-radius: 12px; background: var(--panel-2); border: 1px solid var(--line-2); box-shadow: var(--shadow); display: flex; flex-direction: column; gap: 2px; }
  .pop-item { display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 10px; border-radius: 8px; font-size: 13px; text-align: left; }
  .pop-item:hover { background: var(--panel-3); }
  .pop .field { width: 100%; height: 34px; margin-top: 4px; }
</style>
