import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, logFront, type Clip, type Settings, type Status, type Cs2Event, type Moment, type UpdateInfo } from "./api";
import { installErrorLog } from "./errlog";
import { activity } from "./activity.svelte";
import { t } from "./i18n.svelte";

// Le Studio (montage) vit dans studio/store.svelte.ts depuis la 0.3.0 (projets, calques, pistes son).

type Toast = { id: number; kind: "ok" | "err" | "info"; title: string; text?: string; clip?: Clip };

class AppState {
  settings = $state<Settings | null>(null);
  status = $state<Status | null>(null);
  clips = $state<Clip[]>([]);
  /** Mes clips : tout sauf les clips reçus d'amis (ceux-là ne sont visibles que dans l'onglet Amis) */
  ownClips = $derived(this.clips.filter((c) => c.kind !== "received"));
  toasts = $state<Toast[]>([]);
  view = $state<"library" | "studio" | "friends" | "cs2" | "settings">("library");
  settingsSection = $state("video");
  /** Fenêtre d'accueil affichée (premier démarrage ou « Revoir l'accueil ») */
  showWelcome = $state(false);
  /** Jeux lancés en administrateur (relu toutes les 5 s par la barre de gauche) */
  adminInput = $state<import("./api").AdminInput | null>(null);
  openClip = $state<Clip | null>(null);
  progress = $state<{ op: string; value: number } | null>(null);
  cs2Feed = $state<{ at: number; text: string; kind: string }[]>([]);
  cs2Map = $state<{ map: string; mode: string } | null>(null);
  clipping = $state(false);
  /** Dernier résultat de la recherche de mise à jour. */
  update = $state<UpdateInfo | null>(null);
  private toastId = 0;

  async init() {
    installErrorLog();
    activity.start();
    this.settings = await api.getSettings();
    this.applyLook();
    this.clips = await api.clips();
    this.poll();
    await listen<Clip>("clip-saved", (e) => {
      this.clipping = false;
      this.clips = [e.payload, ...this.clips.filter((c) => c.id !== e.payload.id)];
      const shot = e.payload.kind === "screenshot";
      this.toast("ok", t(shot ? "Capture enregistrée" : "Clip enregistré"), e.payload.title, e.payload);
    });
    await listen<string>("clip-failed", (e) => {
      this.clipping = false;
      this.toast("err", t("Clip impossible"), e.payload);
    });
    await listen<number>("clip-started", () => {
      this.clipping = true;
      setTimeout(() => (this.clipping = false), 6000);
    });
    await listen<Settings>("settings-changed", (e) => {
      this.settings = e.payload;
      this.applyLook();
    });
    // Plusieurs changements rapprochés (miniatures, collections…) = une seule relecture de la liste
    let libTimer: ReturnType<typeof setTimeout> | undefined;
    await listen("library-changed", () => {
      clearTimeout(libTimer);
      libTimer = setTimeout(async () => {
        try { this.clips = await api.clips(); } catch {}
      }, 200);
    });
    await listen<{ op: string; value: number }>("edit-progress", (e) => {
      this.progress = e.payload.value >= 1 ? null : e.payload;
    });
    await listen<Cs2Event>("cs2-event", (e) => this.onCs2(e.payload));
    await listen<UpdateInfo>("update-available", (e) => (this.update = e.payload));
    // Pause automatique : retour au jeu, fenêtre réduite ou cachée (jamais de reprise toute seule)
    await listen("game-front", () => this.pauseMedia());
    document.addEventListener("visibilitychange", () => {
      if (document.hidden) this.pauseMedia();
      else this.pollNow();
    });
    window.addEventListener("focus", () => this.pollNow());
    try {
      // Redimensionner envoie des dizaines d'événements par seconde : on ne vérifie qu'à la fin.
      const win = getCurrentWindow();
      let resizeTimer: ReturnType<typeof setTimeout> | undefined;
      await win.onResized(() => {
        clearTimeout(resizeTimer);
        resizeTimer = setTimeout(async () => {
          try { if (await win.isMinimized()) this.pauseMedia(); } catch {}
        }, 150);
      });
    } catch {}
  }

  /** Met en pause tout ce qui joue (lecteur, Studio) : on ne relance jamais tout seul. */
  pauseMedia() {
    document.querySelectorAll<HTMLMediaElement>("video, audio").forEach((m) => {
      if (!m.paused) m.pause();
    });
    window.dispatchEvent(new CustomEvent("rewind-pause"));
  }

  private onCs2(ev: Cs2Event) {
    if ("Connected" in ev) {
      this.cs2Map = ev.Connected;
      this.pushFeed(t("Partie détectée : {map} ({mode})", { map: ev.Connected.map, mode: ev.Connected.mode || t("mode inconnu") }), "info");
    } else if ("Moment" in ev) {
      const m: Moment = ev.Moment;
      const labels: Record<string, string> = {
        kill: "Kill", headshot: "Headshot", onetap: "One Tap", knife: t("Kill au couteau"), zeus: t("Kill au Zeus"),
        collateral: "Collateral", death: t("Mort"), assist: "Assist", streak: t("Série de {n} kills", { n: m.count ?? "" }),
      };
      const multi = m.multi ? ` · ${m.multi.toUpperCase()}` : "";
      const weapon = m.weapon ? ` · ${m.weapon.toUpperCase()}` : "";
      this.pushFeed(`${labels[m.kind] ?? m.kind}${multi}${weapon}`, m.kind);
    } else if ("Clip" in ev) {
      this.pushFeed(`${t("Clip automatique")} : ${ev.Clip.title}`, "clip");
    }
  }

