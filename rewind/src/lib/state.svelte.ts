import { listen } from "@tauri-apps/api/event";
import { api, type Clip, type Settings, type Status, type Cs2Event, type Moment, type Marker, type UpdateInfo } from "./api";
import { t } from "./i18n.svelte";

export type Transition = "none" | "fade" | "black" | "white" | "zoom" | "slide" | "wipe";

export type StudioPiece = {
  uid: string;
  clipId: string;
  path: string;
  title: string;
  game: string;
  thumb: string;
  srcDur: number;
  start: number;
  end: number;
  speed: number;
  /** Volume général (1 = 100 %, jusqu'à 2) */
  volume: number;
  /** Volume de chaque piste séparée (son du PC, micro, Discord…) ; vide = piste de mixage */
  trackVolumes: number[];
  /** Transition vers le morceau suivant */
  transition: Transition;
  transitionDur: number;
  /** Repères du clip d'origine (kills…), en temps du clip */
  markers: Marker[];
};

export type StudioMusic = {
  path: string;
  name: string;
  duration: number;
  volume: number;
  /** Début dans le fichier de musique */
  offset: number;
  /** Position de départ dans le montage */
  at: number;
  fadeIn: number;
  fadeOut: number;
  /** Baisse la musique quand le jeu ou la voix sont forts */
  duck: boolean;
};

export type StudioFormat = "16:9" | "9:16-blur" | "9:16-crop";

const STUDIO_KEY = "rewind.studio.v2";

/** Complète un morceau venu d'une ancienne version (champs manquants). */
export function normalizePiece(p: Partial<StudioPiece> & { path: string }): StudioPiece {
  return {
    uid: p.uid ?? Math.random().toString(36).slice(2),
    clipId: p.clipId ?? "",
    path: p.path,
    title: p.title ?? "Clip",
    game: p.game ?? "",
    thumb: p.thumb ?? "",
    srcDur: p.srcDur ?? Math.max(p.end ?? 0, 0.1),
    start: p.start ?? 0,
    end: p.end ?? p.srcDur ?? 1,
    speed: p.speed ?? 1,
    volume: p.volume ?? 1,
    trackVolumes: p.trackVolumes ?? [],
    transition: p.transition ?? "none",
    transitionDur: p.transitionDur ?? 0.5,
    markers: p.markers ?? [],
  };
}

type Toast = { id: number; kind: "ok" | "err" | "info"; title: string; text?: string; clip?: Clip };

class AppState {
  settings = $state<Settings | null>(null);
  status = $state<Status | null>(null);
  clips = $state<Clip[]>([]);
  toasts = $state<Toast[]>([]);
  view = $state<"library" | "studio" | "cs2" | "settings">("library");
  settingsSection = $state("video");
  studio = $state<StudioPiece[]>([]);
  studioMusic = $state<StudioMusic | null>(null);
  studioTitle = $state("Montage");
  studioFormat = $state<StudioFormat>("16:9");
  /** Touches clavier/souris affichées dans l'aperçu et incrustées à l'export */
  studioKeys = $state(true);
  openClip = $state<Clip | null>(null);
  progress = $state<{ op: string; value: number } | null>(null);
  cs2Feed = $state<{ at: number; text: string; kind: string }[]>([]);
  cs2Map = $state<{ map: string; mode: string } | null>(null);
  clipping = $state(false);
  /** Dernier résultat de la recherche de mise à jour. */
  update = $state<UpdateInfo | null>(null);
  private toastId = 0;

  async init() {
    this.settings = await api.getSettings();
    this.applyLook();
    this.clips = await api.clips();
    this.restoreStudio();
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
    await listen("library-changed", async () => {
      this.clips = await api.clips();
    });
    await listen<{ op: string; value: number }>("edit-progress", (e) => {
      this.progress = e.payload.value >= 1 ? null : e.payload;
    });
    await listen<Cs2Event>("cs2-event", (e) => this.onCs2(e.payload));
    await listen<UpdateInfo>("update-available", (e) => (this.update = e.payload));
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

  private async poll() {
    try {
      this.status = await api.status();
    } catch {}
    setTimeout(() => this.poll(), 400);
  }

  toast(kind: Toast["kind"], title: string, text?: string, clip?: Clip) {
    const id = ++this.toastId;
    this.toasts = [...this.toasts, { id, kind, title, text, clip }].slice(-4);
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

  /** Transforme des clips en morceaux de montage (clip entier, réglages par défaut). */
  piecesFrom(clips: Clip[]): StudioPiece[] {
    return clips
      .filter((c) => c.kind !== "screenshot" && c.duration > 0.2)
      .map((c) =>
        normalizePiece({
          clipId: c.id, path: c.path, title: c.title, game: c.game, thumb: c.thumb,
          srcDur: c.duration, start: 0, end: c.duration, markers: c.markers,
        }),
      );
  }

  /** Ajoute des clips au Studio (morceaux entiers) et y va. */
  openStudio(clips: Clip[], replace = false) {
    const pieces = this.piecesFrom(clips);
    this.studio = replace ? pieces : [...this.studio, ...pieces];
    if (replace || this.studio.length === pieces.length) this.studioTitle = pieces[0]?.title ?? "Montage";
    this.openClip = null;
    this.view = "studio";
  }

  /** Le montage en cours survit à la fermeture de l'appli (sauvegarde locale). */
  saveStudio() {
    try {
      localStorage.setItem(
        STUDIO_KEY,
        JSON.stringify({ pieces: this.studio, music: this.studioMusic, title: this.studioTitle, format: this.studioFormat }),
      );
    } catch {}
  }

  private restoreStudio() {
    try {
      const raw = localStorage.getItem(STUDIO_KEY);
      if (!raw) return;
      const s = JSON.parse(raw);
      if (Array.isArray(s.pieces)) this.studio = s.pieces.filter((p: any) => p && typeof p.path === "string").map(normalizePiece);
      if (s.music && typeof s.music.path === "string") this.studioMusic = s.music;
      if (typeof s.title === "string") this.studioTitle = s.title;
      if (["16:9", "9:16-blur", "9:16-crop"].includes(s.format)) this.studioFormat = s.format;
    } catch {}
  }

  updateClip(c: Clip) {
    this.clips = this.clips.map((x) => (x.id === c.id ? c : x));
    if (this.openClip?.id === c.id) this.openClip = c;
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
