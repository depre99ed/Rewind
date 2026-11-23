<script lang="ts">
  // Panneau de droite : seulement les réglages de ce qui est choisi (morceau, calque, son, touches, transition).
  import {
    Diamond, Trash2, Snowflake, AlignLeft, AlignCenter, AlignRight, Monitor, Smartphone, Square, Keyboard, MousePointerClick,
    ArrowLeftToLine, ArrowRightToLine, ArrowLeft, ArrowRight, CopyPlus, Volume2, VolumeX, Gauge, Palette, ZoomIn, Scissors,
    Crosshair, Blend, Type, Sparkles, Move, SunMoon, Droplets, Film, AudioLines, Clock3, LayoutTemplate,
  } from "@lucide/svelte";
  import { studio, media, TRANSITIONS, SPEEDS, fmtTime } from "$lib/studio/store.svelte";
  import { KIND_LABEL } from "$lib/api";
  import { momentTone } from "$lib/moments";
  import MomentIcon from "../MomentIcon.svelte";
  import Slider from "./Slider.svelte";
  import SpeedCurve from "./SpeedCurve.svelte";
  import { kfValue, outLen, localAt, fxInfo, isFilterFx, isPunch, isFullBlur, hasCurve, type Color, type Fit, type Format, type TextAnimIn, type TextAnimOut } from "$lib/studio/model";
  import { FONTS, fontOf } from "$lib/studio/fonts";
  import FontPicker from "./FontPicker.svelte";
  import { LOOKS as TEXT_LOOKS, type TextLook } from "$lib/studio/textfx";
  import { t } from "$lib/i18n.svelte";

  const P = $derived(studio.project);
  const piece = $derived(studio.piece);
  const pIndex = $derived(studio.pieceIndex);
  const layer = $derived(studio.layer);
  const audio = $derived(studio.audio);
  const junction = $derived(studio.junction);

  // Valeurs « au moment de la tête de lecture » (avec les points clés)
  const local = $derived(layer ? Math.max(0, Math.min(layer.dur, studio.t - layer.at)) : 0);
  const lv = (prop: string, base: number) => (layer ? kfValue(layer.kf, prop, local, base) : base);
  const onKf = $derived(layer ? layer.kf.some((k) => Math.abs(k.t - local) < 0.04) : false);
  const pLocal = $derived(piece && pIndex >= 0 ? Math.max(0, Math.min(outLen(piece), studio.t - studio.lay.offsets[pIndex])) : 0);
  const onZoomKf = $derived(piece ? piece.zoom.some((k) => Math.abs(k.t - pLocal) < 0.04) : false);
  const zoomV = (prop: string, base: number) => (piece ? kfValue(piece.zoom, prop, pLocal, base) : base);
  const aLocal = $derived(audio ? Math.max(0, Math.min(audio.dur, studio.t - audio.at)) : 0);
  const onVolKf = $derived(audio ? audio.kf.some((k) => Math.abs(k.t - aLocal) < 0.04) : false);

  const FORMATS: { id: Format; hint: string; icon: typeof Monitor }[] = [
    { id: "16:9", hint: "YouTube, Discord", icon: Monitor },
    { id: "9:16", hint: "TikTok, Shorts", icon: Smartphone },
    { id: "1:1", hint: "Instagram", icon: Square },
  ];
  const COLORS = ["#ffffff", "#000000", "#ff5d5d", "#ffd25a", "#3ddc97", "#38bdf8", "#b07cff"];
  const LOOKS: { id: string; label: string; c: Color }[] = [
    { id: "none", label: "Normal", c: { b: 1, c: 1, s: 1 } },
    { id: "vivid", label: "Vif", c: { b: 1.05, c: 1.12, s: 1.35 } },
    { id: "cine", label: "Ciné", c: { b: 0.95, c: 1.18, s: 0.85 } },
    { id: "bright", label: "Lumineux", c: { b: 1.2, c: 1.02, s: 1.05 } },
    { id: "bw", label: "Noir et blanc", c: { b: 1, c: 1.15, s: 0 } },
  ];
  const ANIM_IN: { id: TextAnimIn; label: string }[] = [
    { id: "none", label: "Aucune" }, { id: "fade", label: "Fondu" }, { id: "pop", label: "Pop" }, { id: "slide", label: "Glisser" }, { id: "type", label: "Machine à écrire" },
  ];
  /** Animations lettre par lettre (chaque lettre arrive à son tour) */
  const ANIM_IN_LETTERS: { id: TextAnimIn; label: string }[] = [
    { id: "letters", label: "Lettre par lettre" }, { id: "drop", label: "Chute" }, { id: "rise", label: "Montée" }, { id: "spin", label: "Tourbillon" }, { id: "blur", label: "Mise au point" }, { id: "glitch", label: "Glitch" },
  ];
  const ANIM_OUT_LETTERS: { id: TextAnimOut; label: string }[] = [
    { id: "letters", label: "Lettre par lettre" }, { id: "drop", label: "Chute" }, { id: "blur", label: "Flou" },
  ];
  const ANIM_OUT: { id: TextAnimOut; label: string }[] = [
    { id: "none", label: "Aucune" }, { id: "fade", label: "Fondu" }, { id: "pop", label: "Pop" }, { id: "slide", label: "Glisser" },
  ];

  /** Rogne le morceau choisi à la tête de lecture (début ou fin). */
  function trimHere(side: "start" | "end") {
    if (!piece || pIndex < 0 || piece.kind === "freeze") return;
    const src = studio.srcTime(pIndex, studio.t);
    if (side === "start" && piece.end - src >= 0.2) studio.updatePiece(piece.uid, { start: src });
    else if (side === "end" && src - piece.start >= 0.2) studio.updatePiece(piece.uid, { end: src });
    else studio.say("Place la tête de lecture dans le morceau");
  }

  // Historique : une seule entrée par geste (au début du glissement d'un curseur)
  /** Rendu « edit » d'un texte : modifie un réglage (part du chrome 3D si le texte était plat) */
  function setLook(id: string, cur: TextLook | null | undefined, patch: Partial<TextLook>, rec = true) {
    studio.updateLayer(id, { look: { ...(cur ?? TEXT_LOOKS[0].look), ...patch } }, rec);
  }
  let fontPicker = $state(false);
  const begin = () => studio.snapshot();
  /** Courbes toutes faites, calées sur la tête de lecture (le moment fort) quand elle est dans le morceau */
  function setCurve(kind: "none" | "slowfast" | "fastslow" | "dip") {
    if (!piece) return;
    if (kind === "none") return studio.updatePiece(piece.uid, { curve: [] });
    const a = piece.start, b = piece.end, len = b - a;
    const now = studio.srcTime(pIndex, studio.t);
    const h = Math.min(b - len * 0.15, Math.max(a + len * 0.15, now > a && now < b ? now : a + len * 0.4));
    const d = Math.max(0.15, len * 0.08);
    const curve =
      kind === "slowfast" ? [{ t: a, v: 0.3 }, { t: h, v: 0.3 }, { t: b, v: 2.5 }]
      : kind === "fastslow" ? [{ t: a, v: 2.5 }, { t: h, v: 0.3 }, { t: b, v: 0.3 }]
      : [{ t: a, v: 2 }, { t: Math.max(a + 0.02, h - d), v: 0.3 }, { t: Math.min(b - 0.02, h + d), v: 0.3 }, { t: b, v: 2 }];
    studio.updatePiece(piece.uid, { curve });
  }
  const finish = () => {};
  const pct = (v: number) => `${Math.round(v * 100)} %`;
  const sec = (v: number) => `${v.toFixed(1)} s`;
  const x100 = (v: number) => `${v.toFixed(2)}×`;
  const deg = (v: number) => `${Math.round(v)}°`;
  const int = (v: number) => String(Math.round(v));
  const parsePct = (s: string) => parseFloat(s.replace(",", ".")) / 100;
