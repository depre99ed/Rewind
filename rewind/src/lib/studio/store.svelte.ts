// Studio v2 : état du projet ouvert, actions d'édition avec annuler / rétablir, projets enregistrés.
import { api, type Clip, type Filmstrip, type InputEvent, type MediaInfo, type Waveform } from "../api";
import { app } from "../state.svelte";
import { t } from "../i18n.svelte";
import {
  type AudioItem, type Format, type Kf, type Layer, type Piece, type Project, type Track, type Transition, type TextStyle, type VideoLayer,
  clampSpeed, defaultKeys, frameSize, fromLegacy, layout, newAudio, newBlur, newMediaLayer, newProject, newText,
  normalizePiece, outLen, projectTotal, srcAt, uid,
} from "./model";

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

export function fmtTime(s: number, precise = false): string {
  if (!isFinite(s) || s < 0) s = 0;
  const m = Math.floor(s / 60);
  const sec = s - m * 60;
  if (precise) return `${m}:${sec.toFixed(2).padStart(5, "0")}`;
  return `${m}:${Math.floor(sec).toString().padStart(2, "0")}`;
}

const IMG = ["png", "jpg", "jpeg", "webp", "bmp"];
const VID = ["mp4", "mkv", "mov", "webm", "avi"];
const AUD = ["mp3", "wav", "ogg", "m4a", "aac", "flac", "opus", "wma"];
export const extOf = (p: string) => (p.split(".").pop() ?? "").toLowerCase();
export const baseName = (p: string) => (p.split(/[\\/]/).pop() ?? "").replace(/\.[^.]+$/, "");

// ---------------------------------------------------------------------------
// Infos des médias (pistes, vignettes, forme d'onde, touches), chargées une fois par fichier
// ---------------------------------------------------------------------------
class MediaCache {
  info = $state.raw<Record<string, MediaInfo>>({});
  strips = $state.raw<Record<string, Filmstrip>>({});
  waves = $state.raw<Record<string, Waveform>>({});
  inputs = $state.raw<Record<string, InputEvent[]>>({});
  labels = $state.raw<Record<string, string>>({});
  private asked = new Set<string>();

  loadInputs(clipId: string) {
    const key = "inputs:" + clipId;
    if (!clipId || this.asked.has(key)) return;
    this.asked.add(key);
    api.clipInputs(clipId).then((l) => (this.inputs = { ...this.inputs, [clipId]: l ?? [] })).catch(() => {});
    if (!this.asked.has("labels")) {
      this.asked.add("labels");
      api.keyLabels().then((l) => (this.labels = l ?? {})).catch(() => {});
    }
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
    const tr = this.info[path]?.tracks ?? [];
    return tr.length > 1 ? tr.slice(1) : [];
  }
}
export const media = new MediaCache();

export type Sel =
  | { type: "piece"; id: string }
  | { type: "layer"; id: string }
  | { type: "audio"; id: string }
  | { type: "keys" }
  | null;

export type ProjectMeta = { id: string; name: string; updated: number; duration: number; thumb: string; pieces: number };

const LEGACY_KEY = "rewind.studio.v2";
const LAST_KEY = "rewind.studio.last";

class StudioStore {
  project = $state<Project>(newProject());
  /** Tête de lecture (secondes du montage) */
  t = $state(0);
  playing = $state(false);
  sel = $state<Sel>(null);
  /** Jonction choisie (index du morceau qui suit la transition) */
  junction = $state<number | null>(null);
  zoom = $state(1);
  snap = $state(true);
  hint = $state("");
  canUndo = $state(false);
  canRedo = $state(false);
  saved = $state(true);
  loaded = $state(false);
  projects = $state<ProjectMeta[]>([]);
  /** Enregistrement de voix off en cours (heure de départ dans le montage) */
  recording = $state<{ at: number; since: number } | null>(null);

  lay = $derived(layout(this.project.pieces));
  total = $derived(projectTotal(this.project, this.lay));
  frame = $derived(frameSize(this.project.format));

  private history: string[] = [];
  private future: string[] = [];
  private hintTimer: ReturnType<typeof setTimeout> | undefined;
  private saveTimer: ReturnType<typeof setTimeout> | undefined;

  get pieces() {
    return this.project.pieces;
  }

  // ----- sélection -----
  get piece(): Piece | null {
    const s = this.sel;
    return s?.type === "piece" ? this.project.pieces.find((p) => p.uid === s.id) ?? null : null;
  }
  get pieceIndex(): number {
    const s = this.sel;
    return s?.type === "piece" ? this.project.pieces.findIndex((p) => p.uid === s.id) : -1;
  }
  get layer(): Layer | null {
    const s = this.sel;
    if (s?.type !== "layer") return null;
    for (const tr of this.project.tracks) if (tr.kind === "layer") for (const l of tr.items) if (l.uid === s.id) return l;
    return null;
  }
  get audio(): AudioItem | null {
    const s = this.sel;
    if (s?.type !== "audio") return null;
    for (const tr of this.project.tracks) if (tr.kind === "audio") for (const a of tr.items) if (a.uid === s.id) return a;
    return null;
  }
  trackOf(id: string): Track | null {
    return this.project.tracks.find((tr) => (tr.items as { uid: string }[]).some((i) => i.uid === id)) ?? null;
  }
  select(sel: Sel) {
    this.sel = sel;
    if (sel) this.junction = null;
  }

