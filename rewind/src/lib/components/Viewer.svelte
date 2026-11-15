<script lang="ts">
  import {
    ArrowLeft, Play, Pause, Volume2, VolumeX, Maximize, Star, Send, FolderOpen, ExternalLink, Trash2,
    Film, Gauge, ChevronLeft, ChevronRight, Loader2, Hash, Plus, X, MessageSquare, Keyboard, Users,
  } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { friends } from "$lib/friends.svelte";
  import { api, fileUrl, formatDuration, formatSize, relativeDate, gameTone, badges, KIND_LABEL, type Clip, type InputEvent } from "$lib/api";
  import KeysOverlay from "./KeysOverlay.svelte";
  import MomentIcon from "./MomentIcon.svelte";
  import Avatar from "./Avatar.svelte";
  import { momentTone } from "$lib/moments";
  import { t, locale } from "$lib/i18n.svelte";

  let { clip }: { clip: Clip } = $props();

  let video: HTMLVideoElement | undefined = $state();
  let stage: HTMLDivElement | undefined = $state();
  let track: HTMLDivElement | undefined = $state();
  let playing = $state(false);
  let time = $state(0);
  let duration = $state(0);
  let muted = $state(false);
  let volume = $state(1);
  let rate = $state(1);
  let title = $state("");
  let share = $state(false);
  let hook = $state(0);
  let message = $state("");
  let sending = $state(false);
  // Envoi à des amis (partage entre amis)
  let toFriends = $state(false);
  let picked = $state<string[]>([]);
  let friendSending = $state(false);
  const fv = $derived(friends.view);
  const canShare = $derived(!!fv?.enabled && !!fv?.ready);
  async function sendToFriends() {
    if (!picked.length) return;
    friendSending = true;
    const n = picked.length;
    const ok = await friends.run(() => api.shareSend(clip.id, picked));
    friendSending = false;
    if (ok) {
      app.toast("ok", t(n > 1 ? "Envoi à {n} amis lancé" : "Envoi lancé", { n }), t("Suis l'avancement dans Amis."));
      picked = [];
      toFriends = false;
    }
  }
  let hoverT = $state<number | null>(null);
  let newTag = $state("");

  const isShot = $derived(clip.kind === "screenshot");
  const total = $derived(duration || clip.duration || 0);
  const webhooks = $derived(app.settings?.webhooks.filter((w) => w.url) ?? []);
  const color = $derived(gameTone(clip.game));
  const received = $derived(clip.kind === "received");
  const list = $derived(received ? app.clips.filter((c) => c.kind === "received") : app.ownClips);
  // Clip reçu : l'ami qui l'a envoyé (historique du partage) et sa photo
  const origin = $derived(received ? (friends.view?.received.find((r) => r.clip_id === clip.id) ?? null) : null);
  const originAvatar = $derived(origin ? (friends.view?.friends.find((f) => f.id === origin.from_id)?.avatar ?? 0) : 0);
  $effect(() => {
    if (received) friends.start();
  });
  const index = $derived(list.findIndex((c) => c.id === clip.id));
  const markers = $derived(clip.markers.filter((m) => m.time <= total + 0.05));
  // Largeur de la barre de lecture : sert à empiler (voies) les repères trop proches pour tenir côte à côte
  let trackW = $state(600);
  const placed = $derived.by(() => {
    const lastX = [-999, -999, -999];
    return [...markers]
      .sort((a, b) => a.time - b.time)
      .map((m) => {
        const x = (total > 0 ? m.time / total : 0) * trackW;
        let lane = lastX.findIndex((l) => x - l >= 20);
        if (lane < 0) lane = lastX.indexOf(Math.min(...lastX));
        lastX[lane] = x;
        return { m, lane };
      });
  });

  $effect(() => {
    title = clip.title;
    share = false;
  });

  // Touches clavier/souris enregistrées avec ce clip (option « touches à l'écran »)
  let inputs = $state<InputEvent[]>([]);
  let showKeys = $state(true);
  $effect(() => {
    const id = clip.id;
    inputs = [];
    if (clip.kind !== "screenshot") api.clipInputs(id).then((l) => { if (clip.id === id) inputs = l ?? []; }).catch(() => {});
  });

  function onMeta() {
    if (!video) return;
    duration = video.duration;
    const first = clip.markers.find((m) => m.kind !== "manual");
    if (first && first.time > 3) video.currentTime = Math.max(0, first.time - 3);
    video.play().catch(() => {});
  }
  function toggle() { if (video) video.paused ? video.play() : video.pause(); }
  function seek(t: number) { if (video) video.currentTime = Math.max(0, Math.min(total, t)); }
  function at(e: PointerEvent) {
    const r = track!.getBoundingClientRect();
    return Math.max(0, Math.min(total, ((e.clientX - r.left) / r.width) * total));
  }
  let dragging = false;
  function down(e: PointerEvent) { dragging = true; (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId); seek(at(e)); }
  function move(e: PointerEvent) { if (track) hoverT = at(e); if (dragging) seek(at(e)); }
  function go(delta: number) {
    const next = list[index + delta];
    if (next) app.openClip = next;
  }
  function key(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
    if (e.key === " " || e.key === "k") { e.preventDefault(); toggle(); }
    else if (e.key === "ArrowLeft") seek(time - (e.shiftKey ? 1 : 5));
    else if (e.key === "ArrowRight") seek(time + (e.shiftKey ? 1 : 5));
    else if (e.key === ",") { video?.pause(); seek(time - 1 / 60); }
    else if (e.key === ".") { video?.pause(); seek(time + 1 / 60); }
    else if (e.key === "f" || e.key === "F11") { e.preventDefault(); fullscreen(); }
    else if (e.key === "m") muted = !muted;
    else if (e.key === "PageDown") go(1);
    else if (e.key === "PageUp") go(-1);
  }
  function fullscreen() { if (document.fullscreenElement) document.exitFullscreen(); else stage?.requestFullscreen(); }

  async function saveTitle() {
    const name = title.trim();
    if (!name || name === clip.title) return;
    try { app.updateClip(await api.updateClip(clip.id, { title: name })); }
    catch (e) { app.toast("err", t("Renommage impossible"), String(e)); title = clip.title; }
  }
  async function fav() { app.updateClip(await api.updateClip(clip.id, { favorite: !clip.favorite })); }
  async function setTags(tags: string[]) { app.updateClip(await api.updateClip(clip.id, { tags })); }
  async function send() {
    sending = true;
    try {
      const r = await api.shareDiscord(clip.id, hook, message);
      app.toast("ok", t("Envoyé sur Discord"), r.compressed ? t("Compressé automatiquement avant l'envoi.") : undefined);
      share = false;
      message = "";
    } catch (e) { app.toast("err", t("Envoi impossible"), String(e)); }
    sending = false;
  }
  async function remove() {
    if (!confirm(t("Envoyer ce clip à la Corbeille ?"))) return;
    try {
      const next = list[index + 1] ?? list[index - 1] ?? null;
      await api.deleteClips([clip.id]);
      app.removeClips([clip.id]);
      app.toast("info", t("Clip dans la Corbeille"), t("Récupérable depuis la Corbeille de Windows."));
      if (next) app.openClip = next;
    } catch (e) { app.toast("err", t("Suppression impossible"), String(e)); }
  }
  const pct = (t: number) => (total > 0 ? (t / total) * 100 : 0);
  const kindLabel = $derived(t(clip.kind === "auto" ? "Automatique" : clip.kind === "edit" ? "Montage" : clip.kind === "screenshot" ? "Capture" : clip.kind === "session" ? "Session" : clip.kind === "received" ? "Reçu" : "Manuel"));
