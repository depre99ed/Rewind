<script lang="ts">
  import { Volume2, VolumeX, Gauge, Blend, Scissors, CopyPlus, Trash2, Music, Crosshair, Monitor, Smartphone, X, Keyboard, ArrowLeftToLine, ArrowRightToLine, ArrowLeft, ArrowRight } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { studio, media, SPEEDS, TRANSITIONS, fmtTime, clampSpeed, pickMusic } from "$lib/studio.svelte";
  import { fileUrl, gameTone, KIND_LABEL } from "$lib/api";
  import { t } from "$lib/i18n.svelte";

  const p = $derived(studio.sel);
  const i = $derived(studio.selIndex);
  const tracks = $derived(p ? media.separateTracks(p.path) : []);
  const m = $derived(app.studioMusic);
  const L = $derived(studio.lay);

  /** Curseur : on enregistre l'état au début du geste, puis on modifie sans remplir l'historique. */
  const begin = () => studio.snapshot();

  function setTrack(k: number, v: number) {
    if (!p) return;
    const list = [...p.trackVolumes];
    list[k] = v;
    studio.update(p.uid, { trackVolumes: list }, false);
  }
  /** Rogne le début ou la fin à la tête de lecture. */
  function trimHere(side: "start" | "end") {
    if (!p || i < 0) return;
    const src = studio.srcTime(i, studio.t);
    if (side === "start" && src < p.end - 0.1) studio.update(p.uid, { start: src });
    else if (side === "end" && src > p.start + 0.1) studio.update(p.uid, { end: src });
    else return studio.say(t("La tête de lecture doit être sur ce morceau"));
    studio.say(side === "start" ? t("Début rogné à la tête de lecture") : t("Fin rognée à la tête de lecture"));
  }
  function setNum(side: "start" | "end", value: string) {
    if (!p) return;
    const v = parseFloat(value.replace(",", "."));
    if (!isFinite(v)) return;
    const max = media.info[p.path]?.duration || p.srcDur;
    if (side === "start") studio.update(p.uid, { start: Math.max(0, Math.min(v, p.end - 0.1)) });
    else studio.update(p.uid, { end: Math.min(max, Math.max(v, p.start + 0.1)) });
  }
  /** Va au repère (kill…) dans le montage. */
  function goMarker(time: number) {
    if (!p || i < 0) return;
    studio.playing = false;
    studio.seek(L.offsets[i] + (time - p.start) / clampSpeed(p.speed));
  }
  const pct = (v: number) => `${Math.round(v * 100)} %`;
</script>