  private pushFeed(text: string, kind: string) {
    this.cs2Feed = [{ at: Date.now(), text, kind }, ...this.cs2Feed].slice(0, 60);
  }

  // État du moteur : 400 ms au premier plan (inchangé), 1 s derrière le jeu, 5 s réduite ou masquée.
  // Un état identique au précédent ne remplace pas l'objet (rien ne se recalcule dans l'interface).
  private pollTimer: ReturnType<typeof setTimeout> | undefined;
  private polling = false;
  private lastStatus = "";
  private async poll() {
    clearTimeout(this.pollTimer);
    this.polling = true;
    try {
      const s = await api.status();
      const json = JSON.stringify(s);
      if (json !== this.lastStatus) {
        this.lastStatus = json;
        this.status = s;
      }
    } catch {}
    this.polling = false;
    const delay = !activity.visible ? 5000 : activity.focused ? 400 : 1000;
    this.pollTimer = setTimeout(() => this.poll(), delay);
  }
  /** Relit l'état tout de suite (retour au premier plan) au lieu d'attendre le prochain tour. */
  private pollNow() {
    if (!this.polling) this.poll();
  }

  toast(kind: Toast["kind"], title: string, text?: string, clip?: Clip) {
    const id = ++this.toastId;
    if (kind === "err") logFront("warn", `message d'erreur affiché : ${title}${text ? " — " + text : ""}`);
    this.toasts =[...this.toasts, { id, kind, title, text, clip }].slice(-4);
    setTimeout(() => (this.toasts = this.toasts.filter((t) => t.id !== id)), kind === "err" ? 7000 : 4200);
  }

  async save(next: Settings) {
    try {
      const res = await api.saveSettings(next);
      this.settings = next;
      this.applyLook();
      if (res.hotkey_errors.length) this.toast("err", t("Raccourci indisponible"), res.hotkey_errors.join(" · "));
      return res;
    } catch (e) {
      this.toast("err", t("Réglages non enregistrés"), String(e));
    }
  }

  /** Modifie un réglage et l'enregistre aussitôt. */
  async patch(fn: (s: Settings) => void) {
    if (!this.settings) return;
    const next: Settings = JSON.parse(JSON.stringify(this.settings));
    fn(next);
    this.settings = next;
    this.applyLook();
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.save(next), 350);
  }
  private saveTimer: ReturnType<typeof setTimeout> | undefined;

  applyLook() {
    const s = this.settings;
    if (!s) return;
    const root = document.documentElement;
    root.dataset.theme = s.theme === "light" ? "light" : "dark";
    root.dataset.motion = s.animations ? "on" : "off";
    root.dataset.density = s.density;
    root.style.setProperty("--accent", s.accent || "#c8f031");
    root.style.setProperty("--accent-ink", inkFor(s.accent || "#c8f031"));
  }

  /** Copie prête pour Discord : compresse sous la taille réglée si besoin, puis presse-papiers. */
  discordBusy = $state<string | null>(null);
  async copyForDiscord(c: Clip) {
    if (this.discordBusy) return;
    this.discordBusy = c.id;
    try {
      const r = await api.copyForDiscord(c.id);
      this.toast(
        "ok",
        t("Prêt à coller dans Discord"),
        r.compressed
          ? t("Compressé à {mb} Mo ({h}p, limite {max} Mo). Ctrl+V dans Discord.", { mb: r.size_mb.toFixed(1), h: r.height, max: r.max_mb })
          : t("{mb} Mo, aucune compression nécessaire. Ctrl+V dans Discord.", { mb: r.size_mb.toFixed(1) }),
      );
    } catch (e) {
      this.toast("err", t("Trop lourd pour Discord"), String(e));
    }
    this.discordBusy = null;
  }

  /** Ouvre le Studio avec ces clips : dans le projet ouvert s'il est vide, sinon dans un nouveau projet. */
  async openStudio(clips: Clip[]) {
    this.openClip = null;
    this.view = "studio";
    const { openInStudio } = await import("./studio/store.svelte");
    await openInStudio(clips);
  }

  updateClip(c: Clip) {
    this.clips = this.clips.map((x) => (x.id === c.id ? c : x));
    if (this.openClip?.id === c.id) this.openClip = c;
  }

  /** Remplace plusieurs clips modifiés d'un coup (collections). */
  updateClips(list: Clip[]) {
    if (!list.length) return;
    const by = new Map(list.map((c) => [c.id, c]));
    this.clips = this.clips.map((x) => by.get(x.id) ?? x);
    if (this.openClip && by.has(this.openClip.id)) this.openClip = by.get(this.openClip.id)!;
  }

  /** Collections existantes (orthographe d'origine, sans doublon de casse), avec leur nombre de clips. */
  get collections(): { name: string; count: number }[] {
    const m = new Map<string, { name: string; count: number }>();
    for (const c of this.ownClips) for (const t of c.tags) {
      const k = t.toLowerCase();
      const e = m.get(k);
      if (e) e.count++; else m.set(k, { name: t, count: 1 });
    }
    return [...m.values()].sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
  }

  removeClips(ids: string[]) {
    this.clips = this.clips.filter((c) => !ids.includes(c.id));
    if (this.openClip && ids.includes(this.openClip.id)) this.openClip = null;
  }
}

/** Couleur du texte posé sur l'accent (noir ou blanc selon la luminosité). */
function inkFor(hex: string): string {
  const h = hex.replace("#", "");
  if (h.length < 6) return "#0b0c0f";
  const r = parseInt(h.slice(0, 2), 16), g = parseInt(h.slice(2, 4), 16), b = parseInt(h.slice(4, 6), 16);
  const l = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return l > 0.6 ? "#0b0c0f" : "#ffffff";
}

export const app = new AppState();
