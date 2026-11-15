<script lang="ts">
  import { Star, Check, Image as ImageIcon, Zap, Scissors, Send, Film, Loader2 } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { type Clip, fileUrl, formatDuration, relativeDate, badges, gameTone } from "$lib/api";
  import { t } from "$lib/i18n.svelte";
  import { fit } from "$lib/fit";

  let { clip, selected = false, selecting = false, onopen, onselect, onfav }: {
    clip: Clip;
    selected?: boolean;
    selecting?: boolean;
    onopen: () => void;
    onselect: () => void;
    onfav: () => void;
  } = $props();

  let hover = $state(false);
  let video: HTMLVideoElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const isShot = $derived(clip.kind === "screenshot");
  const b = $derived(badges(clip));

  function enter() {
    if (isShot) return;
    timer = setTimeout(() => (hover = true), 260);
  }
  function leave() {
    clearTimeout(timer);
    hover = false;
  }
  $effect(() => {
    if (hover && video) {
      const start = clip.markers[0]?.time ? Math.max(0, clip.markers[0].time - 2) : Math.min(3, clip.duration * 0.3);
      video.currentTime = start;
      video.play().catch(() => {});
    }
  });

  function click(e: MouseEvent) {
    if (selecting || e.ctrlKey || e.shiftKey) onselect();
    else onopen();
  }
</script>

<div class="card" style="--c:{gameTone(clip.game)}" class:selected role="button" tabindex="0" onclick={click} onkeydown={(e) => e.key === "Enter" && onopen()} onmouseenter={enter} onmouseleave={leave}>
  <div class="media">
    {#if clip.thumb}
      <img use:fit src={fileUrl(clip.thumb)} alt="" loading="lazy" draggable="false" />
    {:else}
      <div class="ph"></div>
    {/if}
    {#if hover}
      <video use:fit bind:this={video} src={fileUrl(clip.path)} muted playsinline preload="auto" class="appear"></video>
    {/if}
    <div class="top">
      <div class="badges">
        {#if clip.kind === "auto"}<span class="tag"><Zap size={11} />{t("Auto")}</span>{/if}
        {#if clip.kind === "edit"}<span class="tag neutral"><Scissors size={11} />{t("Montage")}</span>{/if}
        {#if clip.kind === "received"}<span class="tag neutral">{t("Reçu")}</span>{/if}
        {#each b as x}<span class="tag" class:hot={x.hot}>{x.label}</span>{/each}
      </div>
      <button class="sel" class:on={selected} aria-label={t("Sélectionner")} onclick={(e) => { e.stopPropagation(); onselect(); }}>
        {#if selected}<Check size={13} strokeWidth={3} />{/if}
      </button>
    </div>
    {#if !isShot && !selecting}
      <div class="quick">
        <button title={t("Copier pour Discord (compressé si besoin)")} onclick={(e) => { e.stopPropagation(); app.copyForDiscord(clip); }}>
          {#if app.discordBusy === clip.id}<Loader2 size={14} class="spin" />{:else}<Send size={14} />{/if}
        </button>
        <button title={t("Ouvrir dans le Studio")} onclick={(e) => { e.stopPropagation(); app.openStudio([clip]); }}><Film size={14} /></button>
      </div>
    {/if}
    <div class="bottom">
      {#if isShot}<span class="dur"><ImageIcon size={12} /></span>{:else}<span class="dur mono">{formatDuration(clip.duration)}</span>{/if}
    </div>
  </div>
  <div class="meta">
    <div class="line">
      <span class="title">{clip.title}</span>
      <button class="fav" class:on={clip.favorite} aria-label={t("Favori")} onclick={(e) => { e.stopPropagation(); onfav(); }}>
        <Star size={15} fill={clip.favorite ? "currentColor" : "none"} />
      </button>
    </div>
    <div class="sub"><i class="gdot"></i>{clip.game || t("Bureau")} · {relativeDate(clip.created)}</div>
  </div>
</div>

<style>
  .card { position: relative; border-radius: var(--radius); cursor: pointer; outline: none; }
  .media { position: relative; aspect-ratio: 16 / 9; border-radius: var(--radius); overflow: hidden; background: var(--panel-2); box-shadow: 0 0 0 1px var(--line); transition: transform 320ms var(--ease-out), box-shadow var(--dur) var(--ease); }
  .card:hover .media { transform: translateY(-2px); box-shadow: 0 0 0 1px var(--line-2), 0 18px 36px -18px rgba(0, 0, 0, 0.9); }
  .card.selected .media { box-shadow: 0 0 0 2px var(--hi); }
  .card:focus-visible .media { box-shadow: 0 0 0 2px var(--hi); }
  img, video, .ph { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover; }
  .ph { background: linear-gradient(135deg, var(--panel-2), var(--panel-3)); }
  .top { position: absolute; inset: 8px 8px auto 8px; display: flex; justify-content: space-between; align-items: flex-start; gap: 6px; }
  .badges { display: flex; flex-wrap: wrap; gap: 4px; }
  .badges .tag { background: rgba(0, 0, 0, 0.55); color: rgba(255, 255, 255, 0.9); backdrop-filter: blur(10px) saturate(1.5); box-shadow: inset 0 0 0 0.5px rgba(255, 255, 255, 0.14); }
  .badges .tag.hot { color: #ff7b72; }
  .badges .tag.neutral { color: rgba(255, 255, 255, 0.75); }
  .sel { width: 22px; height: 22px; border-radius: 7px; border: 1.5px solid rgba(255, 255, 255, 0.7); background: rgba(8, 9, 12, 0.35); display: grid; place-items: center; opacity: 0; transition: opacity var(--dur); color: var(--hi-ink); }
  .card:hover .sel, .sel.on { opacity: 1; }
  .sel.on { background: var(--hi); border-color: var(--hi); color: var(--hi-ink); }
  .bottom { position: absolute; right: 8px; bottom: 8px; }
  .quick { position: absolute; left: 8px; bottom: 8px; display: flex; gap: 5px; opacity: 0; transform: translateY(4px); transition: all var(--dur) var(--ease); }
  .card:hover .quick { opacity: 1; transform: none; }
  .quick button { width: 30px; height: 30px; border-radius: 9px; display: grid; place-items: center; background: rgba(0, 0, 0, 0.5); color: #fff; backdrop-filter: blur(12px) saturate(1.6); box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.18), inset 0 0 0 0.5px rgba(255, 255, 255, 0.12); transition: background var(--dur), color var(--dur); }
  .quick button:hover { background: rgba(255, 255, 255, 0.92); color: #000; }
  .dur { display: inline-flex; align-items: center; height: 19px; padding: 0 6px; border-radius: 5px; background: rgba(0, 0, 0, 0.6); color: #fff; font-family: var(--mono); font-size: 11px; font-weight: 500; font-variant-numeric: tabular-nums; backdrop-filter: blur(10px); }
  .meta { padding: 9px 2px 0; }
  .line { display: flex; align-items: center; gap: 6px; }
  .title { flex: 1; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fav { color: var(--text-3); opacity: 0; transition: opacity var(--dur), color var(--dur); }
  .card:hover .fav, .fav.on { opacity: 1; }
  .fav.on { color: var(--text); }
  .gdot { display: none; }
  .sub { font-size: 12px; color: var(--text-3); margin-top: 1px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
