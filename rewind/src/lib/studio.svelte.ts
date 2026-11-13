// Studio : état du montage, calcul de la frise (identique au moteur Rust, editor.rs) et actions
// d'édition avec annuler / rétablir. Le lecteur et la frise lisent tout ici.
import { app, normalizePiece, type StudioPiece, type StudioMusic, type Transition } from "./state.svelte";
import { api, type Clip, type Filmstrip, type InputEvent, type MediaInfo, type Waveform } from "./api";
import { t } from "./i18n.svelte";

export const TRANSITIONS: { id: Transition; label: string; hint: string }[] = [
  { id: "none", label: "Aucune", hint: "Coupe sèche" },
  { id: "fade", label: "Fondu", hint: "Les deux images se mélangent" },
  { id: "black", label: "Noir", hint: "Passage par le noir" },
  { id: "white", label: "Flash", hint: "Éclair blanc" },
  { id: "zoom", label: "Zoom", hint: "Plongée dans l'image" },
  { id: "slide", label: "Glisser", hint: "L'image suivante pousse la précédente" },
  { id: "wipe", label: "Balayage", hint: "Rideau de droite à gauche" },
];

export const SPEEDS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];

export const clampSpeed = (s: number) => Math.min(4, Math.max(0.25, s || 1));
/** Durée de sortie d'un morceau (après vitesse). */
export const outLen = (p: StudioPiece) => Math.max(0.05, p.end - p.start) / clampSpeed(p.speed);

export type Layout = {
  lens: number[];
  /** trans[k] = chevauchement entre le morceau k-1 et k (0 pour k = 0) */
  trans: number[];
  offsets: number[];
  total: number;
};

/** Même calcul que `transition_durations` / `total_duration` dans editor.rs. */
export function layout(pieces: StudioPiece[]): Layout {
  const lens = pieces.map(outLen);
  const trans = pieces.map(() => 0);
  for (let k = 1; k < pieces.length; k++) {
    const p = pieces[k - 1];
    if (!p.transition || p.transition === "none") continue;
    trans[k] = Math.min(Math.min(2, Math.max(0.1, p.transitionDur || 0.5)), lens[k - 1] * 0.5, lens[k] * 0.5);
  }
  const offsets: number[] = [];
  let acc = 0;
  for (let k = 0; k < pieces.length; k++) {
    if (k > 0) acc -= trans[k];
    offsets.push(acc);
    acc += lens[k];
  }
  return { lens, trans, offsets, total: pieces.length ? acc : 0 };
}

export function fmtTime(s: number, precise = false): string {
  if (!isFinite(s) || s < 0) s = 0;
  const m = Math.floor(s / 60);
  const sec = s - m * 60;
  if (precise) return `${m}:${sec.toFixed(2).padStart(5, "0")}`;
  return `${m}:${Math.floor(sec).toString().padStart(2, "0")}`;
}

// ---------------------------------------------------------------------------
// Infos des médias (pistes, vignettes, forme d'onde), chargées une fois par fichier
// ---------------------------------------------------------------------------
class MediaCache {
  info = $state.raw<Record<string, MediaInfo>>({});
  strips = $state.raw<Record<string, Filmstrip>>({});
  waves = $state.raw<Record<string, Waveform>>({});
  /** Touches clavier/souris enregistrées avec chaque clip (par identifiant de clip) */
  inputs = $state.raw<Record<string, InputEvent[]>>({});
  private asked = new Set<string>();

  loadInputs(clipId: string) {
    const key = "inputs:" + clipId;
    if (!clipId || this.asked.has(key)) return;
    this.asked.add(key);
    api.clipInputs(clipId).then((l) => (this.inputs = { ...this.inputs, [clipId]: l ?? [] })).catch(() => {});
  }

  load(path: string) {
    if (!path || this.asked.has(path)) return;
    this.asked.add(path);
    api.mediaInfo(path).then((i) => i && (this.info = { ...this.info, [path]: i })).catch(() => {});
    api.mediaWaveform(path).then((w) => w && (this.waves = { ...this.waves, [path]: w })).catch(() => {});
  }