  say(text: string) {
    this.hint = t(text);
    clearTimeout(this.hintTimer);
    this.hintTimer = setTimeout(() => (this.hint = ""), 2600);
  }

  // ----- historique -----
  snapshot() {
    this.history.push(JSON.stringify(this.project));
    if (this.history.length > 150) this.history.shift();
    this.future = [];
    this.canUndo = true;
    this.canRedo = false;
  }
  private restore(s: string) {
    this.project = JSON.parse(s);
    const sel = this.sel;
    if (sel && sel.type !== "keys") {
      const id = sel.id;
      const alive = this.project.pieces.some((p) => p.uid === id) || this.project.tracks.some((tr) => (tr.items as { uid: string }[]).some((i) => i.uid === id));
      if (!alive) this.sel = null;
    }
    this.clampT();
    this.changed();
  }
  undo() {
    const prev = this.history.pop();
    if (!prev) return;
    this.future.push(JSON.stringify(this.project));
    this.restore(prev);
    this.canUndo = this.history.length > 0;
    this.canRedo = true;
    this.say("Action annulée");
  }
  redo() {
    const next = this.future.pop();
    if (!next) return;
    this.history.push(JSON.stringify(this.project));
    this.restore(next);
    this.canUndo = true;
    this.canRedo = this.future.length > 0;
    this.say("Action rétablie");
  }
  /** Modification : historique + sauvegarde automatique. */
  edit(f: (p: Project) => void, record = true) {
    if (record) this.snapshot();
    f(this.project);
    this.project = { ...this.project, updated: Date.now() / 1000 };
    this.changed();
  }

