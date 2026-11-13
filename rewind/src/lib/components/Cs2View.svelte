<script lang="ts">
  import { t } from "$lib/i18n.svelte";
  import { onMount } from "svelte";
  import { CheckCircle2, AlertTriangle, Wrench, Radio, RotateCw, ChevronDown, Shield, Skull } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { api, type Cs2Info, type Cs2Rules, type Settings } from "$lib/api";

  let info = $state<Cs2Info | null>(null);
  onMount(() => {
    const load = async () => { try { info = await api.cs2Info(); } catch {} };
    load();
    const t = setInterval(load, 2000);
    return () => clearInterval(t);
  });
  const modeNames: Record<string, string> = {
    competitive: "compétitif", casual: "occasionnel", deathmatch: "deathmatch", scrimcomp2v2: "wingman",
    gungameprogressive: "course aux armements", retakes: "retakes", survival: "danger zone",
  };
  const phaseNames: Record<string, string> = { warmup: "échauffement", live: "en jeu", intermission: "mi-temps", gameover: "fin de partie" };
  const connected = $derived(!!info && info.since_last >= 0 && info.since_last < 45);

  const s = $derived(app.settings);
  const modes: [string, string][] = [
    ["competitive", "Compétitif / Premier / FACEIT"],
    ["scrimcomp2v2", "Wingman"],
    ["casual", "Occasionnel"],
    ["deathmatch", "Deathmatch"],
    ["gungameprogressive", "Course aux armements"],
    ["retakes", "Retakes"],
    ["survival", "Danger Zone"],
    ["other", "Autres / serveurs perso"],
  ];


  // Réglages par événement : chaque case cochée peut déclencher LE clip du round (jamais deux)
  const EVENTS: { id: string; label: string; desc: string; beta?: boolean }[] = [
    { id: "onetap", label: "One tap", desc: "Un headshot d'une seule balle." },
    { id: "headshot", label: "Headshot", desc: "N'importe quel kill à la tête (beaucoup de clips)." },
    { id: "knife", label: "Couteau", desc: "Kill au couteau." },
    { id: "zeus", label: "Zeus", desc: "Kill au Zeus." },
    { id: "collateral", label: "Collateral", desc: "Deux kills avec une seule balle." },
    { id: "2k", label: "2K", desc: "2 kills dans le round." },
    { id: "3k", label: "3K", desc: "3 kills dans le round." },
    { id: "4k", label: "4K", desc: "4 kills dans le round." },
    { id: "ace", label: "ACE", desc: "Toute l'équipe adverse." },
    { id: "streak", label: "Série", desc: "{n} kills avec moins de {s} s entre chacun." },
    { id: "clutch", label: "Clutch estimé", desc: "Round gagné en restant en vie avec 2 kills ou plus. CS2 ne dit pas si tes coéquipiers sont morts : peut se tromper.", beta: true },
    { id: "death", label: "Mort", desc: "Quand tu meurs (pour revoir l'action)." },
    { id: "assist", label: "Assist", desc: "Quand tu fais une assistance." },
  ];

  // Deux profils : « legit » (champs de cs2) et « HvH » (cs2.hvh). Le moteur utilise celui choisi.
  type Profile = "legit" | "hvh";
  type PresetId = "normal" | "hvh" | "all";
  type Preset = { id: PresetId; label: string; hint: string };
  const PROFILES: { id: Profile; label: string; hint: string; presets: Preset[] }[] = [
    {
      id: "legit", label: "Jeu legit", hint: "Compétitif, Premier, FACEIT… : one tap, headshots, nombre de kills, etc.",
      presets: [
        { id: "normal", label: "Conseillé", hint: "3 kills, one tap, couteau, zeus, collateral, 3K, 4K, ACE" },
        { id: "all", label: "Tout clipper", hint: "Chaque round avec au moins un kill" },
      ],
    },
    {
      id: "hvh", label: "HvH", hint: "Presque tout est en one tap : on garde surtout les rounds et les séries à plusieurs kills.",
      presets: [
        { id: "hvh", label: "Conseillé", hint: "Seulement 3 kills ou plus (en HvH presque tout est en one tap)" },
        { id: "all", label: "Tout clipper", hint: "Chaque round avec au moins un kill" },
      ],
    },
  ];
  const profile = $derived<Profile>(s?.cs2.profile === "hvh" ? "hvh" : "legit");
  // Menus déroulants : celui du profil utilisé est ouvert au départ
  let open = $state<Record<Profile, boolean>>({ legit: false, hvh: false });
  let openedOnce = false;
  $effect(() => {
    if (s && !openedOnce) {
      openedOnce = true;
      open[s.cs2.profile === "hvh" ? "hvh" : "legit"] = true;
    }
  });
  const rulesOf = (x: Settings, p: Profile): Cs2Rules => (p === "hvh" ? x.cs2.hvh : x.cs2);
  function edit(p: Profile, f: (r: Cs2Rules) => void) {
    app.patch((x) => f(rulesOf(x, p)));
  }
  function setProfile(p: Profile) {
    if (profile === p) return;
    app.patch((x) => (x.cs2.profile = p));
    open[p] = true;
    app.toast("ok", t("Profil {p} utilisé", { p: t(PROFILES.find((x) => x.id === p)!.label) }), t("Les clips automatiques suivent maintenant ces règles."));
  }

  function presetValues(id: PresetId) {
    const ev: Record<string, boolean> = { kill: true, headshot: false, onetap: true, knife: true, zeus: true, collateral: true, "2k": false, "3k": true, "4k": true, ace: true, streak: true, clutch: false, death: false, assist: false };
    if (id === "hvh") Object.assign(ev, { onetap: false, knife: false, zeus: false, collateral: false });
    if (id === "all") Object.assign(ev, { headshot: true, "2k": true });
    return { ev, kills: id === "all" ? 1 : 3, seq: id === "all" ? 1 : 3, special: id === "all" };
  }
  function applyPreset(p: Profile, id: PresetId, label: string) {
    const v = presetValues(id);
    edit(p, (r) => {
      for (const [k, on] of Object.entries(v.ev)) if (r.events[k]) r.events[k].enabled = on;
      r.min_round_kills = v.kills;
      r.min_sequence_kills = v.seq;
      r.special_always = v.special;
    });
    app.toast("ok", t("Préréglage appliqué"), `${t(PROFILES.find((x) => x.id === p)!.label)} · ${t(label)}`);
  }
  function activePreset(r: Cs2Rules, presets: Preset[]): PresetId | null {
    for (const pr of presets) {
      const v = presetValues(pr.id);
      const same = Object.entries(v.ev).every(([k, on]) => (r.events[k]?.enabled ?? false) === on);
      if (same && r.min_round_kills === v.kills && r.min_sequence_kills === v.seq && r.special_always === v.special) return pr.id;
    }
    return null;
  }
  function setEvent(p: Profile, id: string, patch: Partial<{ enabled: boolean; before: number; after: number }>) {
    edit(p, (r) => {
      if (r.events[id]) Object.assign(r.events[id], patch);
    });
  }
  /** Nombre d'événements cochés (affiché sur le menu fermé) */
  const countOn = (r: Cs2Rules) => EVENTS.filter((e) => r.events[e.id]?.enabled).length;

  async function install() {
    const files = await api.cs2Install();
    info = await api.cs2Info();
    app.toast(files.length ? "ok" : "err", t(files.length ? "CS2 configuré" : "CS2 introuvable"), t(files.length ? "Relance CS2 si le jeu est déjà ouvert." : "Aucune installation de Counter-Strike 2 trouvée."));
  }

  const ago = (t: number) => {
    const s = Math.floor((Date.now() - t) / 1000);
    return s < 60 ? `${s} s` : `${Math.floor(s / 60)} min`;
  };