  loadStrip(path: string) {
    const key = "strip:" + path;
    if (!path || this.asked.has(key)) return;
    this.asked.add(key);
    api.mediaFilmstrip(path).then((s) => s && (this.strips = { ...this.strips, [path]: s })).catch(() => {});
  }

  /** Pistes séparées d'un clip (sans la piste de mixage) ; vide si une seule piste. */
  separateTracks(path: string): string[] {
    const t = this.info[path]?.tracks ?? [];
    return t.length > 1 ? t.slice(1) : [];
  }
}
export const media = new MediaCache();

// ---------------------------------------------------------------------------
// Édition
// ---------------------------------------------------------------------------
const uid = () => Math.random().toString(36).slice(2);

class StudioState {
  /** Tête de lecture (secondes du montage) */
  t = $state(0);
  playing = $state(false);
  selected = $state<string | null>(null);
  musicSelected = $state(false);
  /** Jonction choisie (index du morceau qui suit la transition) */
  junction = $state<number | null>(null);
  zoom = $state(1);
  snap = $state(true);
  hint = $state("");
  canUndo = $state(false);
  canRedo = $state(false);
  lay = $derived(layout(app.studio));
  private history: string[] = [];
  private future: string[] = [];
  private hintTimer: ReturnType<typeof setTimeout> | undefined;

  get pieces() {
    return app.studio;
  }
  get sel(): StudioPiece | null {
    return app.studio.find((p) => p.uid === this.selected) ?? null;
  }
  get selIndex(): number {
    return app.studio.findIndex((p) => p.uid === this.selected);
  }

  /** Petit message de confirmation sous la frise (retour visuel de chaque action). */
  say(text: string) {
    this.hint = t(text);
    clearTimeout(this.hintTimer);
    this.hintTimer = setTimeout(() => (this.hint = ""), 2600);
  }

  // ----- historique -----
  private state() {
    return JSON.stringify({ p: app.studio, m: app.studioMusic });
  }
  private apply(s: string) {
    const o = JSON.parse(s);
    app.studio = o.p;
    app.studioMusic = o.m;
    if (this.selected && !app.studio.some((p) => p.uid === this.selected)) this.selected = null;
    this.clampT();
  }
  snapshot() {
    this.history.push(this.state());
    if (this.history.length > 120) this.history.shift();
    this.future = [];
    this.canUndo = true;
    this.canRedo = false;
  }
  undo() {
    const prev = this.history.pop();
    if (!prev) return;
    this.future.push(this.state());
    this.apply(prev);
    this.canUndo = this.history.length > 0;
    this.canRedo = true;
    this.say("Action annulée");
  }
  redo() {
    const next = this.future.pop();
    if (!next) return;
    this.history.push(this.state());
    this.apply(next);
    this.canUndo = true;
    this.canRedo = this.future.length > 0;
    this.say("Action rétablie");
  }

  // ----- temps -----
  clampT() {
    this.t = Math.max(0, Math.min(this.t, Math.max(0, this.lay.total - 0.001)));
  }
  seek(t: number) {
    this.t = Math.max(0, Math.min(t, Math.max(0, this.lay.total - 0.001)));
  }
  /** Morceau affiché à l'instant t (pendant une transition : celui qui arrive). */
  indexAt(t: number): number {
    const { offsets } = this.lay;
    let k = -1;
    for (let i = 0; i < offsets.length; i++) if (offsets[i] <= t + 1e-6) k = i;
    return k;
  }
  /** Heure dans le clip d'origine pour le morceau i à l'instant t du montage. */
  srcTime(i: number, t: number): number {
    const p = app.studio[i];
    if (!p) return 0;
    return Math.min(p.end, Math.max(p.start, p.start + (t - this.lay.offsets[i]) * clampSpeed(p.speed)));
  }
  /** Limites de coupe (début/fin des morceaux) pour sauter de l'une à l'autre. */
  cuts(): number[] {
    const { offsets, lens } = this.lay;
    const out = new Set<number>([0]);
    offsets.forEach((o, i) => {
      out.add(+o.toFixed(3));
      out.add(+(o + lens[i]).toFixed(3));
    });
    return [...out].sort((a, b) => a - b);
  }
  jumpCut(dir: 1 | -1) {
    const c = this.cuts();
    const next = dir > 0 ? c.find((x) => x > this.t + 0.02) : [...c].reverse().find((x) => x < this.t - 0.02);
    if (next !== undefined) this.seek(next);
  }