<aside class="insp">
  {#if p}
    <!-- Morceau sélectionné -->
    <header class="head">
      <div class="hthumb" style="--c:{gameTone(p.game)}">{#if p.thumb}<img src={fileUrl(p.thumb)} alt="" />{/if}</div>
      <div class="htext">
        <strong>{p.title}</strong>
        <small class="faint">{t("Morceau {a} sur {b}", { a: i + 1, b: app.studio.length })} · {L.lens[i]?.toFixed(1)} s</small>
      </div>
      <button class="x" onclick={() => studio.select(null)} aria-label={t("Fermer")}><X size={16} /></button>
    </header>

    <section>
      <h4><Scissors size={13} />{t("Rogner")}</h4>
      <div class="row2">
        <label class="num"><span>{t("Début")}</span><input class="field mono" value={p.start.toFixed(2)} onchange={(e) => setNum("start", e.currentTarget.value)} /></label>
        <label class="num"><span>{t("Fin")}</span><input class="field mono" value={p.end.toFixed(2)} onchange={(e) => setNum("end", e.currentTarget.value)} /></label>
      </div>
      <div class="row2">
        <button class="btn small" onclick={() => trimHere("start")} title={t("Le morceau commence à la tête de lecture")}><ArrowLeftToLine size={14} />{t("Début ici")}</button>
        <button class="btn small" onclick={() => trimHere("end")} title={t("Le morceau finit à la tête de lecture")}><ArrowRightToLine size={14} />{t("Fin ici")}</button>
      </div>
    </section>

    <section>
      <h4><Gauge size={13} />{t("Vitesse")} <b class="val mono">{clampSpeed(p.speed)}×</b></h4>
      <div class="speeds">
        {#each SPEEDS as v}
          <button class:on={clampSpeed(p.speed) === v} onclick={() => studio.setSpeed(v)}>{v}×</button>
        {/each}
      </div>
      {#if clampSpeed(p.speed) < 1}<p class="faint note">{t("Ralenti : idéal sur un one-tap ou un ace.")}</p>{/if}
    </section>

    <section>
      <h4>
        {#if p.volume <= 0.001}<VolumeX size={13} />{:else}<Volume2 size={13} />{/if}{t("Son")}
        <b class="val mono">{pct(p.volume)}</b>
      </h4>
      <div class="slider">
        <button class="mute" class:on={p.volume <= 0.001} onclick={() => studio.update(p.uid, { volume: p.volume <= 0.001 ? 1 : 0 })} aria-label={t("Couper le son")}>
          {#if p.volume <= 0.001}<VolumeX size={15} />{:else}<Volume2 size={15} />{/if}
        </button>
        <input type="range" min="0" max="2" step="0.05" value={p.volume} onpointerdown={begin} oninput={(e) => studio.update(p.uid, { volume: +e.currentTarget.value }, false)} />
      </div>
      {#if tracks.length}
        <div class="tracks">
          {#each tracks as label, k}
            {@const v = p.trackVolumes[k] ?? 1}
            <label class="track">
              <span>{label}</span>
              <input type="range" min="0" max="2" step="0.05" value={v} onpointerdown={begin} oninput={(e) => setTrack(k, +e.currentTarget.value)} />
              <b class="mono">{pct(v)}</b>
            </label>
          {/each}
          <p class="faint note">{t("L'aperçu joue le mélange d'origine ; le réglage par piste (jeu, micro, Discord…) s'applique à l'export.")}</p>
        </div>
      {/if}
    </section>

    {#if i < app.studio.length - 1}
      <section>
        <h4><Blend size={13} />{t("Transition vers le suivant")}</h4>
        <div class="trans">
          {#each TRANSITIONS as tr}
            <button class:on={p.transition === tr.id} onclick={() => studio.setTransition(i + 1, tr.id)} title={t(tr.hint)}>{t(tr.label)}</button>
          {/each}
        </div>
        {#if p.transition !== "none"}
          <label class="track">
            <span>{t("Durée")}</span>
            <input type="range" min="0.2" max="2" step="0.1" value={p.transitionDur} onpointerdown={begin} oninput={(e) => studio.update(p.uid, { transitionDur: +e.currentTarget.value }, false)} />
            <b class="mono">{(L.trans[i + 1] || p.transitionDur).toFixed(1)} s</b>
          </label>
        {/if}
      </section>
    {/if}

    {#if p.markers.length}
      <section>
        <h4><Crosshair size={13} />{t("Moments du clip")}</h4>
        <div class="marks">
          {#each p.markers as mk}
            {@const inside = mk.time >= p.start && mk.time <= p.end}
            <button class="mark" class:out={!inside} disabled={!inside} onclick={() => goMarker(mk.time)} title={inside ? t("Aller à ce moment") : t("Rogné : hors du morceau")}>
              <i></i><span>{mk.label || KIND_LABEL[mk.kind] || mk.kind}</span><small class="mono">{fmtTime(mk.time, true)}</small>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    <div class="actions">
      <button class="btn small icon" onclick={() => studio.move(i, i - 1)} disabled={i <= 0} title={t("Déplacer avant")}><ArrowLeft size={14} /></button>
      <button class="btn small icon" onclick={() => studio.move(i, i + 1)} disabled={i >= app.studio.length - 1} title={t("Déplacer après")}><ArrowRight size={14} /></button>
      <button class="btn small" onclick={() => studio.duplicate()}><CopyPlus size={14} />{t("Dupliquer")}</button>
      <button class="btn small danger" onclick={() => studio.remove()}><Trash2 size={14} />{t("Supprimer")}</button>
    </div>
  {:else if studio.musicSelected && m}
    <!-- Musique -->
    <header class="head">
      <div class="hthumb music"><Music size={18} /></div>
      <div class="htext"><strong>{m.name}</strong><small class="faint">{t("Musique de fond")} · {fmtTime(m.duration)}</small></div>
      <button class="x" onclick={() => (studio.musicSelected = false)} aria-label={t("Fermer")}><X size={16} /></button>
    </header>
    <section>
      <h4><Volume2 size={13} />{t("Volume")} <b class="val mono">{pct(m.volume)}</b></h4>
      <input class="full" type="range" min="0" max="1.5" step="0.05" value={m.volume} onpointerdown={begin} oninput={(e) => studio.updateMusic({ volume: +e.currentTarget.value }, false)} />
      <label class="toggle">
        <span><b>{t("Atténuation automatique")}</b><small class="faint">{t("La musique baisse quand le jeu ou la voix sont forts")}</small></span>
        <span class="switch"><input type="checkbox" checked={m.duck} onchange={(e) => studio.updateMusic({ duck: e.currentTarget.checked })} /><span></span></span>
      </label>
    </section>
    <section>
      <h4>{t("Calage")}</h4>
      <label class="track"><span>{t("Début du morceau")}</span>
        <input type="range" min="0" max={Math.max(1, m.duration - 1)} step="0.1" value={m.offset} onpointerdown={begin} oninput={(e) => studio.updateMusic({ offset: +e.currentTarget.value }, false)} />
        <b class="mono">{fmtTime(m.offset, true)}</b></label>
      <label class="track"><span>{t("Départ dans le montage")}</span>
        <input type="range" min="0" max={Math.max(1, L.total - 0.5)} step="0.1" value={m.at} onpointerdown={begin} oninput={(e) => studio.updateMusic({ at: +e.currentTarget.value }, false)} />
        <b class="mono">{fmtTime(m.at, true)}</b></label>
      <p class="faint note">{t("Astuce : glisse le bloc violet sur la frise pour caler le drop de la musique sur un kill.")}</p>
    </section>
    <section>
      <h4>{t("Fondus")}</h4>
      <label class="track"><span>{t("Entrée")}</span>
        <input type="range" min="0" max="5" step="0.1" value={m.fadeIn} onpointerdown={begin} oninput={(e) => studio.updateMusic({ fadeIn: +e.currentTarget.value }, false)} />
        <b class="mono">{m.fadeIn.toFixed(1)} s</b></label>
      <label class="track"><span>{t("Sortie")}</span>
        <input type="range" min="0" max="5" step="0.1" value={m.fadeOut} onpointerdown={begin} oninput={(e) => studio.updateMusic({ fadeOut: +e.currentTarget.value }, false)} />
        <b class="mono">{m.fadeOut.toFixed(1)} s</b></label>
    </section>
    <div class="actions">
      <button class="btn small" onclick={pickMusic}><Music size={14} />{t("Remplacer")}</button>
      <button class="btn small danger" onclick={() => studio.setMusic(null)}><Trash2 size={14} />{t("Retirer")}</button>
    </div>
  {:else}
    <!-- Projet -->
    <header class="head plain"><div class="htext"><strong>{t("Projet")}</strong><small class="faint">{t("Sélectionne un morceau pour le régler")}</small></div></header>
    <section>
      <h4>{t("Format de l'aperçu")}</h4>
      <div class="formats">
        <button class:on={app.studioFormat === "16:9"} onclick={() => (app.studioFormat = "16:9")}><Monitor size={17} /><b>16:9</b><small>YouTube, Discord</small></button>
        <button class:on={app.studioFormat === "9:16-blur"} onclick={() => (app.studioFormat = "9:16-blur")}><Smartphone size={17} /><b>9:16</b><small>{t("fond flou")}</small></button>
        <button class:on={app.studioFormat === "9:16-crop"} onclick={() => (app.studioFormat = "9:16-crop")}><Smartphone size={17} /><b>9:16</b><small>{t("recadré")}</small></button>
      </div>
    </section>
    <section class="stats">
      <div><span class="faint">{t("Durée")}</span><strong class="mono">{fmtTime(L.total, true)}</strong></div>
      <div><span class="faint">{t("Morceaux")}</span><strong>{app.studio.length}</strong></div>
      <div><span class="faint">{t("Transitions")}</span><strong>{L.trans.filter((d) => d > 0).length}</strong></div>
      <div><span class="faint">{t("Musique")}</span><strong>{m ? t("Oui") : t("Non")}</strong></div>
    </section>
    <section>
      <h4><Keyboard size={13} />{t("Raccourcis")}</h4>
      <dl class="keys">
        <dt><span class="kbd">Espace</span></dt><dd>{t("Lecture / pause")}</dd>
        <dt><span class="kbd">S</span></dt><dd>{t("Couper à la tête de lecture")}</dd>
        <dt><span class="kbd">Suppr</span></dt><dd>{t("Supprimer le morceau")}</dd>
        <dt><span class="kbd">Ctrl D</span></dt><dd>{t("Dupliquer")}</dd>
        <dt><span class="kbd">Ctrl Z</span> <span class="kbd">Ctrl Y</span></dt><dd>{t("Annuler / rétablir")}</dd>
        <dt><span class="kbd">←</span> <span class="kbd">→</span></dt><dd>{t("Image par image (Maj : 1 s)")}</dd>
        <dt><span class="kbd">↑</span> <span class="kbd">↓</span></dt><dd>{t("Coupe précédente / suivante")}</dd>
        <dt><span class="kbd">+</span> <span class="kbd">−</span></dt><dd>{t("Zoom de la frise (aussi Ctrl + molette)")}</dd>
      </dl>
    </section>
    {#if app.studio.length}
      <div class="actions"><button class="btn small ghost danger" onclick={() => studio.clear()}><Trash2 size={14} />{t("Vider le montage")}</button></div>
    {/if}
  {/if}
</aside>

<style>
  .insp { display: flex; flex-direction: column; min-height: 0; overflow-y: auto; background: var(--bg-2); border-left: 1px solid var(--line); padding-bottom: 16px; }
  .head { display: flex; align-items: center; gap: 10px; padding: 12px; border-bottom: 1px solid var(--line); }
  .head.plain { padding: 14px 12px; }
  .hthumb { width: 64px; aspect-ratio: 16/9; border-radius: 7px; overflow: hidden; background: var(--panel-3); flex: none; box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c, #888) 40%, transparent); }
  .hthumb img { width: 100%; height: 100%; object-fit: cover; }
  .hthumb.music { display: grid; place-items: center; background: #7b86ff; color: #fff; width: 40px; border-radius: 50%; aspect-ratio: 1; }
  .htext { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .htext strong { font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .htext small { font-size: 11.5px; }
  .x { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; color: var(--text-3); flex: none; }
  .x:hover { background: var(--panel-3); color: var(--text); }
  section { padding: 12px; border-bottom: 1px solid var(--line); display: flex; flex-direction: column; gap: 8px; }
  h4 { margin: 0; display: flex; align-items: center; gap: 6px; font-size: 11.5px; font-weight: 650; letter-spacing: .05em; text-transform: uppercase; color: var(--text-3); }
  h4 .val { margin-left: auto; font-size: 12px; color: var(--text); text-transform: none; letter-spacing: 0; }
  .row2 { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .num { display: flex; flex-direction: column; gap: 3px; font-size: 11px; color: var(--text-3); }
  .num .field { height: 32px; width: 100%; font-size: 12.5px; }
  .speeds { display: grid; grid-template-columns: repeat(5, 1fr); gap: 4px; }
  .speeds button, .trans button { height: 28px; border-radius: 7px; font-size: 12px; font-weight: 600; color: var(--text-2); border: 1px solid var(--line); transition: all var(--dur); }
  .speeds button { font-family: var(--mono); }
  .speeds button:hover, .trans button:hover { color: var(--text); border-color: var(--line-2); }
  .speeds button.on, .trans button.on { background: var(--hi); color: var(--hi-ink); border-color: transparent; }
  .trans { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; }
  .slider { display: flex; align-items: center; gap: 8px; }
  .slider input, .full { flex: 1; width: 100%; }
  .mute { width: 30px; height: 30px; border-radius: 8px; display: grid; place-items: center; color: var(--text-2); border: 1px solid var(--line); flex: none; }
  .mute.on { background: var(--rec); color: #fff; border-color: transparent; }
  .tracks { display: flex; flex-direction: column; gap: 6px; margin-top: 4px; }
  .track { display: grid; grid-template-columns: 92px 1fr 52px; align-items: center; gap: 8px; font-size: 12px; color: var(--text-2); }
  .track span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .track b { font-size: 11.5px; text-align: right; color: var(--text); font-weight: 500; }
  .note { font-size: 11.5px; margin: 0; line-height: 1.45; }
  .marks { display: flex; flex-direction: column; gap: 2px; max-height: 180px; overflow-y: auto; }
  .mark { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 8px; font-size: 12px; text-align: left; color: var(--text-2); }
  .mark:hover:not(:disabled) { background: var(--panel-2); color: var(--text); }
  .mark i { width: 8px; height: 8px; transform: rotate(45deg); background: #ffcf3f; border-radius: 2px; flex: none; }
  .mark span { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .mark.out { opacity: .4; }
  .actions { display: flex; gap: 6px; padding: 12px; }
  .toggle { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-top: 4px; }
  .toggle > span:first-child { display: flex; flex-direction: column; font-size: 12.5px; }
  .toggle small { font-size: 11px; }
  .formats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .formats button { display: flex; flex-direction: column; align-items: center; gap: 2px; padding: 9px 4px; border-radius: 10px; border: 1px solid var(--line); color: var(--text-2); transition: all var(--dur); }
  .formats button b { font-size: 12.5px; color: var(--text); }
  .formats button small { font-size: 10.5px; color: var(--text-3); }
  .formats button.on { border-color: var(--hi); background: color-mix(in srgb, var(--hi) 10%, transparent); color: var(--hi); }
  .stats { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .stats div { display: flex; flex-direction: column; gap: 1px; padding: 8px 10px; border-radius: 10px; background: var(--panel); border: 1px solid var(--line); font-size: 11.5px; }
  .stats strong { font-size: 15px; }
  .keys { display: grid; grid-template-columns: auto 1fr; gap: 6px 10px; margin: 0; font-size: 12px; align-items: center; }
  .keys dt { white-space: nowrap; }
  .keys dd { margin: 0; color: var(--text-2); }
</style>
