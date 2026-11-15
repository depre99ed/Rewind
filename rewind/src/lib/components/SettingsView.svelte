<script lang="ts">
  import { t } from "$lib/i18n.svelte";
  import { onMount } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { Video, AudioLines, Keyboard, HardDrive, Share2, Palette, Power, Stethoscope, Plus, Trash2, FolderOpen, MonitorUp, Layers, Bell } from "@lucide/svelte";
  import KeysOverlay from "./KeysOverlay.svelte";
  import type { KeysPreset } from "$lib/studio/render";

  // Superposition : petite démo qui joue en boucle (avance, saut, tirs, accroupi)
  const DEMO = [
    [0x57, 0, 2.6], [0x41, 0.5, 0.9], [0x44, 1.3, 1.7], [0x20, 1.9, 2.1], [0x01, 2.3, 2.4], [0x01, 2.6, 2.7],
    [0xa0, 3.0, 3.8], [0xa2, 3.4, 3.9], [0x02, 3.1, 3.6], [0x31, 0.2, 0.4], [0x58, 1.0, 1.2], [0x46, 2.8, 3.0],
  ].flatMap(([key, a, b]) => [{ t: a, key, down: true }, { t: b, key, down: false }]);
  let demoT = $state(0);
  $effect(() => {
    if (section !== "overlay") return;
    const start = performance.now();
    const id = setInterval(() => (demoT = ((performance.now() - start) / 1000) % 4), 80);
    return () => clearInterval(id);
  });
  import { app } from "$lib/state.svelte";
  import { api, type Hardware, type AudioSource, type VoiceHeard } from "$lib/api";
  import HotkeyInput from "./HotkeyInput.svelte";

  let section = $derived((typeof window !== "undefined" && (window as any).__REWIND_MOCK__ && new URLSearchParams(location.search).get("section")) || app.settingsSection);
  let hw = $state<Hardware | null>(null);
  let log = $state("");
  let newApp = $state("");
  onMount(async () => (hw = await api.hardware()));

  // Commande vocale : dernière fois que « Rewind clip » a été entendu (pour tester sa voix)
  let heard = $state<VoiceHeard | null>(null);
  let nowSec = $state(Date.now() / 1000);
  $effect(() => {
    if (!s?.voice_command || section !== "keys") return;
    const id = setInterval(async () => {
      nowSec = Date.now() / 1000;
      heard = await api.voiceLast().catch(() => null);
    }, 1000);
    return () => clearInterval(id);
  });

  const s = $derived(app.settings!);
  // Rangement : ce qu'on filme, ce qu'on garde, ce qu'on entend, comment on déclenche, ce qui s'affiche, le reste
  const sections = [
    ["video", "Enregistrement", Video],
    ["clips", "Clips et stockage", HardDrive],
    ["audio", "Audio", AudioLines],
    ["keys", "Raccourcis", Keyboard],
    ["notif", "Notifications", Bell],
    ["overlay", "Superposition", Layers],
    ["share", "Partage", Share2],
    ["look", "Apparence", Palette],
    ["general", "Système", Power],
    ["diag", "Diagnostic", Stethoscope],
  ] as const;
  const accents = ["#c8f031", "#ff5d5d", "#ff8a3d", "#ffc53d", "#3ddc97", "#38bdf8", "#8b8cff", "#f472b6", "#eef0f4"];
  const encoderNames: Record<string, string> = {
    auto: "Automatique (recommandé)", // traduit à l'affichage
    h264_nvenc: "NVIDIA NVENC · H.264",
    hevc_nvenc: "NVIDIA NVENC · H.265",
    av1_nvenc: "NVIDIA NVENC · AV1",
    h264_amf: "AMD · H.264",
    hevc_amf: "AMD · H.265",
    av1_amf: "AMD · AV1",
    h264_qsv: "Intel QuickSync · H.264",
    hevc_qsv: "Intel QuickSync · H.265",
    libx264: "Processeur (x264, lent)",
  };
  const est = $derived(Math.round((s.bitrate_mbps / 8) * s.buffer_seconds));

  async function chooseFolder() {
    const dir = await openDialog({ directory: true, defaultPath: s.clip_folder, title: t("Dossier des clips") });
    if (typeof dir === "string") app.patch((x) => (x.clip_folder = dir));
  }

  function addApp() {
    const exe = newApp.trim();
    if (!exe) return;
    const name = exe.replace(/\.exe$/i, "");
    app.patch((x) =>
      x.audio_sources.push({ id: `app-${Date.now()}`, kind: "app", device: "", app: exe.toLowerCase().endsWith(".exe") ? exe : `${exe}.exe`, label: name.charAt(0).toUpperCase() + name.slice(1), enabled: true, volume: 100 }),
    );
    newApp = "";
  }
  function addSource(kind: "system" | "mic") {
    app.patch((x) => x.audio_sources.push({ id: `${kind}-${Date.now()}`, kind, device: "", app: "", label: kind === "mic" ? "Micro 2" : "Sortie 2", enabled: true, volume: 100 }));
  }
  function patchSource(i: number, fn: (a: AudioSource) => void) {
    app.patch((x) => fn(x.audio_sources[i]));
  }

  // Jeux lancés en administrateur : assistant d'entrées (tâche Windows créée avec une seule autorisation)
  const adm = $derived(app.adminInput);
  let admBusy = $state(false);
  async function setAdmin() {
    admBusy = true;
    const on = !adm?.enabled;
    try {
      await api.adminInputSet(on);
      app.toast("ok", t(on ? "Assistant autorisé" : "Assistant retiré"), on ? t("Les raccourcis marchent maintenant dans les jeux lancés en administrateur.") : "");
    } catch (e) {
      app.toast("err", t("Autorisation impossible"), String(e));
    }
    app.adminInput = await api.adminInputStatus().catch(() => app.adminInput);
    admBusy = false;
  }

  // Mises à jour
  const upd = $derived(app.update);
  let checking = $state(false);
  let installing = $state(false);
  async function checkUpdate() {
    checking = true;
    try { app.update = await api.checkUpdate(); } catch (e) { app.toast("err", t("Mise à jour"), String(e)); }
    checking = false;
  }
  // Rapport de diagnostic
  let reporting = $state<"" | "copy" | "save">("");
  async function makeReport(copy: boolean) {
    reporting = copy ? "copy" : "save";
    try {
      const r = await api.diagnosticReport(copy, !copy);
      const info = t("{n} Ko · {p} points à regarder", { n: Math.max(1, Math.round(r.text.length / 1000)), p: r.problems });
      app.toast("ok", t(copy ? "Rapport copié" : "Rapport enregistré"), copy ? `${info} · ${t("Colle-le avec Ctrl+V")}` : info);
    } catch (e) {
      app.toast("err", t("Rapport impossible"), String(e));
    }
    reporting = "";
  }
  async function installUpdate() {
    installing = true;
    try { await api.installUpdate(); } catch (e) { app.toast("err", t("Mise à jour impossible"), String(e)); installing = false; }
  }