  // ----- actions -----
  select(uid: string | null) {
    this.selected = uid;
    this.musicSelected = false;
    if (uid) this.junction = null;
  }
  update(uid: string, patch: Partial<StudioPiece>, record = true) {
    if (record) this.snapshot();
    app.studio = app.studio.map((p) => (p.uid === uid ? { ...p, ...patch } : p));
  }
  insertClips(clips: Clip[], at = app.studio.length) {
    const pieces = app.piecesFrom(clips);
    if (!pieces.length) return;
    this.snapshot();
    const list = [...app.studio];
    list.splice(Math.max(0, Math.min(at, list.length)), 0, ...pieces);
    app.studio = list;
    if (list.length === pieces.length) app.studioTitle = pieces[0].title;
    this.select(pieces[pieces.length - 1].uid);
    for (const p of pieces) media.load(p.path);
    this.say(pieces.length > 1 ? t("{n} clips ajoutés", { n: pieces.length }) : t("« {x} » ajouté", { x: pieces[0].title }));
  }
  split() {
    const i = this.indexAt(this.t);
    const p = app.studio[i];
    if (!p) return;
    const cut = this.srcTime(i, this.t);
    if (cut - p.start < 0.1 || p.end - cut < 0.1) {
      this.say("Trop près du bord pour couper");
      return;
    }
    this.snapshot();
    const a: StudioPiece = { ...p, end: cut, transition: "none" };
    const b: StudioPiece = { ...p, uid: uid(), start: cut };
    app.studio = [...app.studio.slice(0, i), a, b, ...app.studio.slice(i + 1)];
    this.select(b.uid);
    this.say("Morceau coupé en deux");
  }
  remove(target = this.selected) {
    if (!target) return;
    const i = app.studio.findIndex((p) => p.uid === target);
    if (i < 0) return;
    this.snapshot();
    app.studio = app.studio.filter((p) => p.uid !== target);
    this.selected = app.studio[Math.min(i, app.studio.length - 1)]?.uid ?? null;
    this.clampT();
    this.say("Morceau supprimé · Ctrl+Z pour annuler");
  }
  duplicate(target = this.selected) {
    const i = app.studio.findIndex((p) => p.uid === target);
    if (i < 0) return;
    this.snapshot();
    const copy = { ...app.studio[i], uid: uid() };
    app.studio = [...app.studio.slice(0, i + 1), copy, ...app.studio.slice(i + 1)];
    this.select(copy.uid);
    this.say("Morceau dupliqué");
  }
  move(from: number, to: number) {
    if (from === to || from < 0 || from >= app.studio.length) return;
    this.snapshot();
    const list = [...app.studio];
    const [p] = list.splice(from, 1);
    list.splice(Math.max(0, Math.min(to, list.length)), 0, p);
    app.studio = list;
    this.say("Morceau déplacé");
  }
  setSpeed(v: number, target = this.selected) {
    if (!target) return;
    this.update(target, { speed: clampSpeed(v) });
    this.say(t("Vitesse {v}×", { v }));
  }
  /** Transition entre le morceau k-1 et le morceau k. */
  setTransition(k: number, type: Transition, dur?: number) {
    const p = app.studio[k - 1];
    if (!p || k >= app.studio.length) return;
    this.update(p.uid, { transition: type, transitionDur: dur ?? p.transitionDur ?? 0.5 });
    this.say(type === "none" ? t("Transition retirée") : t("Transition « {x} »", { x: t(TRANSITIONS.find((x) => x.id === type)?.label ?? "") }));
  }
  setTransitionAll(type: Transition) {
    if (app.studio.length < 2) return;
    this.snapshot();
    app.studio = app.studio.map((p, i) => (i < app.studio.length - 1 ? { ...p, transition: type } : p));
    this.say(type === "none" ? "Transitions retirées" : "Transition appliquée partout");
  }
  clear() {
    if (!app.studio.length && !app.studioMusic) return;
    this.snapshot();
    app.studio = [];
    app.studioMusic = null;
    this.selected = null;
    this.t = 0;
    this.say("Montage vidé · Ctrl+Z pour annuler");
  }