</script>

{#snippet rules(p: Profile, r: Cs2Rules, presets: Preset[])}
  {@const cur = activePreset(r, presets)}
  <div class="presets">
    <span class="faint">{t("Préréglages :")}</span>
    {#each presets as pr}
      <button class="preset" class:on={cur === pr.id} onclick={() => applyPreset(p, pr.id, pr.label)} title={t(pr.hint)}>{t(pr.label)}</button>
    {/each}
  </div>
  <div class="row">
    <div class="evname">
      <span class="switch"><input type="checkbox" checked={r.events.kill?.enabled ?? true} onchange={(e) => setEvent(p, "kill", { enabled: e.currentTarget.checked })} /><span></span></span>
      <div>
        <strong>{t("Nombre de kills dans le round")}</strong>
        <span class="muted">{t(r.min_round_kills > 1 ? "À la fin du round, un seul clip si tu as fait au moins {n} kills dans ce round." : "À la fin du round, un seul clip si tu as fait au moins {n} kill dans ce round.", { n: r.min_round_kills })}</span>
      </div>
    </div>
    <div class="steps">
      {#each [1, 2, 3, 4, 5] as n}<button class:on={r.min_round_kills === n} disabled={!(r.events.kill?.enabled ?? true)} onclick={() => edit(p, (x) => (x.min_round_kills = n))}>{n === 5 ? "ACE" : `${n}+`}</button>{/each}
    </div>
  </div>
  <div class="evhead faint"><span>{t("Aussi un clip pour…")}</span><span>{t("avant")}</span><span>{t("après")}</span></div>
  {#each EVENTS as ev}
    {@const er = r.events[ev.id]}
    {#if er}
      <div class="evrow" class:off={!er.enabled}>
        <label class="evname">
          <span class="switch"><input type="checkbox" checked={er.enabled} onchange={(e) => setEvent(p, ev.id, { enabled: e.currentTarget.checked })} /><span></span></span>
          <span class="evtext">
            <strong>{t(ev.label)}{#if ev.beta} <span class="tag neutral">{t("expérimental")}</span>{/if}</strong>
            <span class="muted">{t(ev.desc, { n: r.streak_count, s: r.streak_window })}</span>
          </span>
        </label>
        <label class="num small"><input class="field" type="number" min="0" max="30" value={er.before} onchange={(e) => setEvent(p, ev.id, { before: Math.max(0, +e.currentTarget.value) })} /><span class="faint">s</span></label>
        <label class="num small"><input class="field" type="number" min="0" max="30" value={er.after} onchange={(e) => setEvent(p, ev.id, { after: Math.max(0, +e.currentTarget.value) })} /><span class="faint">s</span></label>
      </div>
    {/if}
  {/each}
  <div class="row">
    <div><strong>{t("Marges autour des kills")}</strong><span class="muted">{t("{a} s avant le premier kill, {b} s après le dernier.", { a: r.events.kill?.before ?? 5, b: r.events.kill?.after ?? 3 })}</span></div>
    <div class="inline">
      <label class="num"><input class="field" type="number" min="0" max="30" value={r.events.kill?.before ?? 5} onchange={(e) => setEvent(p, "kill", { before: Math.max(0, +e.currentTarget.value) })} /><span class="faint">{t("avant")}</span></label>
      <label class="num"><input class="field" type="number" min="0" max="30" value={r.events.kill?.after ?? 3} onchange={(e) => setEvent(p, "kill", { after: Math.max(0, +e.currentTarget.value) })} /><span class="faint">{t("après")}</span></label>
    </div>
  </div>
  <div class="row">
    <div>
      <strong>{t("Sans rounds (deathmatch HvH, échauffement)")}</strong>
      <span class="muted">{t("Un clip quand tu enchaînes au moins {n} kills avec moins de {g} s entre chacun. {m} s maximum par clip.", { n: r.min_sequence_kills, g: r.merge_gap, m: r.max_sequence })}</span>
    </div>
    <div class="inline">
      <label class="num"><input class="field" type="number" min="1" max="20" value={r.min_sequence_kills} onchange={(e) => edit(p, (x) => (x.min_sequence_kills = Math.max(1, +e.currentTarget.value)))} /><span class="faint">{t("kills")}</span></label>
      <label class="num"><input class="field" type="number" min="2" max="60" value={r.merge_gap} onchange={(e) => edit(p, (x) => (x.merge_gap = Math.max(2, +e.currentTarget.value)))} /><span class="faint">s</span></label>
      <label class="num"><input class="field" type="number" min="10" max="180" step="5" value={r.max_sequence} onchange={(e) => edit(p, (x) => (x.max_sequence = Math.max(10, +e.currentTarget.value)))} /><span class="faint">max</span></label>
    </div>
  </div>
  <div class="row">
    <div><strong>{t("Événements cochés aussi sans rounds")}</strong><span class="muted">{t("En deathmatch, un one tap, un couteau… font aussi un clip. Coupé conseillé en HvH (sinon presque chaque kill fait un clip).")}</span></div>
    <span class="switch"><input type="checkbox" checked={r.special_always} onchange={(e) => edit(p, (x) => (x.special_always = e.currentTarget.checked))} /><span></span></span>
  </div>
  <div class="row">
    <div><strong>{t("One tap au sniper")}</strong><span class="muted">{t("Compter aussi l'AWP et le Scout comme one tap (pour le titre du clip).")}</span></div>
    <span class="switch"><input type="checkbox" checked={!r.onetap_ignore_snipers} onchange={(e) => edit(p, (x) => (x.onetap_ignore_snipers = !e.currentTarget.checked))} /><span></span></span>
  </div>
{/snippet}

{#if s}
  <section class="cs appear">
    <header>
      <div>
        <h1>{t("Counter-Strike 2")}</h1>
        <p class="muted">{t("Rewind lit l'API officielle de Valve (Game State Integration). Aucune injection, aucune lecture de mémoire : aucun risque pour ton compte.")}</p>
      </div>
      <label class="master">
        <span>{t("Clips automatiques")}</span>
        <span class="switch"><input type="checkbox" checked={s.cs2.enabled} onchange={(e) => app.patch((x) => (x.cs2.enabled = e.currentTarget.checked))} /><span></span></span>
      </label>
    </header>

    <div class="cols">
      <div class="main">
        <div class="card status" class:good={connected} class:bad={info?.needs_restart}>
          {#if info?.needs_restart}
            <RotateCw size={20} class="warn" />
            <div>
              <strong>{t("Redémarre Counter-Strike 2")}</strong>
              <span class="muted">{t("CS2 était déjà ouvert quand Rewind s'est relié à lui. Le jeu ne lit ce lien qu'au lancement : ferme-le et relance-le une fois.")}</span>
            </div>
          {:else if connected && info}
            <CheckCircle2 size={20} class="ok" />
            <div>
              <strong>{t("Connecté à CS2")}</strong>
              <span class="muted">
                {info.live.map || t("menu")}{info.live.mode ? ` · ${t(modeNames[info.live.mode] ?? info.live.mode)}` : ""}{info.live.map_phase ? ` · ${t(phaseNames[info.live.map_phase] ?? info.live.map_phase)}` : ""}
                {info.live.spectating ? ` · ${t("tu regardes un autre joueur (pas de clip)")}` : ""}
                {!s.cs2.modes[info.live.mode] && info.live.mode ? ` · ${t("ce mode est désactivé ci-dessous")}` : ""}
              </span>
            </div>
          {:else if info?.running}
            <AlertTriangle size={20} class="warn" />
            <div><strong>{t("CS2 est lancé mais ne parle pas encore à Rewind")}</strong><span class="muted">{t("Si ça dure, relance CS2 une fois (il lit le lien seulement au démarrage).")}</span></div>
          {:else if info && info.configured.length}
            <CheckCircle2 size={20} class="ok" />
            <div><strong>{t("Prêt")}</strong><span class="muted">{t(info.configured.length > 1 ? "Relié à {n} installations de CS2. Lance une partie : les actions s'afficheront à droite." : "Relié à {n} installation de CS2. Lance une partie : les actions s'afficheront à droite.", { n: info.configured.length })}</span></div>
          {:else if info && info.installs.length}
            <AlertTriangle size={20} class="warn" />
            <div><strong>{t("À configurer")}</strong><span class="muted">{t("CS2 est installé mais pas encore relié à Rewind.")}</span></div>
          {:else}
            <AlertTriangle size={20} class="warn" />
            <div><strong>{t("CS2 introuvable")}</strong><span class="muted">{t("Aucune installation détectée dans tes bibliothèques Steam.")}</span></div>
          {/if}
          <button class="btn small" onclick={install}><Wrench size={14} />{t("Réinstaller le lien")}</button>
        </div>

        <div class="card">
          <div class="ghead"><strong>{t("Quand faire un clip")}</strong><span class="faint">{t("Un seul clip par round, même s'il y a plusieurs actions : jamais deux clips pour le même round.")}</span></div>
          <div class="profile">
            <span class="faint">{t("Je joue en :")}</span>
            <div class="seg" style="--i:{profile === 'hvh' ? 1 : 0}">
              <i class="thumb"></i>
              {#each PROFILES as p}
                <button class:on={profile === p.id} onclick={() => setProfile(p.id)}>{#if p.id === "hvh"}<Skull size={14} />{:else}<Shield size={14} />{/if}{t(p.id === "hvh" ? "HvH" : "Legit")}</button>
              {/each}
            </div>
          </div>
          {#each PROFILES as p (p.id)}
            {@const r = rulesOf(s, p.id)}
            <div class="acc" class:open={open[p.id]} class:used={profile === p.id}>
              <button class="acchead" onclick={() => (open[p.id] = !open[p.id])} aria-expanded={open[p.id]}>
                <span class="accicon">{#if p.id === "hvh"}<Skull size={16} />{:else}<Shield size={16} />{/if}</span>
                <span class="acctext">
                  <strong>{t(p.label)}{#if profile === p.id}<span class="tag">{t("utilisé")}</span>{/if}</strong>
                  <span class="muted">{t(p.hint)}</span>
                </span>
                <span class="faint accsum">{t(r.events.kill?.enabled ? "{k}+ kills · {n} événements" : "{n} événements", { k: r.min_round_kills, n: countOn(r) })}</span>
                <span class="chev"><ChevronDown size={18} /></span>
              </button>
              <div class="accbody">
                <div class="accinner">
                  {@render rules(p.id, r, p.presets)}
                </div>
              </div>
            </div>
          {/each}
        </div>

        <div class="card">
          <div class="ghead"><strong>{t("Modes de jeu")}</strong><span class="faint">{t("Les clips automatiques ne se font que dans les modes cochés.")}</span></div>
          <div class="modes">
            {#each modes as [key, label]}
              <label class="mode"><input type="checkbox" checked={s.cs2.modes[key] ?? false} onchange={(e) => app.patch((x) => (x.cs2.modes[key] = e.currentTarget.checked))} />{t(label)}</label>
            {/each}
          </div>
        </div>
      </div>

      <div class="feed card">
        <div class="ghead"><strong><Radio size={15} /> {t("En direct")}</strong><span class="faint">{t("Ce que Rewind voit pendant ta partie.")}</span></div>
        {#if app.cs2Feed.length === 0}
          <p class="faint empty">{t("Rien pour l'instant. Les kills, headshots et clips s'afficheront ici.")}</p>
        {:else}
          <ul>
            {#each app.cs2Feed as f (f.at + f.text)}
              <li class="k-{f.kind} appear"><span>{f.text}</span><span class="faint mono">{ago(f.at)}</span></li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  </section>
{/if}

<style>
  /* Profil utilisé : deux positions, le curseur glisse */
  .profile { display: flex; align-items: center; gap: 10px; margin: 2px 0 12px; font-size: 12.5px; }
  .seg { position: relative; display: grid; grid-template-columns: 1fr 1fr; padding: 3px; border-radius: 11px; background: var(--bg-2); border: 1px solid var(--line); }
  .seg button { position: relative; z-index: 1; display: flex; align-items: center; justify-content: center; gap: 6px; height: 30px; padding: 0 16px; border-radius: 8px; font-weight: 650; font-size: 13px; color: var(--text-2); transition: color var(--dur) var(--ease); }
  .seg button.on { color: var(--hi-ink); }
  .seg .thumb { position: absolute; top: 3px; bottom: 3px; left: 3px; width: calc(50% - 3px); border-radius: 8px; background: var(--hi); box-shadow: 0 1px 3px rgba(0, 0, 0, 0.5); transform: translateX(calc(var(--i) * 100%)); transition: transform 380ms var(--ease-out); }
  /* Menus déroulants */
  .acc { border: 1px solid var(--line); border-radius: 12px; transition: border-color var(--dur) var(--ease); }
  .acc + .acc { margin-top: 8px; }
  .acc.used { border-color: var(--line-2); }
  .acchead { width: 100%; display: flex; align-items: center; gap: 12px; padding: 12px 14px; text-align: left; border-radius: 14px; transition: background var(--dur) var(--ease); }
  .acchead:hover { background: color-mix(in srgb, var(--panel-2) 60%, transparent); }
  .accicon { width: 30px; height: 30px; flex: none; display: grid; place-items: center; border-radius: 8px; background: var(--panel-2); border: 1px solid var(--line-2); color: var(--text-2); transition: color var(--dur), background var(--dur); }
  .acc.used .accicon { color: var(--text); background: var(--panel-3); border-color: var(--line-2); }
  .acctext { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; font-size: 13px; }
  .acctext strong { display: flex; align-items: center; gap: 8px; font-size: 14px; }
  .acctext .tag { animation: pop 320ms var(--ease-out) both; }
  .accsum { font-size: 12px; white-space: nowrap; transition: opacity var(--dur); }
  .acc.open .accsum { opacity: 0; }
  .chev { display: grid; color: var(--text-3); transition: transform 320ms var(--ease-out); }
  .acc.open .chev { transform: rotate(180deg); }
  .accbody { display: grid; grid-template-rows: 0fr; transition: grid-template-rows 360ms var(--ease-out); }
  .acc.open .accbody { grid-template-rows: 1fr; }
  .accinner { min-height: 0; overflow: hidden; padding: 0 16px; opacity: 0; transform: translateY(-4px); transition: opacity 260ms var(--ease), transform 360ms var(--ease-out), padding 360ms var(--ease-out); }
  .acc.open .accinner { opacity: 1; transform: none; padding: 0 16px 10px; }
  @keyframes pop { from { opacity: 0; transform: scale(0.7); } to { opacity: 1; transform: none; } }
  /* Les cartes arrivent l'une après l'autre */

  .presets { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; margin: 4px 0 6px; font-size: 12.5px; }
  .preset { height: 28px; padding: 0 12px; border-radius: 99px; border: 1px solid var(--line); color: var(--text-2); font-size: 12.5px; font-weight: 600; }
  .preset:hover { color: var(--text); border-color: var(--line-2); }
  .preset.on { background: var(--hi); color: var(--hi-ink); border-color: transparent; }
  .evname { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .evname > div, .evtext { display: flex; flex-direction: column; gap: 2px; min-width: 0; font-size: 13px; }
  .evhead { display: grid; grid-template-columns: 1fr 64px 64px; gap: 8px; padding: 12px 0 4px; font-size: 11px; text-transform: uppercase; letter-spacing: .05em; }
  .evhead span:not(:first-child) { text-align: center; }
  .evrow { display: grid; grid-template-columns: 1fr 64px 64px; gap: 8px; align-items: center; padding: 8px 0; border-top: 1px solid var(--line); }
  .evrow.off .evtext { opacity: .55; }
  .num.small .field { width: 46px; }
  .cs { height: 100%; overflow-y: auto; padding: 22px 28px 40px; }
  header { display: flex; justify-content: space-between; align-items: flex-start; gap: 30px; margin-bottom: 20px; }
  h1 { margin: 0 0 4px; font-weight: 600; font-size: 22px; letter-spacing: -0.025em; }
  header p { margin: 0; max-width: 620px; font-size: 13.5px; }
  .master { display: flex; align-items: center; gap: 12px; padding: 10px 14px; border-radius: 12px; background: var(--panel); border: 1px solid var(--line); font-weight: 600; white-space: nowrap; }
  .cols { display: grid; grid-template-columns: minmax(0, 1fr) 320px; gap: 18px; align-items: start; }
  .main { display: flex; flex-direction: column; gap: 14px; }
  .card { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius-l); padding: 16px 18px; box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025); }
  .status { display: flex; align-items: center; gap: 14px; }
  .status div { flex: 1; display: flex; flex-direction: column; gap: 2px; font-size: 13px; }
  .status :global(.ok) { color: var(--accent); }
  .status :global(.warn) { color: #ffb020; }
  .status.good { border-color: color-mix(in srgb, var(--accent) 35%, var(--line)); }
  .status.bad { border-color: color-mix(in srgb, #ffb020 50%, var(--line)); background: color-mix(in srgb, #ffb020 6%, var(--panel)); }
  .inline { display: flex; gap: 10px; flex: none; }
  .steps { display: flex; gap: 4px; flex: none; }
  .steps button { height: 32px; min-width: 44px; padding: 0 10px; border-radius: 9px; border: 1px solid var(--line); font-weight: 650; font-size: 13px; color: var(--text-2); }
  .steps button.on { background: var(--hi); color: var(--hi-ink); border-color: transparent; }
  .row { display: flex; justify-content: space-between; align-items: center; gap: 20px; padding: 10px 0; }
  .row + .row { border-top: 1px solid var(--line); }
  .row > div:first-child { display: flex; flex-direction: column; gap: 2px; font-size: 13px; }
  .row > div.evname:first-child { flex-direction: row; align-items: center; gap: 12px; }
  .ghead { display: flex; flex-direction: column; gap: 2px; margin-bottom: 10px; font-size: 13px; }
  .ghead strong { display: flex; align-items: center; gap: 6px; font-size: 14px; }
  .num { display: flex; align-items: center; gap: 6px; }
  .num .field { width: 56px; height: 32px; text-align: center; padding: 0 6px; }
  .modes { display: grid; grid-template-columns: 1fr 1fr; gap: 8px 16px; }
  .mode { display: flex; align-items: center; gap: 9px; font-size: 13px; color: var(--text-2); cursor: pointer; }
  .mode input { accent-color: var(--hi); width: 15px; height: 15px; }
  .feed { position: sticky; top: 0; max-height: calc(100vh - 120px); display: flex; flex-direction: column; }
  .feed ul { list-style: none; margin: 0; padding: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 4px; }
  .feed li { display: flex; justify-content: space-between; gap: 10px; font-size: 12.5px; padding: 7px 10px; border-radius: 6px; color: var(--text-2); background: var(--panel-2); border-left: 2px solid var(--line-2); }
  /* Flux : neutre ; seul le clip enregistré ressort (texte blanc) */
  .feed li.k-clip { border-left-color: var(--hi); color: var(--text); font-weight: 500; }
  .feed li.k-death { opacity: 0.6; }
  .empty { font-size: 13px; margin: 0; }
</style>