</script>

<aside class="insp">
  {#if junction !== null && P.pieces[junction - 1]}
    {@const prev = P.pieces[junction - 1]}
    <div class="title"><Blend size={15} /><h3>{t("Transition")}</h3></div>
    <div class="grid3">
      {#each TRANSITIONS as tr}
        <button class="jt" class:on={prev.transition === tr.id} onclick={() => studio.setTransition(junction, tr.id)} title={t(tr.hint)}>
          <span class="jprev {tr.id}"><i></i><i></i></span>{t(tr.label)}
        </button>
      {/each}
    </div>
    {#if prev.transition !== "none"}
      <Slider label={t("Durée")} value={prev.transitionDur} min={0.1} max={2} step={0.1} def={0.5} format={sec} onstart={begin} oninput={(v) => studio.updatePiece(prev.uid, { transitionDur: v }, false)} onend={finish} />
    {/if}
    <button class="btn small" onclick={() => studio.setTransitionAll(prev.transition)}>{t("Appliquer à toutes les coupes")}</button>

  {:else if piece}
    <div class="title">{#if piece.kind === "freeze"}<Snowflake size={15} />{:else}<Film size={15} />{/if}<h3>{piece.kind === "freeze" ? t("Image figée") : piece.title}</h3></div>
    {#if piece.kind === "freeze"}
      <section>
        <header><Clock3 size={14} /><h4>{t("Durée")}</h4></header>
        <Slider label={t("Durée")} value={piece.hold} min={0.2} max={10} step={0.1} def={2} format={sec} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { hold: v }, false)} />
      </section>
    {:else}
      <section>
        <header><Scissors size={14} /><h4>{t("Rogner")}</h4></header>
        <div class="two">
          <button class="btn small" onclick={() => trimHere("start")} title={t("Le morceau commence à la tête de lecture")}><ArrowLeftToLine size={14} />{t("Début ici")}</button>
          <button class="btn small" onclick={() => trimHere("end")} title={t("Le morceau finit à la tête de lecture")}><ArrowRightToLine size={14} />{t("Fin ici")}</button>
        </div>
      </section>
      <section>
        <header><Gauge size={14} /><h4>{t("Vitesse")}</h4></header>
        <div class="chips">{#each SPEEDS as v}<button class="chip" class:on={!hasCurve(piece) && piece.speed === v} onclick={() => studio.setSpeed(v)}>{v}×</button>{/each}</div>
        <span class="mini">{t("Courbe de vitesse")}</span>
        <div class="chips">
          <button class="chip" class:on={!hasCurve(piece)} onclick={() => setCurve("none")}>{t("Aucune")}</button>
          <button class="chip" onclick={() => setCurve("slowfast")}>{t("Ralenti → accéléré")}</button>
          <button class="chip" onclick={() => setCurve("fastslow")}>{t("Accéléré → ralenti")}</button>
          <button class="chip" onclick={() => setCurve("dip")}>{t("Creux au milieu")}</button>
        </div>
        {#if hasCurve(piece)}
          <SpeedCurve {piece} src={studio.srcTime(pIndex, studio.t)} onstart={begin} onchange={(c, record) => studio.updatePiece(piece.uid, { curve: c }, record)} />
          <p class="note">{t("Tire les points. Clic sur la courbe : un point de plus ; double-clic sur un point : retiré. La vitesse passe en douceur d'un point à l'autre ; textes et effets suivent l'image.")}</p>
        {/if}
        <label class="row"><span>{t("Ralenti progressif")}</span>
          <span class="switch"><input type="checkbox" checked={!!piece.ramp} onchange={(e) => studio.updatePiece(piece.uid, { ramp: e.currentTarget.checked ? { at: studio.srcTime(pIndex, studio.t), dur: 0.8, speed: 0.3, ease: 0.4 } : null })} /><span></span></span>
        </label>
        {#if piece.ramp}
          {@const r = piece.ramp}
          <p class="note">{t("La vitesse descend en douceur autour de {x}, puis remonte.", { x: fmtTime(r.at - piece.start, true) })}</p>
          <button class="btn small" onclick={() => studio.updatePiece(piece.uid, { ramp: { ...r, at: studio.srcTime(pIndex, studio.t) } })}><MousePointerClick size={14} />{t("Centrer sur la tête de lecture")}</button>
          <Slider label={t("Plus lent")} value={r.speed} min={0.1} max={0.8} step={0.05} def={0.3} format={x100} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { ramp: { ...r, speed: v } }, false)} />
          <Slider label={t("Durée")} value={r.dur} min={0.2} max={4} step={0.1} def={0.8} format={sec} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { ramp: { ...r, dur: v } }, false)} />
          <Slider label={t("Douceur")} value={r.ease} min={0.1} max={1.5} step={0.1} def={0.4} format={sec} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { ramp: { ...r, ease: v } }, false)} />
        {/if}
      </section>
      <section>
        <header><Volume2 size={14} /><h4>{t("Son")}</h4>
          <button class="hbtn" class:on={piece.volume <= 0.001} onclick={() => studio.updatePiece(piece.uid, { volume: piece.volume <= 0.001 ? 1 : 0 })} title={t(piece.volume <= 0.001 ? "Remettre le son" : "Couper le son")}>{#if piece.volume <= 0.001}<VolumeX size={13} />{:else}<Volume2 size={13} />{/if}</button>
        </header>
        <Slider label={t("Volume")} value={piece.volume} min={0} max={2} step={0.05} def={1} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { volume: v }, false)} />
        {#each media.separateTracks(piece.path) as name, i}
          <Slider label={t(name)} value={piece.trackVolumes[i] ?? 1} min={0} max={2} step={0.05} def={1} format={pct} parse={parsePct} onstart={begin}
            oninput={(v) => { const list = [...piece.trackVolumes]; while (list.length < media.separateTracks(piece.path).length) list.push(1); list[i] = v; studio.updatePiece(piece.uid, { trackVolumes: list }, false); }} />
        {/each}
      </section>
      {#if piece.markers.length}
        <section>
          <header><Crosshair size={14} /><h4>{t("Moments du clip")}</h4></header>
          <div class="marks">
            {#each piece.markers as mk}
              {@const lt = localAt(piece, mk.time)}
              <button class="mark" disabled={lt === null} onclick={() => lt !== null && studio.seek(studio.lay.offsets[pIndex] + Math.max(0, lt - 1))} title={lt !== null ? t("Aller à ce moment") : t("Rogné : hors du morceau")}>
                <i class={momentTone(mk.kind)}><MomentIcon kind={mk.kind} size={14} /></i><span>{mk.label || KIND_LABEL[mk.kind] || mk.kind}</span><small class="mono">{fmtTime(mk.time, true)}</small>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    {/if}
    <section>
      <header><Palette size={14} /><h4>{t("Couleur")}</h4></header>
      <div class="chips">{#each LOOKS as lk}<button class="chip" class:on={piece.color.b === lk.c.b && piece.color.c === lk.c.c && piece.color.s === lk.c.s} onclick={() => studio.updatePiece(piece.uid, { color: { ...lk.c } })}>{t(lk.label)}</button>{/each}</div>
      <Slider label={t("Luminosité")} value={piece.color.b} min={0.5} max={1.6} step={0.01} def={1} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { color: { ...piece.color, b: v } }, false)} />
      <Slider label={t("Contraste")} value={piece.color.c} min={0.5} max={1.6} step={0.01} def={1} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { color: { ...piece.color, c: v } }, false)} />
      <Slider label={t("Saturation")} value={piece.color.s} min={0} max={2} step={0.01} def={1} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updatePiece(piece.uid, { color: { ...piece.color, s: v } }, false)} />
    </section>
    <section>
      <header><ZoomIn size={14} /><h4>{t("Zoom")}</h4>
        <button class="hbtn" class:on={onZoomKf} onclick={() => studio.toggleZoomKf(piece.uid)} title={t("Point clé de zoom à la tête de lecture")}><Diamond size={13} /></button>
      </header>
      <p class="note">{piece.zoom.length ? t("{n} points clés : le zoom se déplace en douceur de l'un à l'autre.", { n: piece.zoom.length }) : t("Pose un point clé ◆, avance la lecture, change le zoom : il s'animera tout seul.")}</p>
      <Slider label={t("Zoom")} value={zoomV("z", 1)} min={1} max={3} step={0.01} def={1} format={x100} onstart={begin} oninput={(v) => studio.setZoomAt(piece.uid, { z: v }, false)} />
      <Slider label={t("Horizontal")} value={zoomV("x", 0.5)} min={0} max={1} step={0.01} def={0.5} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.setZoomAt(piece.uid, { x: v }, false)} />
      <Slider label={t("Vertical")} value={zoomV("y", 0.5)} min={0} max={1} step={0.01} def={0.5} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.setZoomAt(piece.uid, { y: v }, false)} />
      {#if piece.zoom.length}<button class="btn small ghost" onclick={() => studio.updatePiece(piece.uid, { zoom: [] })}><Trash2 size={14} />{t("Retirer le zoom")}</button>{/if}
    </section>
    {#if pIndex < P.pieces.length - 1}
      <section>
        <header><Blend size={14} /><h4>{t("Transition vers le suivant")}</h4></header>
        <div class="chips">{#each TRANSITIONS as tr}<button class="chip" class:on={piece.transition === tr.id} onclick={() => studio.setTransition(pIndex + 1, tr.id)} title={t(tr.hint)}>{t(tr.label)}</button>{/each}</div>
      </section>
    {/if}
    {#if piece.kind !== "freeze"}<button class="btn small" onclick={() => studio.freezeFrame()}><Snowflake size={14} />{t("Figer l'image ici (2 s)")}</button>{/if}
    <div class="acts">
      <button class="btn small icon" onclick={() => studio.movePiece(pIndex, pIndex - 1)} disabled={pIndex <= 0} title={t("Déplacer avant")}><ArrowLeft size={14} /></button>
      <button class="btn small icon" onclick={() => studio.movePiece(pIndex, pIndex + 1)} disabled={pIndex >= P.pieces.length - 1} title={t("Déplacer après")}><ArrowRight size={14} /></button>
      <button class="btn small" onclick={() => studio.duplicate()}><CopyPlus size={14} />{t("Dupliquer")}</button>
      <button class="btn small danger" onclick={() => studio.removeSel()}><Trash2 size={14} />{t("Supprimer")}</button>
    </div>

  {:else if layer}
    <div class="title">{#if layer.kind === "text"}<Type size={15} />{:else if layer.kind === "blur"}<Droplets size={15} />{:else if layer.kind === "fx"}<Sparkles size={15} />{:else}<Film size={15} />{/if}<h3>{layer.kind === "text" ? t("Texte") : layer.kind === "blur" ? t(isFullBlur(layer) ? (layer.mode === "pixel" ? "Image pixelisée" : "Image floutée") : "Zone floutée") : layer.kind === "fx" ? t(fxInfo(layer.fx).label) : layer.name}</h3></div>
    {#if layer.kind === "text"}
      <textarea class="field area" value={layer.text} onfocus={begin} oninput={(e) => studio.updateLayer(layer.uid, { text: e.currentTarget.value }, false)} rows="2"></textarea>
      <section>
        <header><Type size={14} /><h4>{t("Police")}</h4></header>
        <!-- Toutes les polices en grand, dans une fenêtre plein écran -->
        <button class="fontpick" onclick={() => (fontPicker = true)} title={t("Voir toutes les polices en grand")}>
          <span class="fp-sample" style="font-family:'{fontOf(layer.font).family}';font-weight:{fontOf(layer.font).weight}">{(layer.text || fontOf(layer.font).label).split(/\r?\n/)[0].slice(0, 18)}</span>
          <span class="fp-meta"><b>{fontOf(layer.font).label}</b><small>{t("{n} polices · voir en grand", { n: FONTS.length })}</small></span>
        </button>
        {#if fontPicker}
          <FontPicker text={layer.text} current={layer.font} look={layer.look} color={layer.color} onpick={(id, look) => studio.updateLayer(layer.uid, { font: id, look, ...(look && look.fill !== "solid" ? { color: "#ffffff" } : {}) })} onclose={() => (fontPicker = false)} />
        {/if}
      </section>
      <section>
        <header><Sparkles size={14} /><h4>{t("Rendu")}</h4></header>
        <div class="chips">
          <button class="chip" class:on={!layer.look} onclick={() => studio.updateLayer(layer.uid, { look: null })}>{t("Plat")}</button>
          {#each TEXT_LOOKS as lk}<button class="chip" class:on={!!layer.look && JSON.stringify(layer.look) === JSON.stringify(lk.look)} onclick={() => studio.updateLayer(layer.uid, { look: structuredClone(lk.look), ...(lk.look.fill !== "solid" ? { color: "#ffffff" } : {}) })}>{t(lk.label)}</button>{/each}
        </div>
        {#if layer.look}
          {#if layer.look.fill === "metal"}
            <div class="row"><span>{t("Teinte du métal")}</span><label class="sw custom" style="background:{layer.look.tint}" title={t("Teinte du métal")}><input type="color" value={layer.look.tint} onchange={(e) => setLook(layer.uid, layer.look, { tint: e.currentTarget.value })} /></label></div>
          {/if}
          <Slider label={t("Relief 3D")} value={layer.look.depth} min={0} max={1} step={0.05} def={0.55} format={pct} parse={parsePct} onstart={begin} oninput={(v) => setLook(layer.uid, layer.look, { depth: v }, false)} />
          {#if layer.look.fill !== "solid"}<Slider label={t("Biseau")} value={layer.look.bevel} min={0} max={1} step={0.05} def={0.6} format={pct} parse={parsePct} onstart={begin} oninput={(v) => setLook(layer.uid, layer.look, { bevel: v }, false)} />{/if}
          <Slider label={t("Étincelles")} value={layer.look.sparkle} min={0} max={1} step={0.05} def={0} format={pct} parse={parsePct} onstart={begin} oninput={(v) => setLook(layer.uid, layer.look, { sparkle: v }, false)} />
          <label class="row"><span>{t("Éclat")}</span><span class="switch"><input type="checkbox" checked={!!layer.look.glow} onchange={(e) => setLook(layer.uid, layer.look, { glow: e.currentTarget.checked ? { color: layer.look?.fill === "solid" ? "#c8f031" : "#ffffff", strength: 0.45, size: 0.45 } : null })} /><span></span></span></label>
          {#if layer.look.glow}
            {@const g = layer.look.glow}
            <div class="sub">
              <label class="sw custom" style="background:{g.color}" title={t("Couleur de l'éclat")}><input type="color" value={g.color} onchange={(e) => setLook(layer.uid, layer.look, { glow: { ...g, color: e.currentTarget.value } })} /></label>
              <Slider label={t("Force")} value={g.strength} min={0.05} max={1} step={0.05} def={0.45} format={pct} parse={parsePct} onstart={begin} oninput={(v) => setLook(layer.uid, layer.look, { glow: { ...g, strength: v } }, false)} />
              <Slider label={t("Taille")} value={g.size} min={0.05} max={1} step={0.05} def={0.45} format={pct} parse={parsePct} onstart={begin} oninput={(v) => setLook(layer.uid, layer.look, { glow: { ...g, size: v } }, false)} />
            </div>
          {/if}
          <p class="note">{t("Le rendu marche avec toutes les polices. Le métal ignore la couleur du texte ; le néon l'utilise.")}</p>
        {/if}
      </section>
      <section>
        <header><Palette size={14} /><h4>{t("Style")}</h4></header>
        <Slider label={t("Taille")} value={layer.size} min={16} max={300} step={1} def={120} format={int} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { size: v }, false)} />
        <div class="row"><span>{t("Couleur")}</span>
          <div class="swatches">
            {#each COLORS as c}<button class="sw" class:on={layer.color === c} style="background:{c}" onclick={() => studio.updateLayer(layer.uid, { color: c })} aria-label={c}></button>{/each}
            <label class="sw custom" title={t("Autre couleur")}><input type="color" value={layer.color} onchange={(e) => studio.updateLayer(layer.uid, { color: e.currentTarget.value })} /></label>
          </div>
        </div>
        <div class="seg3">
          <button class:on={layer.align === "left"} onclick={() => studio.updateLayer(layer.uid, { align: "left" })} aria-label={t("Aligné à gauche")}><AlignLeft size={15} /></button>
          <button class:on={layer.align === "center"} onclick={() => studio.updateLayer(layer.uid, { align: "center" })} aria-label={t("Centré")}><AlignCenter size={15} /></button>
          <button class:on={layer.align === "right"} onclick={() => studio.updateLayer(layer.uid, { align: "right" })} aria-label={t("Aligné à droite")}><AlignRight size={15} /></button>
        </div>
        <label class="row"><span>{t("Contour")}</span><span class="switch"><input type="checkbox" checked={!!layer.stroke} onchange={(e) => studio.updateLayer(layer.uid, { stroke: e.currentTarget.checked ? { color: "#000000", width: 6 } : null })} /><span></span></span></label>
        {#if layer.stroke}
          <div class="sub">
            <label class="sw custom" style="background:{layer.stroke.color}" title={t("Couleur du contour")}><input type="color" value={layer.stroke.color} onchange={(e) => studio.updateLayer(layer.uid, { stroke: { ...layer.stroke!, color: e.currentTarget.value } })} /></label>
            <Slider label={t("Épaisseur")} value={layer.stroke.width} min={1} max={20} step={1} def={6} format={int} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { stroke: { ...layer.stroke!, width: v } }, false)} />
          </div>
        {/if}
        <label class="row"><span>{t("Ombre")}</span><span class="switch"><input type="checkbox" checked={layer.shadow} onchange={(e) => studio.updateLayer(layer.uid, { shadow: e.currentTarget.checked })} /><span></span></span></label>
        <label class="row"><span>{t("Fond")}</span><span class="switch"><input type="checkbox" checked={!!layer.bg} onchange={(e) => studio.updateLayer(layer.uid, { bg: e.currentTarget.checked ? { color: "#000000", opacity: 0.55 } : null })} /><span></span></span></label>
        {#if layer.bg}
          <div class="sub">
            <label class="sw custom" style="background:{layer.bg.color}" title={t("Couleur du fond")}><input type="color" value={layer.bg.color} onchange={(e) => studio.updateLayer(layer.uid, { bg: { ...layer.bg!, color: e.currentTarget.value } })} /></label>
            <Slider label={t("Opacité")} value={layer.bg.opacity} min={0.1} max={1} step={0.05} def={0.55} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { bg: { ...layer.bg!, opacity: v } }, false)} />
          </div>
        {/if}
      </section>
      <section>
        <header><Sparkles size={14} /><h4>{t("Animation")}</h4></header>
        <span class="mini">{t("Entrée")}</span>
        <div class="chips">{#each ANIM_IN as a}<button class="chip" class:on={layer.animIn === a.id} onclick={() => studio.updateLayer(layer.uid, { animIn: a.id })}>{t(a.label)}</button>{/each}</div>
        <span class="mini">{t("Lettre par lettre")}</span>
        <div class="chips">{#each ANIM_IN_LETTERS as a}<button class="chip" class:on={layer.animIn === a.id} onclick={() => studio.updateLayer(layer.uid, { animIn: a.id })}>{t(a.label)}</button>{/each}</div>
        <span class="mini">{t("Sortie")}</span>
        <div class="chips">{#each ANIM_OUT as a}<button class="chip" class:on={layer.animOut === a.id} onclick={() => studio.updateLayer(layer.uid, { animOut: a.id })}>{t(a.label)}</button>{/each}{#each ANIM_OUT_LETTERS as a}<button class="chip" class:on={layer.animOut === a.id} onclick={() => studio.updateLayer(layer.uid, { animOut: a.id })}>{t(a.label)}</button>{/each}</div>
        <Slider label={t("Durée")} value={layer.animDur} min={0.1} max={1.5} step={0.05} def={0.35} format={(v) => `${v.toFixed(2)} s`} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { animDur: v }, false)} />
      </section>
    {:else if layer.kind === "video"}
      <section>
        <header><Volume2 size={14} /><h4>{t("Son et vitesse")}</h4></header>
        <Slider label={t("Volume")} value={layer.volume} min={0} max={2} step={0.05} def={1} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { volume: v }, false)} />
        <div class="chips">{#each [0.5, 1, 1.5, 2] as v}<button class="chip" class:on={layer.speed === v} onclick={() => studio.updateLayer(layer.uid, { speed: v, dur: Math.min(layer.dur, (layer.srcDur - layer.srcStart) / v) })}>{v}×</button>{/each}</div>
      </section>
    {:else if layer.kind === "blur"}
      <section>
        <header><Droplets size={14} /><h4>{t("Flou")}</h4></header>
        <div class="seg3">
          <button class:on={layer.mode === "blur"} onclick={() => studio.updateLayer(layer.uid, { mode: "blur" })}>{t("Flou")}</button>
          <button class:on={layer.mode === "pixel"} onclick={() => studio.updateLayer(layer.uid, { mode: "pixel" })}>{t("Pixels")}</button>
        </div>
        <Slider label={t("Intensité")} value={layer.strength} min={4} max={48} step={1} def={16} format={int} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { strength: v }, false)} />
        <Slider label={t("Largeur")} value={layer.w} min={0.02} max={1} step={0.01} def={0.24} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { w: v }, false)} />
        <Slider label={t("Hauteur")} value={layer.h} min={0.02} max={1} step={0.01} def={0.08} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { h: v }, false)} />
      </section>
    {:else if layer.kind === "fx"}
      {@const info = fxInfo(layer.fx)}
      <section>
        <header><Sparkles size={14} /><h4>{t("Effet")}</h4></header>
        <p class="note">{t(info.hint)}. {t(isPunch(layer) ? "Agit sur le zoom de la vidéo, avec le zoom du morceau." : isFilterFx(layer) ? "Agit sur toute l'image (vidéo et calques), pas sur les touches." : "Image plein cadre : une piste au-dessus passe devant.")}</p>
        {#if layer.fx === "flash"}
          <label class="fxcolor">{t("Couleur")}<input type="color" value={layer.color || "#ffffff"} onchange={(e) => studio.updateLayer(layer.uid, { color: e.currentTarget.value })} /></label>
        {/if}
        {#if info.slider}
          <Slider label={t("Intensité")} value={layer.strength} min={0.05} max={1} step={0.01} def={info.strength} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { strength: v }, false)} />
        {/if}
        <Slider label={t("Durée")} value={layer.dur} min={0.1} max={Math.max(10, layer.dur)} step={0.05} def={info.dur} format={(v) => `${v.toFixed(2)} s`} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { dur: v, fadeIn: Math.min(layer.fadeIn, v), fadeOut: Math.min(layer.fadeOut, v) }, false)} />
        <button class="btn small" onclick={() => studio.updateLayer(layer.uid, { at: studio.t })}><MousePointerClick size={14} />{t("Placer à la tête de lecture")}</button>
      </section>
    {/if}

    {#if layer.kind !== "fx"}
    <section>
      <header><Move size={14} /><h4>{t("Mouvement")}</h4>
        <button class="hbtn" class:on={onKf} onclick={() => studio.toggleKf(layer.uid)} title={t("Point clé à la tête de lecture")}><Diamond size={13} /></button>
      </header>
      <p class="note">{layer.kf.length ? t("{n} points clés : déplace ou agrandis le calque à un autre moment, il s'animera.", { n: layer.kf.length }) : t("Déplace et agrandis le calque directement sur l'aperçu. Pour l'animer, pose un point clé ◆.")}</p>
      {#if layer.kind !== "blur"}
        <Slider label={t("Taille")} value={lv("scale", layer.scale)} min={0.05} max={layer.kind === "text" ? 3 : 1.5} step={0.01} def={layer.kind === "text" ? 1 : undefined} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.setAnimated(layer.uid, { scale: v }, false)} />
        <Slider label={t("Rotation")} value={lv("rot", layer.rot)} min={-180} max={180} step={1} def={0} format={deg} onstart={begin} oninput={(v) => studio.setAnimated(layer.uid, { rot: v }, false)} />
      {/if}
      <Slider label={t("Opacité")} value={lv("opacity", layer.opacity)} min={0} max={1} step={0.01} def={1} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.setAnimated(layer.uid, { opacity: v }, false)} />
      {#if layer.kf.length}<button class="btn small ghost" onclick={() => studio.updateLayer(layer.uid, { kf: [] })}><Trash2 size={14} />{t("Retirer les points clés")}</button>{/if}
    </section>
    {/if}
    {#if !isFilterFx(layer) && !isPunch(layer)}
    <section>
      <header><SunMoon size={14} /><h4>{t("Fondus")}</h4></header>
      <Slider label={t("Entrée")} value={layer.fadeIn} min={0} max={3} step={0.05} def={0} format={sec} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { fadeIn: v }, false)} />
      <Slider label={t("Sortie")} value={layer.fadeOut} min={0} max={3} step={0.05} def={0} format={sec} onstart={begin} oninput={(v) => studio.updateLayer(layer.uid, { fadeOut: v }, false)} />
    </section>
    {/if}
    <div class="acts">
      <button class="btn small" onclick={() => studio.duplicate()}><CopyPlus size={14} />{t("Dupliquer")}</button>
      <button class="btn small danger" onclick={() => studio.removeSel()}><Trash2 size={14} />{t("Supprimer")}</button>
    </div>

  {:else if audio}
    <div class="title"><AudioLines size={15} /><h3>{audio.name}</h3></div>
    <section>
      <header><Volume2 size={14} /><h4>{t("Volume")}</h4>
        <button class="hbtn" class:on={onVolKf} onclick={() => studio.toggleVolumeKf(audio.uid)} title={t("Point de volume à la tête de lecture")}><Diamond size={13} /></button>
      </header>
      <Slider label={t("Volume")} value={audio.kf.length ? kfValue(audio.kf, "v", aLocal, audio.volume) : audio.volume} min={0} max={2} step={0.02} def={audio.kind === "music" ? 0.5 : 1} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.setVolumeAt(audio.uid, v, false)} />
      <p class="note">{audio.kf.length ? t("{n} points de volume : le volume varie en douceur.", { n: audio.kf.length }) : t("Pour faire monter ou baisser le son à un moment, pose un point ◆ puis change le volume plus loin.")}</p>
      {#if audio.kf.length}<button class="btn small ghost" onclick={() => studio.updateAudio(audio.uid, { kf: [] })}><Trash2 size={14} />{t("Retirer les points de volume")}</button>{/if}
    </section>
    <section>
      <header><SunMoon size={14} /><h4>{t("Fondus")}</h4></header>
      <Slider label={t("Entrée")} value={audio.fadeIn} min={0} max={5} step={0.1} def={0} format={sec} onstart={begin} oninput={(v) => studio.updateAudio(audio.uid, { fadeIn: v }, false)} />
      <Slider label={t("Sortie")} value={audio.fadeOut} min={0} max={5} step={0.1} def={0} format={sec} onstart={begin} oninput={(v) => studio.updateAudio(audio.uid, { fadeOut: v }, false)} />
      <label class="row"><span>{t("Baisser quand le jeu est fort")}</span><span class="switch"><input type="checkbox" checked={audio.duck} onchange={(e) => studio.updateAudio(audio.uid, { duck: e.currentTarget.checked })} /><span></span></span></label>
    </section>
    {#if audio.srcDur - audio.dur > 0.2}
      <section>
        <header><Clock3 size={14} /><h4>{t("Calage")}</h4></header>
        <Slider label={t("Début")} value={audio.srcStart} min={0} max={Math.max(0.1, audio.srcDur - audio.dur)} step={0.05} def={0} format={(v) => fmtTime(v, true)} onstart={begin} oninput={(v) => studio.updateAudio(audio.uid, { srcStart: v }, false)} />
      </section>
    {/if}
    <div class="acts"><button class="btn small danger" onclick={() => studio.removeSel()}><Trash2 size={14} />{t("Retirer")}</button></div>

  {:else if studio.sel?.type === "keys"}
    <div class="title"><Keyboard size={15} /><h3>{t("Touches clavier / souris")}</h3></div>
    <p class="note">{t("Déplace-les sur l'aperçu. Elles sont incrustées telles quelles dans la vidéo exportée.")}</p>
    <Slider label={t("Taille")} value={P.keys.scale} min={0.08} max={0.9} step={0.01} format={pct} parse={parsePct} onstart={begin} oninput={(v) => studio.setKeys({ scale: v }, false)} />
    <p class="note">{t("Nombre de touches et QWERTY / AZERTY : Réglages › Superposition.")}</p>
    <button class="btn small ghost" onclick={() => studio.removeSel()}><Trash2 size={14} />{t("Masquer les touches")}</button>

  {:else}
    <div class="title"><LayoutTemplate size={15} /><h3>{t("Projet")}</h3></div>
    <section>
      <header><Monitor size={14} /><h4>{t("Format")}</h4></header>
      <div class="formats">
        {#each FORMATS as fm}
          <button class:on={P.format === fm.id} onclick={() => studio.setFormat(fm.id)}><fm.icon size={17} /><b>{fm.id}</b><small>{fm.hint}</small></button>
        {/each}
      </div>
      {#if P.format !== "16:9"}
        <span class="mini">{t("Les clips 16:9 dans ce format")}</span>
        <div class="seg3">
          {#each [["blur", "Fond flou"], ["crop", "Recadré"], ["fit", "Bandes noires"]] as [f, label]}
            <button class:on={P.fit === f} onclick={() => studio.edit((p) => (p.fit = f as Fit))}>{t(label)}</button>
          {/each}
        </div>
      {/if}
    </section>
    <section>
      <label class="row"><span class="ic"><SunMoon size={15} />{t("Fondu au noir à la fin")}</span>
        <span class="switch"><input type="checkbox" checked={(P.endFade ?? 0) > 0} onchange={(e) => studio.edit((p) => (p.endFade = e.currentTarget.checked ? 1 : 0))} /><span></span></span>
      </label>
      {#if (P.endFade ?? 0) > 0}
        <Slider label={t("Durée")} value={P.endFade ?? 1} min={0.3} max={4} step={0.1} def={1} format={sec} onstart={begin} oninput={(v) => studio.edit((p) => (p.endFade = v), false)} />
      {/if}
      <p class="note">{t("L'image et le son s'éteignent doucement sur les dernières secondes.")}</p>
    </section>
    <section>
      <label class="row"><span class="ic"><Keyboard size={15} />{t("Touches clavier / souris")}</span>
        <span class="switch"><input type="checkbox" checked={P.keys.on} onchange={(e) => studio.setKeys({ on: e.currentTarget.checked })} /><span></span></span>
      </label>
      <p class="note">{t("Affichées seulement pour les clips enregistrés avec l'option « touches à l'écran ».")}</p>
    </section>
    <section class="help">
      <header><Keyboard size={14} /><h4>{t("Raccourcis")}</h4></header>
      <p class="note mono">{t("Espace lecture · S couper · Suppr supprimer · Ctrl+Z annuler · Ctrl+D dupliquer · ← → image par image · Ctrl+molette zoom")}</p>
    </section>
    {#if P.pieces.length || P.tracks.length}
      <div class="acts"><button class="btn small ghost danger" onclick={() => studio.clear()}><Trash2 size={14} />{t("Vider le montage")}</button></div>
    {/if}
  {/if}
</aside>

<style>
  .insp { display: flex; flex-direction: column; gap: 4px; padding: 14px 14px 18px; border-left: 1px solid var(--line); overflow-y: auto; min-height: 0; background: var(--bg-2); }
  /* Dans une colonne qui défile, rien ne doit rétrécir (le titre, coupé, tombait à 0 px de haut) */
  .insp > :global(*) { flex-shrink: 0; }
  .title { display: flex; align-items: center; gap: 8px; color: var(--text-3); padding-bottom: 8px; }
  h3 { margin: 0; font-size: 14px; font-weight: 600; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  section { display: flex; flex-direction: column; gap: 6px; padding: 12px 0 10px; border-top: 1px solid var(--line); }
  section > header { display: flex; align-items: center; gap: 7px; color: var(--text-3); min-height: 24px; }
  h4 { flex: 1; margin: 0; font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-3); }
  .hbtn { width: 26px; height: 24px; border-radius: 7px; display: grid; place-items: center; color: var(--text-3); border: 1px solid var(--line); transition: color var(--dur) var(--ease), background var(--dur) var(--ease); }
  .hbtn:hover { color: var(--text); }
  .hbtn.on { color: var(--hi-ink); background: var(--hi); border-color: var(--hi); }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; font-size: 12.5px; color: var(--text-2); }
  .sub { display: grid; grid-template-columns: 24px minmax(0, 1fr); align-items: center; gap: 8px; padding-left: 4px; }
  .ic { display: inline-flex; align-items: center; gap: 7px; }
  .note { margin: 0; font-size: 11.5px; line-height: 1.5; color: var(--text-3); }
  .mini { font-size: 11px; color: var(--text-3); margin-top: 2px; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip { height: 26px; padding: 0 9px; border-radius: 7px; font-size: 12px; color: var(--text-2); background: var(--panel); border: 1px solid var(--line); transition: color var(--dur) var(--ease), border-color var(--dur) var(--ease); }
  .chip:hover { color: var(--text); border-color: var(--line-2); }
  .chip.on { color: var(--hi-ink); background: var(--hi); border-color: var(--hi); }
  .grid3 { display: grid; grid-template-columns: repeat(3, 1fr); gap: 4px; margin-bottom: 8px; }
  .area { height: auto; min-height: 56px; padding: 8px 10px; resize: vertical; font-size: 13px; user-select: text; margin-bottom: 8px; }
  .fonts { display: grid; grid-template-columns: 1fr 1fr; gap: 4px; }
  .font { height: 34px; padding: 0 8px; border-radius: 8px; font-size: 15px; color: var(--text-2); background: var(--panel); border: 1px solid var(--line); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; transition: color var(--dur) var(--ease), border-color var(--dur) var(--ease); }
  .font:hover { color: var(--text); }
  .font.on { color: var(--text); border-color: #fff; }
  .swatches { display: flex; align-items: center; gap: 4px; justify-content: flex-end; }
  .sw { position: relative; width: 20px; height: 20px; flex: none; border-radius: 50%; box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.22); transition: transform 140ms var(--spring); }
  .sw:hover { transform: scale(1.12); }
  .sw.on { box-shadow: 0 0 0 2px var(--bg-2), 0 0 0 3.5px #fff; }
  .sw.custom { overflow: hidden; background: conic-gradient(#ff5d5d, #ffd25a, #3ddc97, #38bdf8, #b07cff, #ff5d5d); cursor: pointer; }
  .sw.custom input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .seg3 { display: flex; padding: 3px; gap: 2px; border-radius: 9px; background: var(--bg); border: 1px solid var(--line); }
  .seg3 button { flex: 1; height: 26px; border-radius: 6px; display: grid; place-items: center; font-size: 12px; color: var(--text-2); }
  .seg3 button.on { background: var(--panel-3); color: var(--text); }
  .formats { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .formats button { display: flex; flex-direction: column; align-items: center; gap: 3px; padding: 10px 4px; border-radius: 10px; color: var(--text-2); background: var(--panel); border: 1px solid var(--line); }
  .formats button b { font-size: 13px; color: var(--text); }
  .formats button small { font-size: 10.5px; color: var(--text-3); }
  .formats button.on { border-color: #fff; color: var(--text); }
  .help .note { font-size: 11px; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .acts { display: flex; flex-wrap: wrap; gap: 6px; padding-top: 12px; border-top: 1px solid var(--line); }
  /* Transitions : petite image animée au survol (reprise de l'ancienne frise) */
  .jt { display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 8px 4px; border-radius: 10px; font-size: 11.5px; color: var(--text-2); border: 1px solid var(--line); transition: border-color var(--dur), color var(--dur); }
  .jt:hover { border-color: var(--line-2); color: var(--text); }
  .jt.on { border-color: #fff; color: var(--text); }
  .jprev { position: relative; width: 44px; height: 26px; border-radius: 5px; overflow: hidden; background: #3a5a8c; }
  .jprev i { position: absolute; inset: 0; }
  .jprev i:first-child { background: linear-gradient(135deg, #f0a23b, #b8612a); }
  .jprev i:last-child { background: linear-gradient(135deg, #5a9bff, #2b4f8f); opacity: 0; }
  .jt:hover .jprev.fade i:last-child { animation: jfade 1.2s ease-in-out infinite; }
  .jt:hover .jprev.black i:last-child { animation: jblack 1.2s ease-in-out infinite; }
  .jt:hover .jprev.white i:last-child { animation: jwhite 1.2s ease-in-out infinite; }
  .jt:hover .jprev.zoom i:first-child { animation: jzoom 1.2s ease-in infinite; }
  .jt:hover .jprev.zoom i:last-child { animation: jfade 1.2s ease-in-out infinite; }
  .jt:hover .jprev.slide i:last-child { opacity: 1; animation: jslide 1.2s ease-in-out infinite; }
  .jt:hover .jprev.wipe i:last-child { opacity: 1; animation: jwipe 1.2s ease-in-out infinite; }
  @keyframes jfade { 0%, 20% { opacity: 0; } 70%, 100% { opacity: 1; } }
  @keyframes jblack { 0%, 15% { opacity: 0; background: #000; } 45% { opacity: 1; background: #000; } 50% { background: linear-gradient(135deg, #5a9bff, #2b4f8f); opacity: 0; } 100% { opacity: 1; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } }
  @keyframes jwhite { 0%, 15% { opacity: 0; background: #fff; } 45% { opacity: 1; background: #fff; } 50% { background: linear-gradient(135deg, #5a9bff, #2b4f8f); opacity: 1; } 100% { opacity: 1; background: linear-gradient(135deg, #5a9bff, #2b4f8f); } }
  @keyframes jzoom { 0% { transform: scale(1); } 100% { transform: scale(2.2); } }
  @keyframes jslide { 0% { transform: translateX(100%); } 70%, 100% { transform: translateX(0); } }
  @keyframes jwipe { 0% { clip-path: inset(0 0 0 100%); } 70%, 100% { clip-path: inset(0 0 0 0); } }
  @media (prefers-reduced-motion: reduce) { .jprev i { animation: none !important; } }
  .marks { display: flex; flex-direction: column; gap: 2px; }
  .mark { display: grid; grid-template-columns: 20px 1fr auto; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 7px; font-size: 12px; text-align: left; color: var(--text-2); }
  .mark:hover:not(:disabled) { background: var(--panel); color: var(--text); }
  .mark:disabled { opacity: 0.4; }
  .mark i { display: grid; place-items: center; color: var(--text); }
  .mark i.gold { color: #ffd25a; }
  .mark i.green { color: #3ddc97; }
  .mark i.dim { color: var(--text-3); }
  .mark small { font-size: 11px; color: var(--text-3); }
  .fxcolor { display: flex; align-items: center; justify-content: space-between; height: 30px; font-size: 12.5px; color: var(--text-2); }
  .fxcolor input { width: 34px; height: 22px; padding: 0; border: 1px solid var(--line-2); border-radius: 6px; background: none; }
  .fontpick { display: flex; align-items: center; gap: 12px; width: 100%; padding: 10px 12px; border-radius: 10px; background: #0d0e12; border: 1px solid var(--line); text-align: left; transition: border-color var(--dur) var(--ease); }
  .fontpick:hover { border-color: var(--line-2); }
  .fp-sample { flex: 1; min-width: 0; font-size: 26px; line-height: 1.2; color: #fff; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fp-meta { display: flex; flex-direction: column; align-items: flex-end; gap: 2px; flex: none; }
  .fp-meta b { font-size: 12.5px; font-weight: 600; color: var(--text); }
  .fp-meta small { font-size: 11px; color: var(--text-3); }
</style>