  // ----- musique -----
  setMusic(m: StudioMusic | null) {
    this.snapshot();
    app.studioMusic = m;
    this.musicSelected = !!m;
    if (m) {
      this.selected = null;
      media.load(m.path);
    }
    this.say(m ? t("Musique « {x} » ajoutée", { x: m.name }) : t("Musique retirée"));
  }
  updateMusic(patch: Partial<StudioMusic>, record = true) {
    if (!app.studioMusic) return;
    if (record) this.snapshot();
    app.studioMusic = { ...app.studioMusic, ...patch };
  }

  /** Ajoute le nombre de pistes séparées quand on les découvre (volume 100 % chacune). */
  fillTracks() {
    let changed = false;
    const list = app.studio.map((p) => {
      const n = media.separateTracks(p.path).length;
      if (n > 0 && p.trackVolumes.length !== n) {
        changed = true;
        return { ...p, trackVolumes: Array.from({ length: n }, (_, k) => p.trackVolumes[k] ?? 1) };
      }
      return p;
    });
    if (changed) app.studio = list;
  }
}

export const studio = new StudioState();

// ---------------------------------------------------------------------------
// Musique de fond : choix du fichier, musiques récentes
// ---------------------------------------------------------------------------
const RECENT_KEY = "rewind.music.recent";
export type RecentMusic = { path: string; name: string; duration: number };

export function recentMusic(): RecentMusic[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
    return Array.isArray(list) ? list.filter((m) => m && typeof m.path === "string") : [];
  } catch {
    return [];
  }
}
function rememberMusic(m: RecentMusic) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify([m, ...recentMusic().filter((x) => x.path !== m.path)].slice(0, 8)));
  } catch {}
}
export async function useMusic(path: string) {
  try {
    const info = await api.mediaInfo(path);
    if (!info?.tracks.length) {
      studio.say("Ce fichier n'a pas de son");
      return;
    }
    const name = (path.split(/[\\/]/).pop() ?? "Musique").replace(/\.[^.]+$/, "");
    rememberMusic({ path, name, duration: info.duration });
    // Par défaut : sous le son du jeu, avec fondus et atténuation automatique
    studio.setMusic({ path, name, duration: info.duration, volume: 0.5, offset: 0, at: 0, fadeIn: 1, fadeOut: 2, duck: true });
  } catch (e) {
    studio.say(t("Musique illisible : {e}", { e: String(e) }));
  }
}
export async function pickMusic() {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const path = await open({
    multiple: false,
    directory: false,
    title: t("Choisir une musique de fond"),
    filters: [{ name: t("Musique"), extensions: ["mp3", "wav", "ogg", "m4a", "aac", "flac", "opus", "wma"] }],
  }).catch(() => null);
  if (typeof path === "string") await useMusic(path);
}

/** Reprend des morceaux sauvegardés (anciens formats compris). */
export function restorePieces(list: unknown[]): StudioPiece[] {
  return list.filter((p): p is StudioPiece => !!p && typeof (p as StudioPiece).path === "string").map(normalizePiece);
}