  // ----- projets -----
  private changed() {
    this.saved = false;
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.saveNow(), 700);
  }
  async saveNow() {
    clearTimeout(this.saveTimer);
    const p = this.project;
    // Un projet vide et jamais nommé n'est pas gardé (pas de liste remplie de « Montage » vides)
    if (!p.pieces.length && !p.tracks.length) {
      this.saved = true;
      return;
    }
    try {
      await api.projectSave(p.id, JSON.stringify(p));
      this.saved = true;
      try { localStorage.setItem(LAST_KEY, p.id); } catch {}
    } catch {
      this.saved = false;
    }
  }
  async refreshList() {
    this.projects = (await api.projectsList().catch(() => [])) ?? [];
  }
  /** Premier affichage : rouvre le dernier projet (ou reprend l'ancien montage d'avant la 0.3.0). */
  async init() {
    if (this.loaded) return;
    this.loaded = true;
    await this.refreshList();
    try {
      const old = localStorage.getItem(LEGACY_KEY);
      if (old) {
        const p = fromLegacy(JSON.parse(old));
        if (p) {
          await api.projectSave(p.id, JSON.stringify(p));
          await this.refreshList();
        }
        localStorage.removeItem(LEGACY_KEY);
      }
    } catch {}
    let last = "";
    try { last = localStorage.getItem(LAST_KEY) ?? ""; } catch {}
    const pick = this.projects.find((p) => p.id === last) ?? this.projects[0];
    if (pick && !this.project.pieces.length && !this.project.tracks.length) await this.open(pick.id);
  }
  async open(id: string) {
    await this.saveNow();
    try {
      const json = await api.projectLoad(id);
      const p = JSON.parse(json) as Project;
      p.pieces = (p.pieces ?? []).map(normalizePiece);
      p.tracks = p.tracks ?? [];
      p.keys = p.keys ?? defaultKeys(p.format ?? "16:9");
      this.project = p;
      this.resetView();
      try { localStorage.setItem(LAST_KEY, id); } catch {}
    } catch (e) {
      this.say(t("Projet illisible : {e}", { e: String(e) }));
    }
  }
  async create(name = "Montage") {
    await this.saveNow();
    this.project = newProject(name);
    this.resetView();
    await this.refreshList();
  }
  async remove(id: string) {
    await api.projectDelete(id).catch(() => {});
    if (this.project.id === id) {
      this.project = newProject();
      this.resetView();
    }
    await this.refreshList();
  }
  private resetView() {
    this.t = 0;
    this.playing = false;
    this.sel = null;
    this.junction = null;
    this.history = [];
    this.future = [];
    this.canUndo = this.canRedo = false;
    this.saved = true;
  }
  rename(name: string) {
    this.edit((p) => (p.name = name || "Montage"), false);
  }
  setFormat(f: Format) {
    if (this.project.format === f) return;
    this.edit((p) => {
      p.format = f;
      p.keys = { ...defaultKeys(f), on: p.keys.on };
    });
    this.say(t("Format {f}", { f }));
  }

  // ----- temps -----
  clampT() {
    this.t = Math.max(0, Math.min(this.t, Math.max(0, this.total - 0.001)));
  }
  seek(t: number) {
    this.t = Math.max(0, Math.min(t, Math.max(0, this.total - 0.001)));
  }
  /** Morceau affiché à l'instant t (pendant une transition : celui qui arrive). */
  indexAt(t: number): number {
    const { offsets } = this.lay;
    let k = -1;
    for (let i = 0; i < offsets.length; i++) if (offsets[i] <= t + 1e-6) k = i;
    if (k >= 0 && t > offsets[k] + this.lay.lens[k] + 1e-6) return -1; // après la fin de la piste principale
    return k;
  }
  /** Heure dans le clip d'origine pour le morceau i à l'instant t du montage. */
  srcTime(i: number, t: number): number {
    const p = this.project.pieces[i];
    if (!p) return 0;
    return srcAt(p, t - this.lay.offsets[i]);
  }
  /** Limites (coupes, débuts et fins de calques) pour sauter de l'une à l'autre. */
  cuts(): number[] {
    const out = new Set<number>([0]);
    this.lay.offsets.forEach((o, i) => {
      out.add(+o.toFixed(3));
      out.add(+(o + this.lay.lens[i]).toFixed(3));
    });
    for (const tr of this.project.tracks) for (const it of tr.items) {
      out.add(+it.at.toFixed(3));
      out.add(+(it.at + it.dur).toFixed(3));
    }
    return [...out].sort((a, b) => a - b);
  }
  jumpCut(dir: 1 | -1) {
    const c = this.cuts();
    const next = dir > 0 ? c.find((x) => x > this.t + 0.02) : [...c].reverse().find((x) => x < this.t - 0.02);
    if (next !== undefined) this.seek(next);
  }

  // ----- piste principale -----
  piecesFrom(clips: Clip[]): Piece[] {
    return clips
      .filter((c) => c.kind !== "screenshot" && c.duration > 0.1)
      .map((c) => normalizePiece({ clipId: c.id, path: c.path, title: c.title, game: c.game, thumb: c.thumb, srcDur: c.duration, start: 0, end: c.duration, markers: c.markers }));
  }
  insertClips(clips: Clip[], at = this.project.pieces.length) {
    const pieces = this.piecesFrom(clips);
    if (!pieces.length) return;
    this.edit((p) => {
      const list = [...p.pieces];
      list.splice(Math.max(0, Math.min(at, list.length)), 0, ...pieces);
      p.pieces = list;
      if (list.length === pieces.length && (p.name === "Montage" || !p.name)) p.name = pieces[0].title;
    });
    this.select({ type: "piece", id: pieces[pieces.length - 1].uid });
    for (const pc of pieces) media.load(pc.path);
    this.say(pieces.length > 1 ? t("{n} clips ajoutés", { n: pieces.length }) : t("« {x} » ajouté", { x: pieces[0].title }));
  }
  updatePiece(id: string, patch: Partial<Piece>, record = true) {
    this.edit((p) => (p.pieces = p.pieces.map((x) => (x.uid === id ? { ...x, ...patch } : x))), record);
  }
  movePiece(from: number, to: number) {
    if (from === to || from < 0 || from >= this.project.pieces.length) return;
    this.edit((p) => {
      const list = [...p.pieces];
      const [x] = list.splice(from, 1);
      list.splice(Math.max(0, Math.min(to, list.length)), 0, x);
      p.pieces = list;
    });
    this.say("Morceau déplacé");
  }
  setSpeed(v: number, id = this.piece?.uid) {
    if (!id) return;
    this.updatePiece(id, { speed: clampSpeed(v) });
    this.say(t("Vitesse {v}×", { v }));
  }
  setTransition(k: number, type: Transition, dur?: number) {
    const p = this.project.pieces[k - 1];
    if (!p || k >= this.project.pieces.length) return;
    this.updatePiece(p.uid, { transition: type, transitionDur: dur ?? p.transitionDur ?? 0.5 });
    this.say(type === "none" ? t("Transition retirée") : t("Transition « {x} »", { x: t(TRANSITIONS.find((x) => x.id === type)?.label ?? "") }));
  }
  setTransitionAll(type: Transition) {
    if (this.project.pieces.length < 2) return;
    this.edit((p) => (p.pieces = p.pieces.map((x, i) => (i < p.pieces.length - 1 ? { ...x, transition: type } : x))));
    this.say(type === "none" ? "Transitions retirées" : "Transition appliquée partout");
  }
  /** Arrêt sur image : coupe le morceau à la tête de lecture et y insère l'image figée (2 s). */
  freezeFrame(hold = 2) {
    const i = this.indexAt(this.t);
    const p = this.project.pieces[i];
    if (!p || p.kind === "freeze") return;
    const cut = this.srcTime(i, this.t);
    const frozen = normalizePiece({ ...p, uid: uid(), kind: "freeze", start: cut, end: Math.min(p.srcDur, cut + 0.05), hold, ramp: null, zoom: [], transition: "none" });
    this.edit((pr) => {
      const list = [...pr.pieces];
      if (cut - p.start < 0.1) list.splice(i, 0, frozen);
      else if (p.end - cut < 0.1) list.splice(i + 1, 0, frozen);
      else {
        const a: Piece = { ...p, end: cut, transition: "none", ramp: rampIn(p, p.start, cut) };
        const b: Piece = { ...p, uid: uid(), start: cut, ramp: rampIn(p, cut, p.end) };
        list.splice(i, 1, a, frozen, b);
      }
      pr.pieces = list;
    });
    this.select({ type: "piece", id: frozen.uid });
    this.say("Arrêt sur image ajouté");
  }

  // ----- couper / supprimer / dupliquer (élément choisi, sinon morceau sous la tête) -----
  split() {
    const s = this.sel;
    if (s?.type === "layer" || s?.type === "audio") return this.splitItem(s.id);
    const i = this.indexAt(this.t);
    const p = this.project.pieces[i];
    if (!p) return;
    if (p.kind === "freeze") {
      const local = this.t - this.lay.offsets[i];
      if (local < 0.1 || p.hold - local < 0.1) return this.say("Trop près du bord pour couper");
      this.edit((pr) => pr.pieces.splice(i, 1, { ...p, hold: local, transition: "none" }, { ...p, uid: uid(), hold: p.hold - local }));
      return this.say("Morceau coupé en deux");
    }
    const cut = this.srcTime(i, this.t);
    if (cut - p.start < 0.1 || p.end - cut < 0.1) return this.say("Trop près du bord pour couper");
    const local = this.t - this.lay.offsets[i];
    const a: Piece = { ...p, end: cut, transition: "none", ramp: rampIn(p, p.start, cut), zoom: p.zoom.filter((k) => k.t <= local) };
    const b: Piece = { ...p, uid: uid(), start: cut, ramp: rampIn(p, cut, p.end), zoom: p.zoom.filter((k) => k.t >= local).map((k) => ({ ...k, t: k.t - local })) };
    this.edit((pr) => pr.pieces.splice(i, 1, a, b));
    this.select({ type: "piece", id: b.uid });
    this.say("Morceau coupé en deux");
  }
  private splitItem(id: string) {
    const tr = this.trackOf(id);
    const it = (tr?.items as (Layer | AudioItem)[] | undefined)?.find((x) => x.uid === id);
    if (!tr || !it) return;
    const local = this.t - it.at;
    if (local < 0.1 || it.dur - local < 0.1) return this.say("Place la tête de lecture sur l'élément pour le couper");
    const a = { ...it, dur: local, kf: it.kf.filter((k) => k.t <= local), fadeOut: 0 };
    const b: any = { ...it, uid: uid(), at: this.t, dur: it.dur - local, kf: it.kf.filter((k) => k.t >= local).map((k) => ({ ...k, t: k.t - local })), fadeIn: 0 };
    if ("srcStart" in it) b.srcStart = it.srcStart + local * ("speed" in it ? clampSpeed((it as any).speed) : 1);
    this.edit(() => {
      const items = tr.items as (Layer | AudioItem)[];
      items.splice(items.indexOf(it), 1, a as any, b);
    });
    this.select({ type: tr.kind === "audio" ? "audio" : "layer", id: b.uid });
    this.say("Élément coupé en deux");
  }
  removeSel() {
    const s = this.sel;
    if (!s) return;
    if (s.type === "keys") {
      this.edit((p) => (p.keys = { ...p.keys, on: false }));
      this.sel = null;
      return this.say("Touches masquées");
    }
    if (s.type === "piece") {
      const i = this.project.pieces.findIndex((p) => p.uid === s.id);
      if (i < 0) return;
      this.edit((p) => (p.pieces = p.pieces.filter((x) => x.uid !== s.id)));
      const next = this.project.pieces[Math.min(i, this.project.pieces.length - 1)];
      this.sel = next ? { type: "piece", id: next.uid } : null;
      this.clampT();
      return this.say("Morceau supprimé · Ctrl+Z pour annuler");
    }
    this.edit((p) => {
      for (const tr of p.tracks) (tr as any).items = (tr.items as { uid: string }[]).filter((i) => i.uid !== s.id);
      // Pistes vidées : retirées
      p.tracks = p.tracks.filter((tr) => tr.items.length);
    });
    this.sel = null;
    this.clampT();
    this.say("Élément supprimé · Ctrl+Z pour annuler");
  }
  duplicate() {
    const s = this.sel;
    if (!s || s.type === "keys") return;
    if (s.type === "piece") {
      const i = this.project.pieces.findIndex((p) => p.uid === s.id);
      if (i < 0) return;
      const copy = { ...this.project.pieces[i], uid: uid() };
      this.edit((p) => p.pieces.splice(i + 1, 0, copy));
      this.select({ type: "piece", id: copy.uid });
      return this.say("Morceau dupliqué");
    }
    const tr = this.trackOf(s.id);
    const it = (tr?.items as (Layer | AudioItem)[] | undefined)?.find((x) => x.uid === s.id);
    if (!tr || !it) return;
    const copy = { ...structuredClone($state.snapshot(it)), uid: uid(), at: it.at + it.dur };
    this.edit(() => {
      const target = this.freeTrack(tr.kind, copy.at, copy.dur, tr.id);
      (target.items as any[]).push(copy);
    });
    this.select({ type: tr.kind === "audio" ? "audio" : "layer", id: copy.uid });
    this.say("Élément dupliqué");
  }
  clear() {
    if (!this.project.pieces.length && !this.project.tracks.length) return;
    this.edit((p) => {
      p.pieces = [];
      p.tracks = [];
    });
    this.sel = null;
    this.t = 0;
    this.say("Montage vidé · Ctrl+Z pour annuler");
  }

  // ----- pistes -----
  /** Piste du bon type libre sur [at, at+dur] (la préférée d'abord), sinon une nouvelle (en haut pour les calques). */
  freeTrack(kind: "layer" | "audio", at: number, dur: number, prefer?: string, name?: string): Track {
    const free = (tr: Track) => tr.kind === kind && !tr.locked && (tr.items as { at: number; dur: number }[]).every((i) => i.at + i.dur <= at + 1e-3 || i.at >= at + dur - 1e-3);
    const pref = this.project.tracks.find((tr) => tr.id === prefer);
    if (pref && free(pref)) return pref;
    const found = (kind === "layer" ? this.project.tracks : [...this.project.tracks].reverse()).find(free);
    if (found && (!name || found.name === name)) return found;
    const n = this.project.tracks.filter((x) => x.kind === kind).length + 1;
    const tr: Track = kind === "layer"
      ? { id: uid(), kind: "layer", name: name ?? t("Calque {n}", { n }), hidden: false, locked: false, items: [] }
      : { id: uid(), kind: "audio", name: name ?? t("Son {n}", { n }), muted: false, locked: false, items: [] };
    // Calques : nouvelle piste tout en haut ; son : tout en bas
    if (kind === "layer") this.project.tracks.unshift(tr);
    else this.project.tracks.push(tr);
    return this.project.tracks.find((x) => x.id === tr.id)!;
  }
  moveTrack(id: string, dir: -1 | 1) {
    const i = this.project.tracks.findIndex((tr) => tr.id === id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= this.project.tracks.length) return;
    this.edit((p) => {
      const list = [...p.tracks];
      [list[i], list[j]] = [list[j], list[i]];
      p.tracks = list;
    });
    this.say(dir < 0 ? "Piste montée : affichée par-dessus" : "Piste descendue : affichée dessous");
  }
  toggleTrack(id: string, key: "hidden" | "muted" | "locked") {
    this.edit((p) => (p.tracks = p.tracks.map((tr) => (tr.id === id ? ({ ...tr, [key]: !(tr as any)[key] } as Track) : tr))), false);
  }
  /** Déplace un calque / son dans le temps et éventuellement sur une autre piste. */
  moveItem(id: string, at: number, trackId?: string, record = true) {
    this.edit((p) => {
      const from = p.tracks.find((tr) => (tr.items as { uid: string }[]).some((i) => i.uid === id));
      if (!from) return;
      const it = (from.items as (Layer | AudioItem)[]).find((i) => i.uid === id)!;
      it.at = Math.max(0, at);
      if (trackId && trackId !== from.id) {
        const to = p.tracks.find((tr) => tr.id === trackId);
        if (to && to.kind === from.kind && !to.locked) {
          (from as any).items = (from.items as any[]).filter((i) => i.uid !== id);
          (to.items as any[]).push(it);
          p.tracks = p.tracks.filter((tr) => tr.items.length);
        }
      }
    }, record);
  }

  // ----- passer d'une piste à l'autre (vidéo principale ↔ calques, nouvelle piste) -----
  /** Un morceau de la piste vidéo monte dans une piste de calques : il devient une vidéo par-dessus. */
  pieceToLayer(pieceId: string, at: number, trackId: string | null) {
    const i = this.project.pieces.findIndex((p) => p.uid === pieceId);
    const p = this.project.pieces[i];
    if (!p || p.kind === "freeze") return this.say("Une image figée reste sur la piste vidéo");
    const info = media.info[p.path];
    const l = newMediaLayer("video", p.path, p.title, info?.width || 1920, info?.height || 1080, Math.max(0, at), outLen(p), this.frame) as VideoLayer;
    l.srcStart = p.start;
    l.srcDur = p.srcDur;
    l.speed = clampSpeed(p.speed);
    l.volume = p.volume;
    this.edit((pr) => {
      pr.pieces = pr.pieces.filter((x) => x.uid !== pieceId);
      const tr = trackId ? this.freeTrack("layer", l.at, l.dur, trackId) : this.newTrack("layer", 0);
      (tr.items as Layer[]).push(l);
    });
    this.select({ type: "layer", id: l.uid });
    this.say("Clip passé par-dessus la vidéo");
  }
  /** Une vidéo par-dessus redescend sur la piste vidéo principale, à la place `index`. */
  layerToPiece(layerId: string, index: number) {
    const l = this.findLayer(layerId);
    if (!l || l.kind !== "video") return this.say("Seules les vidéos peuvent aller sur la piste vidéo");
    const clip = app.clips.find((c) => c.path === l.path);
    const piece = normalizePiece({
      clipId: clip?.id ?? "", path: l.path, title: l.name, game: clip?.game ?? "", thumb: clip?.thumb ?? "", srcDur: l.srcDur,
      start: l.srcStart, end: Math.min(l.srcDur, l.srcStart + l.dur * clampSpeed(l.speed)), speed: l.speed, volume: l.volume, markers: clip?.markers ?? [],
    });
    this.edit((pr) => {
      for (const tr of pr.tracks) if (tr.kind === "layer") tr.items = tr.items.filter((x) => x.uid !== layerId);
      pr.tracks = pr.tracks.filter((tr) => tr.items.length);
      pr.pieces.splice(Math.max(0, Math.min(index, pr.pieces.length)), 0, piece);
    });
    this.select({ type: "piece", id: piece.uid });
    this.say("Clip remis sur la piste vidéo");
  }
  /** Nouvelle piste vide à la position donnée (dans la liste de toutes les pistes). */
  newTrack(kind: "layer" | "audio", index: number): Track {
    const n = this.project.tracks.filter((x) => x.kind === kind).length + 1;
    const tr: Track = kind === "layer"
      ? { id: uid(), kind: "layer", name: t("Calque {n}", { n }), hidden: false, locked: false, items: [] }
      : { id: uid(), kind: "audio", name: t("Son {n}", { n }), muted: false, locked: false, items: [] };
    this.project.tracks.splice(Math.max(0, Math.min(index, this.project.tracks.length)), 0, tr);
    return this.project.tracks.find((x) => x.id === tr.id)!;
  }
  /** Déplace un calque ou un son sur une nouvelle piste (en haut des calques, ou en bas du son). */
  moveToNewTrack(id: string, at: number) {
    const from = this.trackOf(id);
    if (!from) return;
    this.edit((pr) => {
      const src = pr.tracks.find((tr) => tr.id === from.id)!;
      const it = (src.items as (Layer | AudioItem)[]).find((x) => x.uid === id)!;
      (src as any).items = (src.items as any[]).filter((x) => x.uid !== id);
      it.at = Math.max(0, at);
      const tr = this.newTrack(src.kind, src.kind === "layer" ? 0 : pr.tracks.length);
      (tr.items as any[]).push(it);
      pr.tracks = pr.tracks.filter((x) => x.items.length);
    });
    this.say("Nouvelle piste créée");
  }

  // ----- calques -----
  updateLayer(id: string, patch: Partial<Layer>, record = true) {
    this.edit((p) => {
      for (const tr of p.tracks) if (tr.kind === "layer") tr.items = tr.items.map((l) => (l.uid === id ? ({ ...l, ...patch } as Layer) : l));
    }, record);
  }
  /**
   * Change une propriété animable d'un calque. Sans point clé : la valeur de base change. Avec des points
   * clés : celui sous la tête de lecture est mis à jour, sinon un nouveau est créé (comme CapCut).
   */
  setAnimated(id: string, props: Record<string, number>, record = true) {
    const l = this.findLayer(id);
    if (!l) return;
    if (!l.kf.length) return this.updateLayer(id, props as Partial<Layer>, record);
    const local = Math.max(0, Math.min(l.dur, this.t - l.at));
    this.updateLayer(id, { kf: upsertKf(l.kf, local, props, (p) => (l as any)[p], l) }, record);
  }
  /** Pose ou retire un point clé à la tête de lecture (toutes les valeurs du moment). */
  toggleKf(id: string) {
    const l = this.findLayer(id);
    if (!l) return;
    const local = Math.max(0, Math.min(l.dur, this.t - l.at));
    const near = l.kf.findIndex((k) => Math.abs(k.t - local) < 0.04);
    if (near >= 0) {
      this.updateLayer(id, { kf: l.kf.filter((_, i) => i !== near) });
      return this.say("Point clé retiré");
    }
    const st = { x: l.x, y: l.y, scale: l.scale, rot: l.rot, opacity: l.opacity };
    const cur: Record<string, number> = {};
    for (const k of Object.keys(st)) cur[k] = kfAt(l.kf, k, local, (st as any)[k]);
    this.updateLayer(id, { kf: [...l.kf, { t: local, ...cur }].sort((a, b) => a.t - b.t) });
    this.say("Point clé posé : change la position ou la taille à un autre moment");
  }
  findLayer(id: string): Layer | null {
    for (const tr of this.project.tracks) if (tr.kind === "layer") for (const l of tr.items) if (l.uid === id) return l;
    return null;
  }
  private addLayer(l: Layer, prefer?: string) {
    this.edit(() => {
      const tr = prefer === "new" ? this.newTrack("layer", 0) : this.freeTrack("layer", l.at, l.dur, prefer);
      (tr.items as Layer[]).push(l);
    });
    this.select({ type: "layer", id: l.uid });
  }
  addText(style: TextStyle) {
    const l = newText(this.t, style);
    l.text = t(style === "title" ? "TITRE" : style === "subtitle" ? "Sous-titre" : style === "gothic" ? "Edit" : "Légende");
    this.addLayer(l);
    this.say("Texte ajouté : écris-le à droite, place-le sur l'aperçu");
  }
  addBlur() {
    this.addLayer(newBlur(this.t, Math.max(2, Math.min(5, this.total - this.t || 5))));
    this.say("Zone floutée ajoutée : place-la sur l'aperçu");
  }
  /** Image, GIF ou vidéo par-dessus (taille de départ raisonnable, à droite en haut). */
  async addMedia(path: string, opts: { at?: number; track?: string; name?: string } = {}) {
    const ext = extOf(path);
    const name = opts.name ?? baseName(path);
    const at = Math.max(0, opts.at ?? this.t);
    try {
      if (VID.includes(ext)) {
        const info = await api.mediaInfo(path);
        const dur = Math.max(0.5, info.duration);
        const l = newMediaLayer("video", path, name, info.width || 1920, info.height || 1080, at, dur, this.frame) as any;
        l.srcDur = dur;
        this.addLayer(l, opts.track);
        media.load(path);
      } else if (IMG.includes(ext) || ext === "gif") {
        const { w, h } = await imageSize(path);
        this.addLayer(newMediaLayer(ext === "gif" ? "gif" : "image", path, name, w, h, at, 4, this.frame), opts.track);
      } else if (AUD.includes(ext)) {
        return this.addAudio(path, "sfx");
      } else return this.say("Format non pris en charge");
      this.say(t("« {x} » ajouté par-dessus la vidéo", { x: name }));
    } catch (e) {
      this.say(t("Fichier illisible : {e}", { e: String(e) }));
    }
  }

  // ----- son -----
  async addAudio(path: string, kind: AudioItem["kind"], at = kind === "music" ? 0 : this.t) {
    try {
      const info = await api.mediaInfo(path);
      if (!info?.tracks.length) return this.say("Ce fichier n'a pas de son");
      const name = baseName(path);
      const item = newAudio(kind, path, name, Math.max(0.2, info.duration), at, Math.max(this.total, 1));
      const label = kind === "music" ? t("Musique") : kind === "voice" ? t("Voix off") : t("Bruitages");
      this.edit(() => {
        const tr = this.freeTrack("audio", item.at, item.dur, undefined, label);
        (tr.items as AudioItem[]).push(item);
      });
      this.select({ type: "audio", id: item.uid });
      media.load(path);
      this.say(t("« {x} » ajouté", { x: name }));
    } catch (e) {
      this.say(t("Son illisible : {e}", { e: String(e) }));
    }
  }
  updateAudio(id: string, patch: Partial<AudioItem>, record = true) {
    this.edit((p) => {
      for (const tr of p.tracks) if (tr.kind === "audio") tr.items = tr.items.map((a) => (a.uid === id ? { ...a, ...patch } : a));
    }, record);
  }
  /** Volume dans le temps : point clé de volume à la tête de lecture. */
  setVolumeAt(id: string, v: number, record = true) {
    const a = this.audio?.uid === id ? this.audio : null;
    if (!a) return;
    if (!a.kf.length) return this.updateAudio(id, { volume: v }, record);
    const local = Math.max(0, Math.min(a.dur, this.t - a.at));
    this.updateAudio(id, { kf: upsertKf(a.kf, local, { v }, () => a.volume, null) }, record);
  }
  toggleVolumeKf(id: string) {
    const a = this.audio;
    if (!a || a.uid !== id) return;
    const local = Math.max(0, Math.min(a.dur, this.t - a.at));
    const near = a.kf.findIndex((k) => Math.abs(k.t - local) < 0.04);
    if (near >= 0) return this.updateAudio(id, { kf: a.kf.filter((_, i) => i !== near) });
    this.updateAudio(id, { kf: [...a.kf, { t: local, v: kfAt(a.kf, "v", local, a.volume) }].sort((x, y) => x.t - y.t) });
    this.say("Point de volume posé : change le volume à un autre moment");
  }

  // ----- zoom animé (morceau) -----
  toggleZoomKf(id: string) {
    const i = this.project.pieces.findIndex((p) => p.uid === id);
    const p = this.project.pieces[i];
    if (!p) return;
    const local = Math.max(0, Math.min(outLen(p), this.t - this.lay.offsets[i]));
    const near = p.zoom.findIndex((k) => Math.abs(k.t - local) < 0.04);
    if (near >= 0) return this.updatePiece(id, { zoom: p.zoom.filter((_, j) => j !== near) });
    const cur = { z: kfAt(p.zoom, "z", local, 1), x: kfAt(p.zoom, "x", local, 0.5), y: kfAt(p.zoom, "y", local, 0.5) };
    this.updatePiece(id, { zoom: [...p.zoom, { t: local, ...cur }].sort((a, b) => a.t - b.t) });
    this.say("Point de zoom posé : avance la lecture et change le zoom");
  }
  setZoomAt(id: string, props: { z?: number; x?: number; y?: number }, record = true) {
    const i = this.project.pieces.findIndex((p) => p.uid === id);
    const p = this.project.pieces[i];
    if (!p) return;
    const local = Math.max(0, Math.min(outLen(p), this.t - this.lay.offsets[i]));
    const base = { z: 1, x: 0.5, y: 0.5 } as Record<string, number>;
    const list = p.zoom.length ? p.zoom : [{ t: 0, z: 1, x: 0.5, y: 0.5 }];
    this.updatePiece(id, { zoom: upsertKf(list, local, props as Record<string, number>, (k) => base[k], null) }, record);
  }

  // ----- touches -----
  setKeys(patch: Partial<Project["keys"]>, record = true) {
    this.edit((p) => (p.keys = { ...p.keys, ...patch }), record);
  }
}