</script>

{#if s}
  <section class="settings">
    <nav>
      <h1>{t("Réglages")}</h1>
      {#each sections as [id, label, Icon]}
        <button class:on={section === id} onclick={() => { app.settingsSection = id; if (id === "diag") api.engineLog().then((l) => (log = l)); }}>
          <Icon size={16} />{t(label)}
        </button>
      {/each}
    </nav>

    <div class="content">
      {#key section}
        <div class="pane appear">
          {#if section === "video"}
            <h2>{t("Enregistrement")}</h2>
            <p class="lead muted">{t("Rewind filme en continu et ne garde que les dernières minutes. L'image est capturée et encodée par la carte graphique : l'impact sur les jeux est minime.")}</p>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Enregistrement actif")}</strong><span>{t("Désactive pour mettre Rewind en pause.")}</span></div>
                <span class="switch"><input type="checkbox" checked={s.recording_enabled} onchange={(e) => app.patch((x) => (x.recording_enabled = e.currentTarget.checked))} /><span></span></span>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Que filmer")}</strong><span>{t("Un écran entier (jeux, Discord, navigateur…) ou une seule application.")}</span></div>
                <div class="seg">
                  <button class:on={s.capture_mode === "monitor"} onclick={() => app.patch((x) => (x.capture_mode = "monitor"))}>{t("Écran")}</button>
                  <button class:on={s.capture_mode === "window"} onclick={() => app.patch((x) => (x.capture_mode = "window"))}>{t("Application")}</button>
                </div>
              </div>
              {#if s.capture_mode === "monitor"}
                <div class="item">
                  <div class="lbl"><strong>{t("Écran")}</strong><span>{t("Celui où tu joues.")}</span></div>
                  <select class="field" value={s.monitor} onchange={(e) => app.patch((x) => (x.monitor = e.currentTarget.value))}>
                    <option value="">{t("Écran principal")}</option>
                    {#each hw?.monitors ?? [] as m, i}
                      <option value={m.name}>{t("Écran {n}", { n: i + 1 })} · {m.width}×{m.height}{m.primary ? ` (${t("principal")})` : ""}</option>
                    {/each}
                  </select>
                </div>
              {:else}
                <div class="item">
                  <div class="lbl"><strong>{t("Application")}</strong><span>{t("Nom du programme (ex. cs2.exe). Rewind attend qu'il soit ouvert.")}</span></div>
                  <input class="field" list="apps" value={s.window_exe} placeholder={t("cs2.exe")} onchange={(e) => app.patch((x) => (x.window_exe = e.currentTarget.value.trim()))} />
                  <datalist id="apps">{#each hw?.apps ?? [] as a}<option value={a}></option>{/each}</datalist>
                </div>
              {/if}
              <div class="item">
                <div class="lbl"><strong>{t("Afficher la souris")}</strong></div>
                <span class="switch"><input type="checkbox" checked={s.show_cursor} onchange={(e) => app.patch((x) => (x.show_cursor = e.currentTarget.checked))} /><span></span></span>
              </div>
            </div>

            <h3>{t("Qualité")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Résolution")}</strong><span>{t("Appliquée au petit côté de l'écran.")}</span></div>
                <select class="field" value={String(s.resolution)} onchange={(e) => app.patch((x) => (x.resolution = +e.currentTarget.value))}>
                  <option value="720">720p</option><option value="1080">1080p</option><option value="1440">1440p</option><option value="2160">4K</option><option value="0">{t("Taille réelle de l'écran")}</option>
                </select>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Images par seconde")}</strong></div>
                <div class="seg four">
                  {#each [30, 60, 120, 144] as f}<button class:on={s.fps === f} onclick={() => app.patch((x) => (x.fps = f))}>{f}</button>{/each}
                </div>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Débit : {n} Mb/s", { n: s.bitrate_mbps })}</strong><span>{t("Plus haut = plus net mais plus lourd. 15 à 30 pour du 1080p60.")}</span></div>
                <input type="range" min="4" max="80" step="1" value={s.bitrate_mbps} oninput={(e) => app.patch((x) => (x.bitrate_mbps = +e.currentTarget.value))} />
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Encodeur")}</strong><span>{t("Puce vidéo utilisée pour compresser.")}</span></div>
                <select class="field" value={s.encoder} onchange={(e) => app.patch((x) => (x.encoder = e.currentTarget.value))}>
                  <option value="auto">{t(encoderNames.auto)}</option>
                  {#each hw?.encoders ?? [] as e}<option value={e}>{t(encoderNames[e] ?? e)}</option>{/each}
                </select>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Mode économie")}</strong><span>{t("Pour les petits PC : limite à 720p, 30 i/s et 8 Mb/s.")}</span></div>
                <span class="switch"><input type="checkbox" checked={s.eco_mode} onchange={(e) => app.patch((x) => (x.eco_mode = e.currentTarget.checked))} /><span></span></span>
              </div>
            </div>

          {:else if section === "clips"}
            <h2>{t("Clips et stockage")}</h2>
            <p class="lead muted">{t("La durée de chaque clip, ce que Rewind garde en mémoire, et où les clips sont rangés.")}</p>
            <h3>{t("Durées")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Clip court : {n} s", { n: s.clip_seconds })}</strong><span>{t("Raccourci principal.")}</span></div>
                <input type="range" min="5" max="300" step="5" value={s.clip_seconds} oninput={(e) => app.patch((x) => (x.clip_seconds = +e.currentTarget.value))} />
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Clip long")} : {s.clip_long_seconds >= 60 ? `${(s.clip_long_seconds / 60).toFixed(1).replace(".0", "")} min` : `${s.clip_long_seconds} s`}</strong><span>{t("Second raccourci.")}</span></div>
                <input type="range" min="10" max="1200" step="10" value={s.clip_long_seconds} oninput={(e) => app.patch((x) => (x.clip_long_seconds = +e.currentTarget.value))} />
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Mémoire tampon")} : {s.buffer_seconds >= 60 ? `${(s.buffer_seconds / 60).toFixed(1).replace(".0", "")} min` : `${s.buffer_seconds} s`}</strong><span>{t("Environ {n} Mo sur le disque, effacés en continu.", { n: est })}</span></div>
                <input type="range" min="30" max="1800" step="30" value={s.buffer_seconds} oninput={(e) => app.patch((x) => (x.buffer_seconds = +e.currentTarget.value))} />
              </div>
            </div>
            <h3>{t("Rangement")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Dossier des clips")}</strong><span class="mono path">{s.clip_folder}</span></div>
                <div class="inline"><button class="btn" onclick={chooseFolder}>{t("Changer")}</button><button class="btn icon" onclick={() => api.openFolder(s.clip_folder)} aria-label={t("Ouvrir")}><FolderOpen size={15} /></button></div>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Un sous-dossier par jeu")}</strong><span>{t("Ex. Rewind\\Counter-Strike 2\\…")}</span></div>
                <span class="switch"><input type="checkbox" checked={s.folder_per_game} onchange={(e) => app.patch((x) => (x.folder_per_game = e.currentTarget.checked))} /><span></span></span>
              </div>
            </div>
            <p class="faint note">{t("Rien n'est jamais supprimé définitivement : la suppression envoie à la Corbeille de Windows.")}</p>
          {:else if section === "audio"}
            <h2>{t("Audio")}</h2>
            <p class="lead muted">{t("Chaque source est enregistrée sur sa propre piste en plus du mixage : tu peux couper un pote ou ton micro au montage.")}</p>
            <div class="group">
              {#each s.audio_sources as src, i (src.id)}
                {@const live = app.status?.audio.find((a) => a.label === src.label)}
                <div class="source" class:off={!src.enabled}>
                  <span class="switch"><input type="checkbox" checked={src.enabled} onchange={(e) => patchSource(i, (a) => (a.enabled = e.currentTarget.checked))} /><span></span></span>
                  <div class="sinfo">
                    <input class="name" value={src.label} onchange={(e) => patchSource(i, (a) => (a.label = e.currentTarget.value || a.label))} />
                    {#if src.kind === "app"}
                      <span class="faint">{t("Application")} · {src.app}{live && !live.active ? ` · ${t("pas lancée")}` : ""}</span>
                    {:else}
                      <select class="field mini" value={src.device} onchange={(e) => patchSource(i, (a) => (a.device = e.currentTarget.value))}>
                        <option value="">{t(src.kind === "mic" ? "Micro par défaut de Windows" : "Sortie par défaut de Windows")}</option>
                        {#each (src.kind === "mic" ? hw?.inputs : hw?.outputs) ?? [] as d}<option value={d.id}>{d.name}</option>{/each}
                      </select>
                    {/if}
                  </div>
                  <div class="vol">
                    <span class="lvl"><i style="transform:scaleX({Math.min(1, Math.sqrt(live?.level ?? 0) * 1.1)})"></i></span>
                    <input type="range" min="0" max="200" step="5" value={src.volume} oninput={(e) => patchSource(i, (a) => (a.volume = +e.currentTarget.value))} />
                    <span class="mono faint pct">{src.volume} %</span>
                  </div>
                  <button class="btn small icon ghost" aria-label={t("Retirer")} onclick={() => app.patch((x) => x.audio_sources.splice(i, 1))}><Trash2 size={15} /></button>
                </div>
              {/each}
            </div>
            <div class="add">
              <input class="field" list="apps2" placeholder={t("Ajouter une application (ex. Discord.exe, Spotify.exe)")} bind:value={newApp} onkeydown={(e) => e.key === "Enter" && addApp()} />
              <datalist id="apps2">{#each hw?.apps ?? [] as a}<option value={a}></option>{/each}</datalist>
              <button class="btn" onclick={addApp}><Plus size={15} />{t("Application")}</button>
              <button class="btn" onclick={() => addSource("mic")}><Plus size={15} />{t("Micro")}</button>
              <button class="btn" onclick={() => addSource("system")}><Plus size={15} />{t("Sortie§audio")}</button>
            </div>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Pistes séparées")}</strong><span>{t("Une piste par source dans le fichier, en plus du mixage.")}</span></div>
                <span class="switch"><input type="checkbox" checked={s.separate_tracks} onchange={(e) => app.patch((x) => (x.separate_tracks = e.currentTarget.checked))} /><span></span></span>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Suppression du bruit du micro")}</strong><span>{t("Enlève le souffle, le ventilateur et les bruits de fond légers de ton micro.")}</span></div>
                <span class="switch"><input type="checkbox" checked={s.mic_denoise} onchange={(e) => app.patch((x) => (x.mic_denoise = e.currentTarget.checked))} /><span></span></span>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Décalage du son")} : {s.audio_offset_ms > 0 ? "+" : ""}{s.audio_offset_ms} ms</strong><span>{t("Si le son arrive après l'image dans tes clips, mets une valeur négative.")}</span></div>
                <input type="range" min="-300" max="300" step="5" value={s.audio_offset_ms} oninput={(e) => app.patch((x) => (x.audio_offset_ms = +e.currentTarget.value))} />
              </div>
            </div>
            <p class="faint note">{t("Quand une application a sa propre piste (Discord par défaut), « Son du PC » l'exclut automatiquement : elle n'est jamais entendue deux fois.")}</p>
          {:else if section === "overlay"}
            <h2>{t("Superposition")}</h2>
            <p class="lead muted">{t("Les touches du clavier et de la souris affichées sur tes clips, dans le lecteur, le Studio et les vidéos exportées.")}</p>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Touches clavier / souris à l'écran")}</strong><span>{t("Garde avec chaque clip les touches de jeu (ZQSD, espace, clics…) pour les afficher sur la vidéo. Notées seulement quand un jeu est au premier plan, jamais le texte que tu tapes ailleurs.")}</span></div>
                <span class="switch"><input type="checkbox" checked={s.input_overlay} onchange={(e) => app.patch((x) => (x.input_overlay = e.currentTarget.checked))} /><span></span></span>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Lettres")}</strong><span>{t("« Mon clavier » suit la disposition de Windows ; sinon force QWERTY ou AZERTY.")}</span></div>
                <div class="seg">
                  {#each [["auto", "Mon clavier"], ["qwerty", "QWERTY"], ["azerty", "AZERTY"]] as [id, label]}
                    <button class:on={s.overlay_layout === id} onclick={() => app.patch((x) => (x.overlay_layout = id))}>{t(label)}</button>
                  {/each}
                </div>
              </div>
            </div>
            <h3>{t("Nombre de touches")}</h3>
            <div class="presets">
              {#each [["compact", "Compact", "ZQSD, espace, Maj, Ctrl et souris"], ["standard", "Standard", "En plus : Tab, A, E, R, F, C"], ["complet", "Complet", "En plus : 1 à 5, T, G, W, X, V, B, Alt"]] as [id, label, hint]}
                <button class="preset" class:on={s.overlay_keys === id} onclick={() => app.patch((x) => (x.overlay_keys = id))}>
                  <span class="pv"><KeysOverlay events={DEMO} time={demoT} preset={id as KeysPreset} width={id === "complet" ? 190 : id === "compact" ? 150 : 190} floating={false} /></span>
                  <b>{t(label)}</b>
                  <small>{t(hint)}</small>
                </button>
              {/each}
            </div>
            <p class="faint note">{t("Dans le Studio, déplace et redimensionne les touches directement sur l'aperçu.")}</p>
          {:else if section === "keys"}
            <h2>{t("Raccourcis")}</h2>
            <p class="lead muted">{t("Fonctionnent partout, même en jeu. Clique sur un raccourci puis appuie sur la nouvelle combinaison.")}</p>
            <div class="group">
              <div class="item"><div class="lbl"><strong>{t("Clipper (court)")}</strong><span>{t("Garde les {n} dernières secondes.", { n: s.clip_seconds })}</span></div><HotkeyInput value={s.hotkeys.clip} onchange={(v) => app.patch((x) => (x.hotkeys.clip = v))} /></div>
              <div class="item"><div class="lbl"><strong>{t("Clipper (long)")}</strong><span>{t("Garde {n} secondes.", { n: s.clip_long_seconds })}</span></div><HotkeyInput value={s.hotkeys.clip_long} onchange={(v) => app.patch((x) => (x.hotkeys.clip_long = v))} /></div>
              <div class="item"><div class="lbl"><strong>{t("Clip sur mesure")}</strong><span>{t("1er appui : début. 2e appui : fin. Le clip fait exactement ce que tu as choisi.")}</span></div><HotkeyInput value={s.hotkeys.clip_custom} onchange={(v) => app.patch((x) => (x.hotkeys.clip_custom = v))} /></div>
              <div class="item"><div class="lbl"><strong>{t("Session complète")}</strong><span>{t("1er appui : début. 2e appui : un seul fichier avec tous tes kills repérés sur la frise.")}</span></div><HotkeyInput value={s.hotkeys.session} onchange={(v) => app.patch((x) => (x.hotkeys.session = v))} /></div>
              <div class="item"><div class="lbl"><strong>{t("Repère")}</strong><span>{t("Pendant une session ou un clip sur mesure : marque le moment.")}</span></div><HotkeyInput value={s.hotkeys.bookmark} onchange={(v) => app.patch((x) => (x.hotkeys.bookmark = v))} /></div>
              <div class="item"><div class="lbl"><strong>{t("Capture d'écran")}</strong></div><HotkeyInput value={s.hotkeys.screenshot} onchange={(v) => app.patch((x) => (x.hotkeys.screenshot = v))} /></div>
              <div class="item"><div class="lbl"><strong>{t("Pause / reprise")}</strong></div><HotkeyInput value={s.hotkeys.toggle_recording} onchange={(v) => app.patch((x) => (x.hotkeys.toggle_recording = v))} /></div>
            </div>
            <h3>{t("Jeux lancés en administrateur")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl">
                  <strong>{t("Raccourcis et touches dans ces jeux")}</strong>
                  <span>{t("Quand Steam ou un jeu est lancé « en tant qu'administrateur », Windows cache ses touches aux autres programmes : F8 ne marche qu'en dehors du jeu et les touches à l'écran restent vides. Rewind peut lancer un petit assistant autorisé à les lire (une seule demande de Windows).")}</span>
                  {#if adm?.rewind_admin}
                    <span class="vstate ok">{t("Rewind tourne déjà en administrateur : rien à faire.")}</span>
                  {:else if adm?.enabled}
                    <span class="vstate" class:ok={adm.helper}>{t(adm.helper ? "● Assistant actif" : "Assistant autorisé, démarrage…")}</span>
                  {:else if adm?.game_admin}
                    <span class="vstate bad">{t("Le jeu en cours tourne en administrateur : ses touches sont bloquées.")}</span>
                  {/if}
                </div>
                {#if !adm?.rewind_admin}
                  <button class="btn" class:primary={!adm?.enabled} onclick={setAdmin} disabled={admBusy}>{admBusy ? t("Autorisation…") : t(adm?.enabled ? "Retirer" : "Autoriser")}</button>
                {/if}
              </div>
            </div>
            <p class="faint note">{t("Autre solution : ne plus lancer Steam en administrateur (clic droit sur Steam › Propriétés › Compatibilité › décocher « Exécuter ce programme en tant qu'administrateur »), puis relancer Steam et le jeu.")}</p>
            <h3>{t("Commande vocale")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl">
                  <strong>{t("Dire « Rewind clip » pour clipper")}</strong>
                  <span>{t("Garde les {n} dernières secondes, comme le raccourci. Reconnaissance vocale de Windows, entièrement hors ligne : une seule phrase est reconnue, rien n'est enregistré ni envoyé. Écoute le micro choisi dans Audio. Prononce-le à l'anglaise : « ri-waïnd clip ».", { n: s.clip_seconds })}</span>
                  {#if s.voice_command}
                    <span class="vstate" class:ok={app.status?.voice === 2} class:bad={(app.status?.voice ?? 0) >= 3}>
                      {t(app.status?.voice === 2 ? "● À l'écoute" : app.status?.voice === 1 ? "Démarrage…" : app.status?.voice === 3 ? "Indisponible : ajoute la reconnaissance vocale anglaise (Paramètres Windows › Heure et langue › Langue › Anglais › Options › Reconnaissance vocale)" : app.status?.voice === 4 ? "Erreur : nouvel essai automatique" : "")}
                    </span>
                    {#if app.status?.voice === 2}
                      <span class="vtest">
                        {#if heard && nowSec - heard.at < 30}
                          {t(heard.accepted ? "Entendu il y a {s} s · sûr à {c} % · clip lancé" : heard.confidence >= 0.7 ? "Entendu il y a {s} s · phrase trop longue : pas de clip (dis seulement « rewind clip »)" : "Entendu il y a {s} s · sûr à {c} % seulement : pas de clip (articule « ri-waïnd clip »)", { s: Math.max(0, Math.round(nowSec - heard.at)), c: Math.round(heard.confidence * 100) })}
                        {:else}
                          {t("Pour tester, dis « Rewind clip » : le résultat s'affiche ici.")}
                        {/if}
                      </span>
                    {/if}
                  {/if}
                </div>
                <span class="switch"><input type="checkbox" checked={s.voice_command} onchange={(e) => app.patch((x) => (x.voice_command = e.currentTarget.checked))} /><span></span></span>
              </div>
            </div>
          {:else if section === "notif"}
            <h2>{t("Notifications")}</h2>
            <p class="lead muted">{t("Ce que Rewind te montre ou te fait entendre pendant que tu joues.")}</p>
            <h3>{t("Quand un clip est gardé")}</h3>
            <div class="group">
              <div class="item"><div class="lbl"><strong>{t("Son quand un clip est gardé")}</strong></div><span class="switch"><input type="checkbox" checked={s.sound_on_clip} onchange={(e) => app.patch((x) => (x.sound_on_clip = e.currentTarget.checked))} /><span></span></span></div>
              <div class="item"><div class="lbl"><strong>{t("Notification à l'écran")}</strong><span>{t("Mini-bandeau animé quand un clip est gardé. Ne prend jamais la main sur le jeu (pas visible en plein écran exclusif).")}</span></div><span class="switch"><input type="checkbox" checked={s.notify_overlay} onchange={(e) => app.patch((x) => (x.notify_overlay = e.currentTarget.checked))} /><span></span></span></div>
              {#if s.notify_overlay}
                <div class="item"><div class="lbl"><strong>{t("Position")}</strong><span>{t("Milieu gauche : zone libre dans CS2 (sous le radar).")}</span></div>
                  <select class="field" value={s.notify_position} onchange={(e) => app.patch((x) => (x.notify_position = e.currentTarget.value))}>
                    <option value="left-middle">{t("Milieu gauche")}</option><option value="top-left">{t("En haut à gauche")}</option><option value="top-center">{t("En haut au centre")}</option><option value="top-right">{t("En haut à droite")}</option><option value="bottom-left">{t("En bas à gauche")}</option><option value="bottom-right">{t("En bas à droite")}</option>
                  </select>
                </div>
              {/if}
              <div class="item"><div class="lbl"><strong>{t("Volume du son")} : {s.sound_volume} %</strong></div><input type="range" min="5" max="100" step="5" value={s.sound_volume} oninput={(e) => app.patch((x) => (x.sound_volume = +e.currentTarget.value))} /></div>
            </div>
            <h3>{t("Au lancement d'un jeu")}</h3>
            <div class="group">
              <div class="item"><div class="lbl"><strong>{t("Bandeau des raccourcis")}</strong><span>{t("Pendant 6 s, le jeu reconnu et tes raccourcis s'affichent dans un coin (une fois par lancement).")}</span></div><span class="switch"><input type="checkbox" checked={s.welcome_banner} onchange={(e) => app.patch((x) => (x.welcome_banner = e.currentTarget.checked))} /><span></span></span></div>
            </div>
            <div class="tip">
              <MonitorUp size={18} />
              <div><strong>{t("Tu ne vois pas les notifications en jeu ?")}</strong><span>{t("Mets le jeu en « Plein écran fenêtré » (CS2 : Paramètres › Vidéo › Mode d'affichage). En plein écran exclusif, Windows ne laisse rien s'afficher par-dessus — l'enregistrement, lui, marche dans les deux cas.")}</span></div>
            </div>
          {:else if section === "share"}
            <h2>{t("Partage")}</h2>
            <p class="lead muted">{t("Le plus simple : « Copier pour Discord » dans un clip, puis Ctrl+V dans n'importe quelle conversation. Pour envoyer en un clic sur un salon, ajoute son webhook (Paramètres du salon → Intégrations → Webhooks → Copier l'URL).")}</p>
            <div class="group">
              {#each s.webhooks as w, i}
                <div class="hook">
                  <input class="field" value={w.name} placeholder={t("Nom")} onchange={(e) => app.patch((x) => (x.webhooks[i].name = e.currentTarget.value))} />
                  <input class="field url" type="password" value={w.url} placeholder="https://discord.com/api/webhooks/…" onchange={(e) => app.patch((x) => (x.webhooks[i].url = e.currentTarget.value.trim()))} />
                  <button class="btn icon ghost" aria-label={t("Retirer")} onclick={() => app.patch((x) => x.webhooks.splice(i, 1))}><Trash2 size={15} /></button>
                </div>
              {/each}
              <button class="btn" onclick={() => app.patch((x) => x.webhooks.push({ name: `${t("Salon")} ${x.webhooks.length + 1}`, url: "" }))}><Plus size={15} />{t("Ajouter un webhook")}</button>
            </div>
            <h3>{t("Copier pour Discord")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Taille maximale")} : {s.discord_max_mb} {t("Mo")}</strong><span>{t("Au-dessus, le clip est compressé (1080p, puis 720p, puis 480p) avant la copie. Sans Nitro ni serveur boosté, Discord accepte 10 Mo.")}</span></div>
                <div class="seg">
                  {#each [10, 20, 50, 100] as mb}<button class:on={s.discord_max_mb === mb} onclick={() => app.patch((x) => (x.discord_max_mb = mb))}>{mb} {t("Mo")}</button>{/each}
                </div>
              </div>
            </div>
          {:else if section === "look"}
            <h2>{t("Apparence")}</h2>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Thème")}</strong></div>
                <div class="seg"><button class:on={s.theme === "dark"} onclick={() => app.patch((x) => (x.theme = "dark"))}>{t("Sombre")}</button><button class:on={s.theme === "light"} onclick={() => app.patch((x) => (x.theme = "light"))}>{t("Clair")}</button></div>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Couleur d'accent")}</strong></div>
                <div class="swatches">
                  {#each accents as c}<button class="sw" class:on={s.accent.toLowerCase() === c} style="background:{c}" aria-label={c} onclick={() => app.patch((x) => (x.accent = c))}></button>{/each}
                  <label class="sw custom" title={t("Couleur libre")}><input type="color" value={s.accent} oninput={(e) => app.patch((x) => (x.accent = e.currentTarget.value))} /></label>
                </div>
              </div>
              <div class="item">
                <div class="lbl"><strong>{t("Animations")}</strong><span>{t("Transitions et aperçus animés.")}</span></div>
                <span class="switch"><input type="checkbox" checked={s.animations} onchange={(e) => app.patch((x) => (x.animations = e.currentTarget.checked))} /><span></span></span>
              </div>
            </div>
            <h3>{t("Langue")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl"><strong>{t("Langue de l'interface")}</strong><span>{t("Automatique : celle de Windows.")}</span></div>
                <div class="seg">
                  <button class:on={s.language === "auto"} onclick={() => app.patch((x) => (x.language = "auto"))}>{t("Auto")}</button>
                  <button class:on={s.language === "fr"} onclick={() => app.patch((x) => (x.language = "fr"))}>Français</button>
                  <button class:on={s.language === "en"} onclick={() => app.patch((x) => (x.language = "en"))}>English</button>
                </div>
              </div>
            </div>
          {:else if section === "general"}
            <h2>{t("Système")}</h2>
            <h3>{t("Démarrage")}</h3>
            <div class="group">
              <div class="item"><div class="lbl"><strong>{t("Lancer avec Windows")}</strong><span>{t("Rewind démarre discrètement dans la barre des tâches.")}</span></div><span class="switch"><input type="checkbox" checked={s.launch_at_startup} onchange={(e) => app.patch((x) => (x.launch_at_startup = e.currentTarget.checked))} /><span></span></span></div>
              <div class="item"><div class="lbl"><strong>{t("Démarrer réduit")}</strong><span>{t("Ne pas ouvrir la fenêtre au lancement.")}</span></div><span class="switch"><input type="checkbox" checked={s.start_minimized} onchange={(e) => app.patch((x) => (x.start_minimized = e.currentTarget.checked))} /><span></span></span></div>
              <div class="item"><div class="lbl"><strong>{t("Fermer = réduire")}</strong><span>{t("La croix cache la fenêtre ; l'enregistrement continue. Pour quitter : clic droit sur l'icône près de l'horloge.")}</span></div><span class="switch"><input type="checkbox" checked={s.close_to_tray} onchange={(e) => app.patch((x) => (x.close_to_tray = e.currentTarget.checked))} /><span></span></span></div>
            </div>
            <h3>{t("Mises à jour")}</h3>
            <div class="group">
              <div class="item">
                <div class="lbl">
                  <strong>Rewind {upd?.current ?? ""}</strong>
                  <span>
                    {#if checking}{t("Recherche…")}
                    {:else if upd && !upd.configured}{t("Le canal de mises à jour n'est pas encore branché (dépôt GitHub à créer).")}
                    {:else if upd?.available}{t("Version {v} disponible.", { v: upd.available })}
                    {:else if upd}{t("Tu as la dernière version.")}
                    {:else}{t("Les mises à jour sont signées : une version qui ne vient pas de Rewind est refusée.")}{/if}
                  </span>
                </div>
                {#if upd?.available}
                  <button class="btn primary" onclick={installUpdate} disabled={installing}>{installing ? `${t("Installation…")} ${Math.round((app.progress?.op === "update" ? app.progress.value : 0) * 100)} %` : t("Installer et relancer")}</button>
                {:else}
                  <button class="btn" onclick={checkUpdate} disabled={checking}>{t("Rechercher")}</button>
                {/if}
              </div>
              <div class="item"><div class="lbl"><strong>{t("Rechercher automatiquement")}</strong><span>{t("Au démarrage puis toutes les 4 heures. Installation à ta demande.")}</span></div><span class="switch"><input type="checkbox" checked={s.auto_update} onchange={(e) => app.patch((x) => (x.auto_update = e.currentTarget.checked))} /><span></span></span></div>
            </div>
            <button class="btn danger" onclick={() => api.quit()}><Power size={15} />{t("Quitter Rewind")}</button>
          {:else if section === "diag"}
            <h2>{t("Diagnostic")}</h2>
            <div class="group">
              <div class="item">
                <div class="lbl">
                  <strong>{t("Rapport de diagnostic")}</strong>
                  <span>{t("Version, système, matériel, réglages, erreurs, plantages et journaux en un seul texte. Sans mot de passe ni adresse de webhook. À envoyer quand tu demandes de l'aide.")}</span>
                </div>
                <div class="inline">
                  <button class="btn primary" onclick={() => makeReport(true)} disabled={reporting !== ""}>{reporting === "copy" ? t("Préparation…") : t("Copier le rapport")}</button>
                  <button class="btn" onclick={() => makeReport(false)} disabled={reporting !== ""}>{reporting === "save" ? t("Préparation…") : t("Enregistrer en fichier")}</button>
                </div>
              </div>
            </div>
            <div class="group">
              <div class="item"><div class="lbl"><strong>{t("Encodeurs disponibles")}</strong></div><span class="mono">{hw?.encoders.join(", ") ?? "…"}</span></div>
              <div class="item"><div class="lbl"><strong>{t("Moteur")}</strong></div><span class="mono path">{hw?.ffmpeg || t("introuvable")}</span></div>
              <div class="item"><div class="lbl"><strong>{t("État")}</strong></div><span class="mono">{app.status?.recording ? t("enregistre · {w}×{h} · {mb} Mo en tampon", { w: app.status.width, h: app.status.height, mb: app.status.buffer_mb.toFixed(0) }) : app.status?.error || t("arrêté")}</span></div>
            </div>
            <h3>{t("Journal du moteur")}</h3>
            <pre class="log">{log || t("Vide (aucun avertissement).")}</pre>
          {/if}
        </div>
      {/key}
    </div>
  </section>
{/if}

<style>
  .settings { height: 100%; display: flex; }
  nav { width: 220px; flex: none; padding: 22px 14px; display: flex; flex-direction: column; gap: 2px; border-right: 1px solid var(--line); }
  nav h1 { font-family: var(--display); font-weight: 600; font-size: 24px; letter-spacing: -0.02em; margin: 0 10px 16px; }
  nav button { display: flex; align-items: center; gap: 10px; height: 36px; padding: 0 10px; border-radius: 9px; color: var(--text-2); font-weight: 500; text-align: left; }
  nav button:hover { background: var(--panel); color: var(--text); }
  nav button.on { background: var(--panel-2); color: var(--text); }
  .content { flex: 1; overflow-y: auto; padding: 26px 36px 60px; }
  .pane { max-width: 760px; display: flex; flex-direction: column; gap: 14px; }
  h2 { margin: 0; font-size: 20px; font-weight: 650; }
  h3 { margin: 14px 0 0; font-size: 13px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-3); }
  .lead { margin: -6px 0 0; font-size: 13.5px; max-width: 640px; }
  .group { background: var(--panel); border: 1px solid var(--line); border-radius: var(--radius-l); padding: 4px 18px; display: flex; flex-direction: column; }
  .group > .btn { margin: 12px 0; align-self: flex-start; }
  .item { display: flex; align-items: center; justify-content: space-between; gap: 24px; min-height: 62px; padding: 10px 0; }
  .item + .item { border-top: 1px solid var(--line); }
  .lbl { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .lbl span { font-size: 12.5px; color: var(--text-3); }
  .lbl strong { font-weight: 550; }
  .path { font-size: 12px; word-break: break-all; }
  .item input[type="range"] { width: 220px; flex: none; }
  .item select.field, .item input.field { min-width: 240px; }
  .inline { display: flex; gap: 8px; }
  .seg { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; padding: 3px; border-radius: 10px; background: var(--bg-2); border: 1px solid var(--line); flex: none; }
  .seg button { height: 30px; padding: 0 16px; border-radius: 7px; font-size: 13px; color: var(--text-2); white-space: nowrap; }
  .seg button.on { background: var(--panel-3); color: var(--text); }
  .source { display: grid; grid-template-columns: auto minmax(0, 1fr) 270px auto; align-items: center; gap: 14px; padding: 12px 0; }
  .source + .source { border-top: 1px solid var(--line); }
  .source.off .sinfo, .source.off .vol { opacity: 0.45; }
  .sinfo { display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; min-width: 0; }
  .name { font-weight: 600; font-size: 14px; background: transparent; border: 1px solid transparent; border-radius: 6px; padding: 2px 6px; margin-left: -6px; outline: none; user-select: text; }
  .name:hover, .name:focus { border-color: var(--line-2); }
  .field.mini { height: 30px; font-size: 12.5px; }
  .vol { display: grid; grid-template-columns: 1fr; gap: 6px; }
  .vol input { width: 100%; }
  .vol .pct { font-size: 11px; text-align: right; margin-top: -4px; }
  .lvl { height: 4px; border-radius: 4px; background: var(--panel-3); overflow: hidden; }
  .lvl i { display: block; height: 100%; background: var(--hi); transform-origin: left; transition: transform 120ms linear; }
  .add { display: flex; gap: 8px; }
  .add .field { flex: 1; }
  .note { font-size: 12.5px; margin: 0; }
  /* Superposition : trois tailles de clavier, chacune avec son aperçu animé */
  .presets { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
  .preset { display: flex; flex-direction: column; align-items: flex-start; gap: 4px; padding: 12px; border-radius: var(--radius-l); background: var(--panel); border: 1px solid var(--line); text-align: left; transition: border-color var(--dur) var(--ease); }
  .preset:hover { border-color: var(--line-2); }
  .preset.on { border-color: #fff; }
  .preset .pv { width: 100%; height: 150px; display: grid; place-items: center; margin-bottom: 6px; border-radius: 10px; background: linear-gradient(160deg, #23262e, #121318); }
  .preset b { font-size: 13.5px; font-weight: 600; }
  .preset small { font-size: 12px; color: var(--text-3); line-height: 1.4; }
  .vstate { margin-top: 4px; font-size: 12px; font-weight: 600; color: var(--text-3); }
  .vstate.ok { color: var(--accent); }
  .vstate.bad { color: var(--rec); }
  .vtest { margin-top: 2px; font-size: 12px; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .tip { display: flex; gap: 12px; align-items: flex-start; padding: 14px 16px; border-radius: var(--radius-l); background: color-mix(in srgb, var(--hi) 8%, var(--panel)); border: 1px solid color-mix(in srgb, var(--hi) 35%, var(--line)); }
  .tip :global(svg) { color: var(--hi); flex: none; margin-top: 2px; }
  .tip div { display: flex; flex-direction: column; gap: 3px; font-size: 13px; }
  .tip span { color: var(--text-2); line-height: 1.5; }
  .hook { display: grid; grid-template-columns: 160px 1fr auto; gap: 8px; padding: 12px 0 0; }
  .swatches { display: flex; gap: 8px; align-items: center; }
  .sw { width: 26px; height: 26px; border-radius: 50%; border: 2px solid var(--panel); box-shadow: 0 0 0 1px var(--line-2); transition: transform 140ms var(--ease); }
  .sw:hover { transform: scale(1.12); }
  .sw.on { box-shadow: 0 0 0 2px var(--text); }
  .sw.custom { position: relative; overflow: hidden; background: conic-gradient(#ff5d5d, #ffc53d, #3ddc97, #38bdf8, #8b8cff, #f472b6, #ff5d5d); cursor: pointer; }
  .sw.custom input { position: absolute; inset: -8px; opacity: 0; cursor: pointer; }
  .log { margin: 0; padding: 14px; border-radius: var(--radius); background: var(--bg-2); border: 1px solid var(--line); font-family: var(--mono); font-size: 11.5px; color: var(--text-2); white-space: pre-wrap; max-height: 320px; overflow: auto; user-select: text; }
</style>