</script>

<svelte:window onkeydown={key} onpointerup={() => (dragging = false)} />

<div class="viewer" style="--c:{color}">
  <div class="glow"></div>
  <div class="left">
    <div class="top">
      <button class="btn ghost small" onclick={() => (app.openClip = null)}><ArrowLeft size={16} />{received ? t("Amis") : t("Bibliothèque")}</button>
      <span class="crumb"><i></i>{clip.game || t("Bureau")}<span class="faint">· {relativeDate(clip.created)}</span></span>
      <div class="nav">
        <span class="faint mono">{index + 1} / {list.length}</span>
        <button class="btn icon small ghost" onclick={() => go(-1)} disabled={index <= 0} title={t("Clip précédent (Page préc.)")}><ChevronLeft size={17} /></button>
        <button class="btn icon small ghost" onclick={() => go(1)} disabled={index >= list.length - 1} title={t("Clip suivant (Page suiv.)")}><ChevronRight size={17} /></button>
      </div>
    </div>

    <div class="stage" bind:this={stage}>
      {#if isShot}
        <img src={fileUrl(clip.path)} alt={clip.title} />
      {:else}
        {#key clip.id}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video
            bind:this={video}
            src={fileUrl(clip.path)}
            bind:currentTime={time}
            bind:paused={() => !playing, (p) => (playing = !p)}
            bind:muted
            bind:volume
            bind:playbackRate={rate}
            onloadedmetadata={onMeta}
            onclick={toggle}
            ondblclick={fullscreen}
          ></video>
        {/key}
        {#if inputs.length && showKeys}<KeysOverlay events={inputs} {time} />{/if}
        {#if !playing}<button class="bigplay" onclick={toggle} aria-label={t("Lecture")}><Play size={30} fill="currentColor" /></button>{/if}
      {/if}
    </div>

    {#if !isShot}
      <div class="controls">
        <button class="btn icon play" onclick={toggle} aria-label={playing ? t("Pause") : t("Lecture")}>
          {#if playing}<Pause size={18} fill="currentColor" />{:else}<Play size={18} fill="currentColor" />{/if}
        </button>
        <span class="time mono">{formatDuration(time)}<span class="faint"> / {formatDuration(total)}</span></span>
        <div class="track" bind:this={track} bind:clientWidth={trackW} role="slider" tabindex="-1" aria-valuenow={time} onpointerdown={down} onpointermove={move} onpointerleave={() => (hoverT = null)}>
          <div class="rail"><div class="played" style="width:{pct(time)}%"></div></div>
          {#each placed as { m, lane }}
            <button class="marker {momentTone(m.kind)}" class:past={time >= m.time - 0.05} style="left:{pct(m.time)}%; --lane:{lane}" title="{m.label || KIND_LABEL[m.kind] || m.kind} · {formatDuration(m.time)}" aria-label={m.label || KIND_LABEL[m.kind] || m.kind} onpointerdown={(e) => { e.stopPropagation(); seek(Math.max(0, m.time - 2)); }}><MomentIcon kind={m.kind} size={16} /></button>
          {/each}
          <div class="knob" style="left:{pct(time)}%"></div>
          {#if hoverT !== null}<span class="tip mono" style="left:{pct(hoverT)}%">{formatDuration(hoverT)}</span>{/if}
        </div>
        {#if inputs.length}
          <button class="btn small ghost" class:on={showKeys} onclick={() => (showKeys = !showKeys)} title={t("Afficher les touches clavier / souris")}><Keyboard size={15} />{t("Touches")}</button>
        {/if}
        <button class="btn small ghost" onclick={() => (rate = rate >= 2 ? 0.25 : rate * 2)} title={t("Vitesse de lecture")}><Gauge size={15} />{rate}×</button>
        <button class="btn icon ghost" onclick={() => (muted = !muted)} aria-label={t("Son")}>{#if muted || volume === 0}<VolumeX size={17} />{:else}<Volume2 size={17} />{/if}</button>
        <input class="vol" type="range" min="0" max="1" step="0.02" bind:value={volume} />
        <button class="btn icon ghost" onclick={fullscreen} aria-label={t("Plein écran")}><Maximize size={16} /></button>
      </div>
      <div class="hints faint">{t("Espace lecture · ← → 5 s · , . image par image · F plein écran · Page préc./suiv. clip voisin")}</div>
    {/if}
  </div>

  <aside>
    <div class="head">
      {#if received}
        <h2 class="title-input static" title={clip.title}>{clip.title}</h2>
        <div class="from">
          <Avatar id={origin?.from_id ?? ""} version={originAvatar} name={origin?.from_name ?? "?"} size={28} />
          <span><strong>{t("Envoyé par {name}", { name: origin?.from_name ?? t("un ami") })}</strong><small class="muted">{origin ? relativeDate(origin.at) : ""}</small></span>
        </div>
      {:else}
        <input class="title-input" bind:value={title} onblur={saveTitle} onkeydown={(e) => e.key === "Enter" && (e.currentTarget as HTMLInputElement).blur()} spellcheck="false" />
      {/if}
      <div class="badges">
        <span class="gchip"><i></i>{clip.game || t("Bureau")}</span>
        {#each badges(clip) as b}<span class="tag" class:hot={b.hot}>{b.label}</span>{/each}
      </div>
    </div>

    <div class="facts">
      <div><span>{t("Date")}</span><strong>{new Date(clip.created * 1000).toLocaleString(locale(), { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" })}</strong></div>
      {#if !isShot}<div><span>{t("Durée")}</span><strong class="mono">{formatDuration(clip.duration)}</strong></div>{/if}
      <div><span>{t("Taille")}</span><strong>{formatSize(clip.size)}</strong></div>
      <div><span>{t("Type")}</span><strong>{kindLabel}</strong></div>
    </div>

    {#if !received}
    <button class="btn primary discord" onclick={() => app.copyForDiscord(clip)} disabled={app.discordBusy === clip.id}>
      {#if app.discordBusy === clip.id}<Loader2 size={17} class="spin" />{t("Compression pour Discord…")}{:else}<Send size={17} />{t("Copier pour Discord")}{/if}
    </button>
    <p class="hint faint">{clip.size / 1048576 > (app.settings?.discord_max_mb ?? 20) ? t("Plus de {n} Mo : il sera compressé automatiquement.", { n: app.settings?.discord_max_mb ?? 20 }) : t("Puis Ctrl+V dans n'importe quelle conversation.")}</p>

    <div class="row3">
      {#if !isShot}<button class="btn" onclick={() => app.openStudio([clip])}><Film size={15} />Studio</button>{/if}
      <button class="btn" onclick={() => (share = !share)} class:on={share}><MessageSquare size={15} />{t("Salon")}</button>
      <button class="btn" onclick={fav}><Star size={15} fill={clip.favorite ? "currentColor" : "none"} />{t("Favori")}</button>
    </div>

    {#if share}
      <div class="panel appear">
        {#if webhooks.length === 0}
          <p class="muted">{t("Pour envoyer en un clic sur un salon, ajoute son webhook dans Réglages → Partage. Sinon « Copier pour Discord » puis Ctrl+V marche partout.")}</p>
          <button class="btn small" onclick={() => { app.openClip = null; app.view = "settings"; }}>{t("Ouvrir les réglages")}</button>
        {:else}
          <select class="field" bind:value={hook}>{#each webhooks as w, i}<option value={i}>{w.name}</option>{/each}</select>
          <textarea class="field area" placeholder={t("Message (facultatif)")} bind:value={message}></textarea>
          <button class="btn primary" disabled={sending} onclick={send}><Send size={15} />{sending ? t("Envoi…") : t("Envoyer")}</button>
        {/if}
      </div>
    {/if}

    {#if !isShot}
      <button class="btn tofriends" class:on={toFriends} onclick={() => { toFriends = !toFriends; friends.start(); }}><Users size={15} />{t("Envoyer à un ami")}</button>
      {#if toFriends}
        <div class="panel appear">
          {#if !canShare}
            <p class="muted">{t("Active d'abord le partage entre amis : tes clips arrivent directement dans leur Rewind.")}</p>
            <button class="btn small" onclick={() => { app.openClip = null; app.view = "friends"; }}>{t("Ouvrir Amis")}</button>
          {:else if !fv?.friends.length}
            <p class="muted">{t("Tu n'as pas encore d'amis dans Rewind. Ajoute-les avec leur code ami.")}</p>
            <button class="btn small" onclick={() => { app.openClip = null; app.view = "friends"; }}>{t("Ouvrir Amis")}</button>
          {:else}
            <div class="pick">
              {#each fv.friends as f (f.id)}
                <button class="chip who" class:on={picked.includes(f.id)} onclick={() => (picked = picked.includes(f.id) ? picked.filter((x) => x !== f.id) : [...picked, f.id])}><Avatar id={f.id} version={f.avatar} name={f.name} size={18} online={f.online} />{f.name}</button>
              {/each}
            </div>
            {#if clip.size > (fv.limits.max_clip ?? Infinity)}<p class="muted">{t("Clip trop lourd pour le partage (500 Mo au maximum).")}</p>{/if}
            <button class="btn primary" disabled={!picked.length || friendSending || clip.size > fv.limits.max_clip} onclick={sendToFriends}>
              {#if friendSending}<Loader2 size={15} class="spin" />{:else}<Send size={15} />{/if}{picked.length > 1 ? t("Envoyer à {n} amis", { n: picked.length }) : t("Envoyer")}
            </button>
          {/if}
        </div>
      {/if}
    {/if}
    {/if}

    {#if markers.length && !isShot}
      <div class="label">{t("Moments")}</div>
      <div class="moments">
        {#each [...markers].sort((a, b) => a.time - b.time) as m}
          <button class="moment" onclick={() => seek(Math.max(0, m.time - 2))}>
            <i class="mico {momentTone(m.kind)}"><MomentIcon kind={m.kind} size={16} /></i><span>{m.label || KIND_LABEL[m.kind] || m.kind}</span><span class="mono faint">{formatDuration(m.time)}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if !received}
    <div class="label">{t("Collections")}</div>
    <div class="tags">
      {#each clip.tags as tag}<span class="ctag"><Hash size={12} />{tag}<button aria-label={t("Retirer")} onclick={() => setTags(clip.tags.filter((x) => x !== tag))}><X size={12} /></button></span>{/each}
      <form onsubmit={(e) => { e.preventDefault(); if (newTag.trim()) { setTags([...clip.tags, newTag.trim()]); newTag = ""; } }}>
        <input class="tag-input" placeholder={t("+ Ajouter")} bind:value={newTag} list="cols" />
        <datalist id="cols">{#each [...new Set(app.ownClips.flatMap((c) => c.tags))] as c}<option value={c}></option>{/each}</datalist>
      </form>
    </div>
    {/if}

    <div class="foot">
      <button class="btn small ghost" onclick={() => api.reveal(clip.id)}><FolderOpen size={14} />{t("Dossier")}</button>
      <button class="btn small ghost" onclick={() => api.openExternal(clip.id)}><ExternalLink size={14} />{t("Ouvrir")}</button>
      <button class="btn small ghost danger" onclick={remove}><Trash2 size={14} />{t("Corbeille")}</button>
    </div>

    {#if app.progress}
      <div class="progress appear"><span>{t(app.progress.op === "compress" ? "Compression" : "Traitement")}… {Math.round(app.progress.value * 100)} %</span><div class="pbar"><i style="transform:scaleX({app.progress.value})"></i></div></div>
    {/if}
  </aside>
</div>

<style>
  .viewer { position: fixed; inset: 44px 0 0 0; z-index: 20; display: flex; background: var(--bg); border-top: 1px solid var(--line); animation: fade-in 200ms var(--ease) both; isolation: isolate; overflow: hidden; }
  .glow { display: none; }
  .left { flex: 1; min-width: 0; display: flex; flex-direction: column; padding: 12px 18px 10px 22px; gap: 10px; }
  .top { display: flex; align-items: center; gap: 12px; }
  .crumb { display: flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600; }
  .crumb i { display: none; }
  .nav { margin-left: auto; display: flex; align-items: center; gap: 4px; font-size: 12px; }
  .controls :global(.btn.on) { color: var(--hi); }
  .stage { position: relative; flex: 1; min-height: 0; border-radius: 18px; overflow: hidden; background: #000; display: grid; place-items: center; box-shadow: 0 0 0 1px var(--line), 0 30px 80px -40px rgba(0, 0, 0, 0.9); }
  /* En absolu : dans une grille, la ligne « auto » prenait la hauteur réelle d'une vidéo verticale (1920 px) et la scène n'en montrait qu'une bande */
  .stage video, .stage img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; }
  .stage:fullscreen { border-radius: 0; }
  .bigplay { position: absolute; width: 74px; height: 74px; border-radius: 50%; display: grid; place-items: center; background: rgba(0,0,0,.5); color: #fff; backdrop-filter: blur(10px); border: 1px solid rgba(255,255,255,.15); transition: transform var(--dur) var(--ease); }
  .bigplay:hover { transform: scale(1.06); }
  .controls { display: flex; align-items: center; gap: 6px; }
  .btn.play { width: 40px; height: 40px; border-radius: 50%; background: var(--hi); color: var(--hi-ink); border: 0; }
  .time { font-size: 12.5px; min-width: 92px; margin-left: 4px; }
  /* Barre centrée sur la même ligne que le bouton lecture et le temps (elle était 6 px plus bas) */
  .track { position: relative; flex: 1; height: 40px; cursor: pointer; touch-action: none; }
  .rail { position: absolute; left: 0; right: 0; top: 50%; height: 4px; margin-top: -2px; border-radius: 4px; background: var(--panel-3); overflow: hidden; transition: height 120ms, margin 120ms; }
  .track:hover .rail { height: 6px; margin-top: -3px; }
  .played { height: 100%; background: var(--hi); }
  .knob { position: absolute; top: 50%; z-index: 4; width: 14px; height: 14px; margin: -7px 0 0 -7px; border-radius: 50%; background: #fff; box-shadow: 0 0 0 3px color-mix(in srgb, var(--hi) 40%, transparent), 0 1px 4px rgba(0, 0, 0, 0.5); pointer-events: none; }
  .tip { position: absolute; bottom: 34px; translate: -50% 0; padding: 2px 6px; border-radius: 6px; background: var(--panel-3); font-size: 11px; pointer-events: none; }
  /* Repère : picto CS2 posé sur la barre, blanc (or pour 2K/3K/ACE, vert pour un clutch). Un liseré de la couleur
     du fond le détache de la barre ; avant d'être lu il est atténué. Deux repères trop proches (collateral) :
     le second monte d'un cran au-dessus de la barre. */
  .marker {
    position: absolute; top: 50%; z-index: 2; height: 22px; min-width: 22px; margin: 0; padding: 0 2px;
    translate: -50% calc(-50% - var(--lane, 0) * 20px);
    display: grid; place-items: center; color: #f2f3f5; opacity: 0.6;
    filter: drop-shadow(0 0 1.2px var(--bg)) drop-shadow(0 0 1.2px var(--bg)) drop-shadow(0 1px 2px rgba(0, 0, 0, 0.6));
    transition: scale 200ms var(--spring), opacity 240ms var(--ease);
  }
  .marker.gold { color: #ffd25a; }
  .marker.green { color: #3ddc97; }
  .marker.dim { color: #9aa3b5; }
  .marker.past { opacity: 1; }
  .marker:hover { scale: 1.2; opacity: 1; z-index: 5; }
  .vol { width: 80px; }
  .hints { font-size: 11.5px; text-align: center; }

  aside { width: 360px; flex: none; border-left: 1px solid var(--line); padding: 18px; display: flex; flex-direction: column; gap: 12px; overflow-y: auto; background: color-mix(in srgb, var(--bg-2) 88%, transparent); backdrop-filter: blur(10px); }
  .head { display: flex; flex-direction: column; gap: 8px; }
  .title-input { font-family: var(--display); font-weight: 600; font-size: 20px; letter-spacing: -0.02em; background: transparent; border: 1px solid transparent; border-radius: 8px; padding: 6px 8px; margin: -6px -8px 0; outline: none; user-select: text; }
  .title-input:hover { border-color: var(--line); }
  .title-input:focus { border-color: var(--hi); background: var(--panel); }
  .badges { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .gchip { display: inline-flex; align-items: center; gap: 6px; height: 22px; padding: 0 9px; border-radius: 99px; font-size: 12px; font-weight: 600; background: var(--panel-3); border: 1px solid var(--line-2); font-weight: 500; }
  .gchip i { display: none; }
  .facts { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  /* Contenu centré verticalement : une case sur deux lignes ne laisse plus sa voisine collée en haut */
  .facts div { display: flex; flex-direction: column; justify-content: center; gap: 2px; min-width: 0; padding: 9px 11px; border-radius: 10px; background: var(--panel); border: 1px solid var(--line); }
  .facts span { font-size: 11px; color: var(--text-3); text-transform: uppercase; letter-spacing: 0.05em; font-weight: 600; }
  .facts strong { font-size: 13.5px; font-weight: 600; }
  .discord { height: 48px; font-size: 15px; border-radius: 13px; }
  .hint { margin: -6px 0 0; font-size: 12px; text-align: center; }
  .row3 { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .row3 .btn { padding: 0 8px; font-size: 13px; }
  .row3 .btn.on { border-color: var(--hi); }
  .tofriends { width: 100%; }
  .title-input.static { margin: 0; font-size: inherit; font-weight: inherit; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: default; }
  .from { display: flex; align-items: center; gap: 10px; margin-top: 10px; }
  .from span { display: flex; flex-direction: column; gap: 1px; font-size: 13px; min-width: 0; }
  .from strong { font-weight: 500; }
  .from small { font-size: 12px; }
  .tofriends.on { border-color: var(--hi); }
  .pick { display: flex; flex-wrap: wrap; gap: 6px; }
  .pick .who { padding-left: 4px; }
  .panel { display: flex; flex-direction: column; gap: 10px; padding: 12px; border-radius: var(--radius); background: var(--panel); border: 1px solid var(--line); }
  .panel p { margin: 0; font-size: 13px; }
  .area { height: 64px; padding: 10px 12px; resize: none; }
  .label { font-size: 11.5px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-3); margin-top: 6px; }
  .moments { display: flex; flex-direction: column; gap: 2px; }
  .moment { display: grid; grid-template-columns: 22px 1fr auto; align-items: center; gap: 10px; padding: 8px 10px; border-radius: 8px; text-align: left; font-size: 13px; }
  .moment:hover { background: var(--panel-2); }
  .mico { width: 22px; height: 22px; display: grid; place-items: center; color: var(--text); }
  .mico.gold { color: #ffd25a; }
  .mico.green { color: #3ddc97; }
  .mico.dim { color: var(--text-3); }
  .tags { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .ctag { display: inline-flex; align-items: center; gap: 4px; height: 26px; padding: 0 6px 0 9px; border-radius: 99px; background: var(--panel-2); border: 1px solid var(--line); font-size: 12.5px; }
  .ctag button { display: grid; color: var(--text-3); }
  .tag-input { height: 26px; width: 110px; padding: 0 10px; border-radius: 99px; background: transparent; border: 1px dashed var(--line-2); outline: none; font-size: 12.5px; user-select: text; }
  .tag-input:focus { border-style: solid; border-color: var(--hi); }
  .foot { margin-top: auto; display: flex; gap: 4px; padding-top: 10px; border-top: 1px solid var(--line); }
  .progress { display: flex; flex-direction: column; gap: 8px; font-size: 12.5px; color: var(--text-2); padding: 12px; border-radius: var(--radius); background: var(--panel); border: 1px solid var(--line); }
  .pbar { height: 5px; border-radius: 5px; background: var(--panel-3); overflow: hidden; }
  .pbar i { display: block; height: 100%; background: var(--hi); transform-origin: left; transition: transform 200ms linear; }
</style>