/** Rampe gardée si elle tombe dans [a, b] après une coupe. */
function rampIn(p: Piece, a: number, b: number) {
  return p.ramp && p.ramp.at > a && p.ramp.at < b ? p.ramp : null;
}

function kfAt(kf: Kf[], prop: string, t: number, fallback: number) {
  const list = kf.filter((k) => typeof k[prop] === "number");
  if (!list.length) return fallback;
  if (t <= list[0].t) return list[0][prop];
  const last = list[list.length - 1];
  if (t >= last.t) return last[prop];
  for (let i = 1; i < list.length; i++) {
    const a = list[i - 1], b = list[i];
    if (t <= b.t) {
      const p = (t - a.t) / Math.max(1e-6, b.t - a.t);
      return a[prop] + (b[prop] - a[prop]) * (p * p * (3 - 2 * p));
    }
  }
  return last[prop];
}

/** Met à jour le point clé sous t (± 1 image) ou en crée un avec toutes les valeurs du moment. */
function upsertKf(kf: Kf[], t: number, props: Record<string, number>, base: (p: string) => number, _l: unknown): Kf[] {
  const near = kf.findIndex((k) => Math.abs(k.t - t) < 0.04);
  if (near >= 0) return kf.map((k, i) => (i === near ? { ...k, ...props } : k));
  const keys = new Set<string>(kf.flatMap((k) => Object.keys(k).filter((x) => x !== "t")));
  const cur: Record<string, number> = {};
  for (const k of keys) cur[k] = kfAt(kf, k, t, base(k));
  return [...kf, { t, ...cur, ...props }].sort((a, b) => a.t - b.t);
}

function imageSize(path: string): Promise<{ w: number; h: number }> {
  return new Promise((res, rej) => {
    const img = new Image();
    img.onload = () => res({ w: img.naturalWidth, h: img.naturalHeight });
    img.onerror = () => rej("image illisible");
    import("../api").then(({ fileUrl }) => (img.src = fileUrl(path)));
  });
}

export const studio = new StudioStore();

/** Ouvre le Studio avec ces clips : à la suite du projet ouvert s'il est vide, sinon dans un nouveau projet. */
export async function openInStudio(clips: Clip[]) {
  await studio.init();
  if (studio.project.pieces.length || studio.project.tracks.length) await studio.create(clips[0]?.title ?? "Montage");
  studio.insertClips(clips);
}

export { outLen, uid };
