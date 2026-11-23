import { invoke as rawInvoke, convertFileSrc } from "@tauri-apps/api/core";
import { t, locale } from "./i18n.svelte";

/** Envoie une erreur de l'interface au journal (elle apparaîtra dans le rapport de diagnostic). */
const recent = new Map<string, number>();
export function logFront(level: "error" | "warn", message: string) {
  const now = Date.now();
  const last = recent.get(message);
  if (last && now - last < 60_000) return; // même message : une fois par minute
  if (recent.size > 200) recent.clear();
  recent.set(message, now);
  rawInvoke("log_front", { level, message }).catch(() => {});
}

/** Comme `invoke`, mais une commande en échec est aussi notée dans le journal. */
function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return rawInvoke<T>(cmd, args).catch((e) => {
    if (cmd !== "get_status" && cmd !== "log_front") logFront("warn", `commande ${cmd} : ${String(e)}`);
    throw e;
  });
}

export type Marker = { time: number; kind: string; label: string };

export type Clip = {
  id: string;
  path: string;
  thumb: string;
  title: string;
  game: string;
  created: number;
  duration: number;
  size: number;
  kind: "manual" | "auto" | "edit" | "screenshot" | string;
  markers: Marker[];
  favorite: boolean;
  tags: string[];
};

export type AudioSource = {
  id: string;
  kind: "system" | "mic" | "app" | string;
  device: string;
  app: string;
  label: string;
  enabled: boolean;
  volume: number;
};

export type EventRule = { enabled: boolean; before: number; after: number };

export type Settings = {
  launch_at_startup: boolean;
  start_minimized: boolean;
  close_to_tray: boolean;
  clip_folder: string;
  folder_per_game: boolean;
  recording_enabled: boolean;
  capture_mode: "monitor" | "window" | string;
  monitor: string;
  window_exe: string;
  buffer_seconds: number;
  buffer_in_ram: boolean;
  clip_seconds: number;
  clip_long_seconds: number;
  fps: number;
  resolution: number;
  bitrate_mbps: number;
  encoder: string;
  show_cursor: boolean;
  eco_mode: boolean;
  audio_sources: AudioSource[];
  separate_tracks: boolean;
  audio_offset_ms: number;
  mic_denoise: boolean;
  input_overlay: boolean;
  /** Lettres de la superposition : "auto" (clavier de Windows), "qwerty", "azerty" */
  overlay_layout: string;
  /** Nombre de touches : "compact", "standard", "complet" */
  overlay_keys: string;
  voice_command: boolean;
  auto_update: boolean;
  language: "auto" | "fr" | "en" | string;
  hotkeys: { clip: string; clip_long: string; clip_custom: string; session: string; screenshot: string; toggle_recording: string; bookmark: string };
  sound_on_clip: boolean;
  sound_volume: number;
  toast_on_clip: boolean;
  notify_overlay: boolean;
  notify_position: string;
  welcome_banner: boolean;
  cs2: Cs2Rules & {
    enabled: boolean;
    port: number;
    timing: "round" | "instant" | string;
    modes: Record<string, boolean>;
    /** Profil utilisé : "legit" (règles ci-dessus) ou "hvh" (règles de `hvh`) */
    profile: "legit" | "hvh" | string;
    hvh: Cs2Rules;
  };
  theme: "dark" | "light" | string;
  accent: string;
  animations: boolean;
  density: "comfortable" | "compact" | string;
  webhooks: { name: string; url: string }[];
  discord_max_mb: number;
  /** Partage de clips entre amis (rien n'est envoyé tant que ce n'est pas activé) */
  share_enabled: boolean;
  share_auto_download: boolean;
  /** Présence Discord (profil : « Enregistre Counter-Strike 2 ») */
  discord_presence: boolean;
  max_library_gb: number;
  first_run_done: boolean;
  clip_sound: string;
  clip_sound_name: string;
  welcome_done: boolean;
  medal_prompt_dismissed: boolean;
};

/** Règles d'un profil CS2 (legit ou HvH) */
export type Cs2Rules = {
  merge_gap: number;
  events: Record<string, EventRule>;
  onetap_ignore_snipers: boolean;
  streak_count: number;
  streak_window: number;
  min_round_kills: number;
  min_sequence_kills: number;
  max_sequence: number;
  special_always: boolean;
};

export type Status = {
  recording: boolean;
  starting: boolean;
  buffered: number;
  buffer_target: number;
  encoder: string;
  width: number;
  height: number;
  fps: number;
  buffer_mb: number;
  error: string;
  audio: { label: string; level: number; active: boolean }[];
  app: string;
  /** Début du clip sur mesure en cours (0 = aucun) */
  custom_since: number;
  /** Début de la session complète en cours (0 = aucune) */
  session_since: number;
  /** Commande vocale : 0 coupée, 1 démarrage, 2 à l'écoute, 3 indisponible, 4 erreur */
  voice: number;
};

export type Hardware = {
  ram_gb: number;
  monitors: { index: number; handle: number; name: string; width: number; height: number; primary: boolean; x: number; y: number }[];
  outputs: { id: string; name: string; default: boolean }[];
  inputs: { id: string; name: string; default: boolean }[];
  encoders: string[];
  apps: string[];
  ffmpeg: string;
};

export type Cs2Info = {
  installs: string[];
  configured: string[];
  running: boolean;
  needs_restart: boolean;
  since_last: number;
  live: { messages: number; map: string; mode: string; map_phase: string; round_phase: string; spectating: boolean; token_mismatch: boolean; kills: number };
};

export type DiagnosticOut = { text: string; problems: number; path: string | null; copied: boolean };

export type UpdateInfo ={ configured: boolean; current: string; available: string | null; notes: string };

/** Touche clavier/souris : `key` = code virtuel Windows (1 clic gauche, 2 clic droit, 0x57 W…), `t` en secondes dans le clip. */
export type InputEvent = { t: number; key: number; down: boolean };

export type MediaInfo = { duration: number; width: number; height: number; fps: number; tracks: string[] };
export type Filmstrip = { sprite: string; frames: number; interval: number; frame_w: number; frame_h: number };
export type Waveform = { rate: number; peaks: number[] };

export type StudioExportPiece = {
  path: string;
  start: number;
  end: number;
  speed: number;
  volume: number;
  track_volumes: number[];
  transition: string;
  transition_dur: number;
  clip_id: string;
};
export type StudioExportOptions = {
  title: string;
  format: string;
  height: number;
  fps: number;
  max_mb: number;
  music: { path: string; volume: number; offset: number; at: number; fade_in: number; fade_out: number; duck: boolean } | null;
  keys: boolean;
  lang: string;
};

export type Moment = { time: number; kind: string; multi: string; weapon: string; round: number; count?: number };
export type Cs2Event =
  | { Connected: { map: string; mode: string } }
  | { Moment: Moment }
  | { Clip: { title: string } };

export type VoiceHeard = { at: number; confidence: number; accepted: boolean };

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<{ hotkey_errors: string[]; cs2_files: string[] }>("save_settings", { settings }),
  status: () => invoke<Status>("get_status"),
  clips: () => invoke<Clip[]>("list_clips"),
  refresh: () => invoke<Clip[]>("refresh_library"),
  clipNow: (seconds?: number) => invoke<void>("clip_now", { seconds }),
  /** "custom" (clip sur mesure), "session" (session complète) ou "bookmark" (repère) : comme les raccourcis */
  holdAction: (action: "custom" | "session" | "bookmark") => invoke<void>("hold_action", { action }),
  clipInputs: (id: string) => invoke<InputEvent[]>("clip_inputs", { id }),
  /** Pendant la saisie d'un raccourci : rien ne se déclenche (se rouvre seul après 60 s). */
  hotkeysPause: (on: boolean) => invoke<void>("hotkeys_pause", { on }),
  /** Dernière fois que « Rewind clip » a été entendu (null si jamais). */
  voiceLast: () => invoke<VoiceHeard | null>("voice_last"),
  /** Lettres de l'overlay dans la disposition du clavier (code QWERTY → lettre, ex. 0x57 → « Z » en AZERTY). */
  keyLabels: () => invoke<Record<string, string>>("key_labels"),
  checkUpdate: () => invoke<UpdateInfo>("check_update"),
  installUpdate: () => invoke<void>("install_update"),
  screenshot: () => invoke<void>("take_screenshot"),
  setRecording: (enabled: boolean) => invoke<void>("set_recording", { enabled }),
  setMicMuted: (muted: boolean) => invoke<void>("set_mic_muted", { muted }),
  updateClip: (id: string, patch: { title?: string; favorite?: boolean; tags?: string[] }) =>
    invoke<Clip>("update_clip", { id, ...patch }),
  /** Ajoute (add) ou retire des clips d'une collection en une fois ; renvoie les clips modifiés. */
  setCollection: (ids: string[], name: string, add: boolean) => invoke<Clip[]>("set_collection", { ids, name, add }),
  renameCollection: (old: string, name: string) => invoke<Clip[]>("rename_collection", { old, new: name }),
  deleteCollection: (name: string) => invoke<Clip[]>("delete_collection", { name }),
  deleteClips: (ids: string[]) => invoke<void>("delete_clips", { ids }),
  reveal: (id: string) => invoke<void>("reveal_clip", { id }),
  openExternal: (id: string) => invoke<void>("open_external", { id }),
  openFolder: (path: string) => invoke<void>("open_folder", { path }),
  copy: (id: string) => invoke<void>("copy_clip", { id }),
  copyForDiscord: (id: string) => invoke<{ compressed: boolean; size_mb: number; height: number; max_mb: number }>("copy_for_discord", { id }),
  studioExport: (pieces: StudioExportPiece[], options: StudioExportOptions, game: string) =>
    invoke<Clip>("studio_export", { pieces, options, game }),
  /** Export du Studio v2 (spec calculée par studio/export.ts). */
  studioRender: (spec: unknown, game: string) => invoke<Clip>("studio_render", { spec, game }),
  projectsList: () => invoke<{ id: string; name: string; updated: number; duration: number; thumb: string; pieces: number }[]>("projects_list"),
  projectLoad: (id: string) => invoke<string>("project_load", { id }),
  projectSave: (id: string, json: string) => invoke<void>("project_save", { id, json }),
  projectDelete: (id: string) => invoke<void>("project_delete", { id }),
  /** Image PNG (base64) → fichier pour l'export. */
  studioPng: (b64: string) => invoke<string>("studio_png", { b64 }),
  voiceoverStart: () => invoke<void>("voiceover_start"),
  voiceoverStop: () => invoke<{ path: string; duration: number }>("voiceover_stop"),
  mediaInfo: (path: string) => invoke<MediaInfo>("media_info", { path }),
  mediaFilmstrip: (path: string) => invoke<Filmstrip>("media_filmstrip", { path }),
  mediaWaveform: (path: string) => invoke<Waveform>("media_waveform", { path }),
  mediaStems: (path: string) => invoke<string[]>("media_stems", { path }),
  /** Musique par lien : TikTok (son d'origine), YouTube, SoundCloud → fichier MP3 local */
  /** Détourage (retirer le fond) : modèle présent ? téléchargement ; masque d'une image 1024 × 1024 ; PNG gardé */
  bgReady: () => invoke<boolean>("bg_ready"),
  bgPrepare: () => invoke<void>("bg_prepare"),
  bgMask: (png: string) => invoke<string>("bg_mask", { png }),
  saveImage: (png: string, name: string) => invoke<string>("save_image", { png, name }),
  /** Sons de kill fournis (hitmarker, ding…), fabriqués par Rewind */
  killSounds: () => invoke<{ id: string; name: string; path: string; dur: number }[]>("kill_sounds"),
  musicFromLink: (url: string) => invoke<{ path: string; name: string; note: string }>("music_from_link", { url }),
  /** Moments où tes kills apparaissent dans le kill feed de CS2 (secondes dans le fichier) */
  mediaKillfeed: (path: string) => invoke<number[]>("media_killfeed", { path }),
  medalDetect: (folder?: string) => invoke<MedalInfo>("medal_detect", { folder: folder ?? null }),
  medalImport: (from?: string) => invoke<{ copied: number; skipped: number; failed: number }>("medal_import", { from: from ?? null }),
  soundPreview: (kind: string) => invoke<void>("sound_preview", { kind }),
  soundImport: (path: string) => invoke<string>("sound_import", { path }),
  adminInputStatus: () => invoke<AdminInput>("admin_input_status"),
  adminInputSet: (on: boolean) => invoke<void>("admin_input_set", { on }),
  /** Raccourcit le clip sur place (même fichier ; l'original va dans la Corbeille) */
  shortenClip: (id: string, start: number, end: number) => invoke<Clip>("clip_shorten", { id, start, end }),
  trim: (id: string, start: number, end: number, exact: boolean) => invoke<Clip>("edit_trim", { id, start, end, exact }),
  merge: (ids: string[]) => invoke<Clip>("edit_merge", { ids }),
  vertical: (id: string, mode: string, start: number, end: number) => invoke<Clip>("edit_vertical", { id, mode, start, end }),
  shareDiscord: (id: string, webhook: number, message: string) =>
    invoke<{ url: string; compressed: boolean }>("share_discord", { id, webhook, message }),
  hardware: () => invoke<Hardware>("get_hardware"),
  cs2Info: () => invoke<Cs2Info>("cs2_info"),
  cs2Install: () => invoke<string[]>("cs2_install"),
  engineLog: () => invoke<string>("engine_log"),
  /** Rapport de diagnostic complet ; `copy` = dans le presse-papiers, `save` = dans un fichier (affiché dans l'Explorateur). */
  diagnosticReport: (copy: boolean, save: boolean) => invoke<DiagnosticOut>("diagnostic_report", { copy, save }),
  quit: () => invoke<void>("quit_app"),
  // Partage entre amis
  discordContext: (view: string) => invoke<void>("discord_context", { view }),
  friendsView: () => invoke<FriendsView>("friends_view"),
  friendsEnable: (name: string) => invoke<FriendsView>("friends_enable", { name }),
  friendsDisable: () => invoke<void>("friends_disable"),
  friendsRename: (name: string) => invoke<void>("friends_rename", { name }),
  friendsAdd: (code: string) => invoke<"pending" | "friends" | string>("friends_add", { code }),
  friendsAnswer: (user: string, accept: boolean) => invoke<void>("friends_answer", { user, accept }),
  friendsRemove: (user: string, block: boolean) => invoke<void>("friends_remove", { user, block }),
  friendsRefresh: () => invoke<void>("friends_refresh"),
  friendsDeleteIdentity: () => invoke<void>("friends_delete_identity"),
  friendsRecoveryKey: () => invoke<string>("friends_recovery_key"),
  /** Nouvelle clé de secours (l'ancienne ne marche plus) : renvoie son texte */
  friendsNewRecovery: () => invoke<string>("friends_new_recovery"),
  friendsSaveRecovery: (path: string) => invoke<void>("friends_save_recovery", { path }),
  friendsRestore: (key: string) => invoke<FriendsView>("friends_restore", { key }),
  friendsSetAvatar: (path: string) => invoke<void>("friends_set_avatar", { path }),
  friendsRemoveAvatar: () => invoke<void>("friends_remove_avatar"),
  friendsAvatar: (id: string, version: number) => invoke<string>("friends_avatar", { id, version }),
  friendsShowOnline: (show: boolean) => invoke<void>("friends_show_online", { show }),
  shareSend: (id: string, to: string[]) => invoke<void>("share_send", { id, to }),
  shareCancel: (key: string) => invoke<void>("share_cancel", { key }),
  shareDismiss: (id: string) => invoke<void>("share_dismiss", { id }),
  shareReport: (id: string, reason: string) => invoke<void>("share_report", { id, reason }),
  shareDownload: (id: string) => invoke<void>("share_download", { id }),
  shareThumb: (id: string) => invoke<string>("share_thumb", { id }),
};

/** Partage entre amis : état complet (voir amis.rs). Les dates du guichet sont en millisecondes. */
export type FriendPerson = { id: string; name: string; since: number; avatar: number; online: boolean };
export type FriendIncoming = { id: string; sender: string; sender_name: string; title: string; game: string; duration: number; size: number; sha256: string; markers: Marker[]; has_thumb: boolean; created: number };
export type FriendSent = { id: string; title: string; game: string; size: number; state: string; created: number; to: { id: string; name: string; state: "waiting" | "received" | "dismissed" | string }[] };
export type FriendTransfer = { key: string; share_id: string; kind: "up" | "down"; title: string; peer: string; progress: number; state: "running" | "error" | "done"; error: string; stage: "compress" | "" };
export type FriendReceived = { share_id: string; clip_id: string; from_id: string; from_name: string; title: string; game: string; at: number };
export type FriendsView = {
  enabled: boolean;
  ready: boolean;
  online: boolean;
  error: string;
  synced_at: number;
  me: { id: string; code: string; name: string; avatar: number; show_online: boolean; recovery: string } | null;
  friends: FriendPerson[];
  incoming: FriendPerson[];
  outgoing: FriendPerson[];
  blocked: FriendPerson[];
  inbox: FriendIncoming[];
  sent: FriendSent[];
  limits: { max_clip: number; daily_bytes: number; used_today: number; part: number; keep_days: number };
  transfers: FriendTransfer[];
  received: FriendReceived[];
  /** La clé de secours du compte est gardée sur ce PC */
  recovery_here: boolean;
  /** Relève en cours */
  syncing: boolean;
};

/** Message lisible pour une erreur du partage (« ami:<code> » renvoyé par Rewind). */
export function friendError(e: unknown): string {
  const code = String(e).replace(/^Error:\s*/, "").replace(/^ami:/, "");
  const known: Record<string, string> = {
    offline: "Le service de partage est injoignable. Vérifie ta connexion Internet.",
    clock: "L'horloge de ton PC n'est pas à l'heure : règle-la dans Windows (Date et heure).",
    code_unknown: "Aucun Rewind n'a ce code ami. Vérifie les lettres.",
    own_code: "C'est ton propre code.",
    you_blocked: "Tu as bloqué cette personne : débloque-la d'abord.",
    friends_full: "Liste d'amis pleine (200 au maximum).",
    too_many_requests: "Trop de demandes en attente : attends que certaines soient acceptées.",
    not_friends: "Vous n'êtes plus amis.",
    clip_too_big: "Clip trop lourd pour le partage, même allégé (500 Mo au maximum).",
    daily_quota: "Limite d'envoi du jour atteinte (2 Go). Réessaie demain.",
    name_invalid: "Choisis un pseudo (24 caractères au maximum).",
    file_missing: "Le fichier du clip est introuvable.",
    not_a_video: "Ce n'est pas une vidéo valide.",
    hash_mismatch: "Le clip reçu est abîmé : il a été supprimé. Réessaie.",
    size_mismatch: "Le clip reçu est incomplet. Réessaie.",
    share_unknown: "Ce clip n'est plus disponible.",
    file_gone: "Ce clip n'est plus disponible.",
    too_many_registrations: "Trop d'inscriptions depuis cette connexion aujourd'hui. Réessaie demain.",
    no_identity: "Active d'abord le partage.",
    busy: "Une réception est déjà en cours.",
    cancelled: "Envoi annulé.",
    recovery_invalid: "Clé de secours invalide : vérifie les caractères (une faute est détectée automatiquement).",
    recovery_unknown: "Cette clé de secours ne correspond à aucun compte (remplacée par une plus récente, ou identité supprimée).",
    recovery_missing: "La clé de secours n'est plus sur ce PC : crée-en une nouvelle.",
    recovery_used: "Cette clé de secours sert déjà à un autre compte.",
    avatar_unreadable: "Image illisible : choisis une photo JPEG ou PNG.",
    avatar_too_big: "Image trop lourde.",
  };
  return known[code] ? t(known[code]) : t("Erreur du partage : {x}", { x: code });
}

/** Medal sur ce PC (clips à importer, réglages repris à l'accueil). */
export type MedalInfo = { found: boolean; installed: boolean; folder: string; folders: string[]; clips: number; size_mb: number; imported: number; hotkey: string | null; clip_seconds: number | null; fps: number | null; resolution: number | null; bitrate: number | null };

/** Jeux lancés en administrateur : leurs touches ne sont visibles qu'avec l'assistant. */
export type AdminInput = { game_admin: boolean; helper: boolean; enabled: boolean; rewind_admin: boolean };

export const fileUrl =(path: string) => (!path ? "" : path.startsWith("/mock/") || path.startsWith("data:") ? path : convertFileSrc(path));

const KIND_FR: Record<string, string> = {
  kill: "Kill",
  headshot: "Headshot",
  onetap: "One Tap",
  knife: "Couteau",
  zeus: "Zeus",
  collateral: "Collateral",
  "2k": "2K",
  "3k": "3K",
  "4k": "4K",
  ace: "ACE",
  clutch: "Clutch",
  streak: "Série",
  death: "Mort",
  assist: "Assist",
  manual: "Raccourci",
};
/** Libellés des moments (traduits à la lecture). */
export const KIND_LABEL: Record<string, string> = new Proxy(KIND_FR, {
  get: (o, k: string) => (k in o ? t(o[k]) : undefined),
});

export function formatDuration(s: number): string {
  if (!isFinite(s) || s <= 0) return "0:00";
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${sec.toString().padStart(2, "0")}`;
}

export function formatSize(bytes: number): string {
  if (bytes > 1e9) return `${(bytes / 1e9).toFixed(1)} ${t("Go")}`;
  if (bytes > 1e6) return `${(bytes / 1e6).toFixed(0)} ${t("Mo")}`;
  if (bytes <= 0) return `0 ${t("Ko")}`;
  return `${Math.max(1, Math.round(bytes / 1e3))} ${t("Ko")}`;
}

export function relativeDate(epoch: number): string {
  const d = new Date(epoch * 1000);
  const now = new Date();
  const diff = (now.getTime() - d.getTime()) / 1000;
  if (diff < 60) return t("à l'instant");
  if (diff < 3600) return t("il y a {n} min", { n: Math.floor(diff / 60) });
  const time = d.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return t("aujourd'hui, {h}", { h: time });
  const y = new Date(now);
  y.setDate(now.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return t("hier, {h}", { h: time });
  return d.toLocaleDateString(locale(), { day: "numeric", month: "short", year: d.getFullYear() === now.getFullYear() ? undefined : "numeric" }) + `, ${time}`;
}

export function dayGroup(epoch: number): string {
  const d = new Date(epoch * 1000);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return t("Aujourd'hui");
  const y = new Date(now);
  y.setDate(now.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return t("Hier");
  const diffDays = (now.getTime() - d.getTime()) / 86400000;
  if (diffDays < 7) return d.toLocaleDateString(locale(), { weekday: "long" }).replace(/^./, (c) => c.toUpperCase());
  return d.toLocaleDateString(locale(), { month: "long", year: "numeric" }).replace(/^./, (c) => c.toUpperCase());
}

/** Badges principaux d'un clip (événements CS2, etc.) */
export function badges(clip: Clip): { label: string; hot: boolean }[] {
  const kinds = clip.markers.map((m) => m.kind);
  const out: { label: string; hot: boolean }[] = [];
  for (const k of ["ace", "4k", "3k", "clutch", "collateral", "onetap", "knife", "zeus", "2k"]) {
    if (kinds.includes(k)) out.push({ label: KIND_LABEL[k], hot: ["ace", "4k", "clutch"].includes(k) });
  }
  const hs = kinds.filter((k) => k === "headshot").length;
  if (hs > 0 && out.length < 3) out.push({ label: hs > 1 ? `${hs}× HS` : "HS", hot: false });
  return out.slice(0, 3);
}

/** Couleur propre à chaque jeu / appli (connue, sinon déduite du nom). */
const GAME_COLORS: Record<string, string> = {
  "counter-strike 2": "#f0a23b",
  discord: "#7b86ff",
  valorant: "#ff5a64",
  fortnite: "#b07cff",
  "league of legends": "#d4af5a",
  minecraft: "#6cc24a",
  "rocket league": "#3b8cff",
  "apex legends": "#e5484d",
  "overwatch 2": "#f99e1a",
  chrome: "#5a9bff",
  edge: "#35c1d9",
  firefox: "#ff8a3d",
  brave: "#ff6a3d",
  spotify: "#1ed760",
  steam: "#66c0f4",
  bureau: "#9aa3b5",
};

/** Nom de jeu affiché : les clips du bureau (« Bureau » ou « Desktop » selon la langue à la création) dans la langue actuelle */
export function gameLabel(game: string | undefined | null): string {
  return !game || game === "Bureau" || game === "Desktop" ? t("Bureau") : game;
}

export function gameColor(name: string): string {
  const key = (name || "Bureau").toLowerCase();
  if (GAME_COLORS[key]) return GAME_COLORS[key];
  let h = 0;
  for (const c of key) h = (h * 31 + c.charCodeAt(0)) % 360;
  return `hsl(${h} 70% 62%)`;
}

/** Teinte d'un jeu dans l'interface : volontairement neutre (la couleur est réservée aux signaux). */
export function gameTone(_name: string): string {
  return "#5c5c63";
}

export function gameInitials(name: string): string {
  const words = (name || "Bureau").replace(/[^\p{L}\p{N} ]/gu, " ").split(/\s+/).filter(Boolean);
  if (words.length >= 2 && /\d/.test(words[words.length - 1])) return (words[0][0] + words[words.length - 1]).toUpperCase().slice(0, 3);
  if (words.length >= 2) return (words[0][0] + words[1][0]).toUpperCase();
  return (words[0] ?? "?").slice(0, 2).toUpperCase();
}

/** Découpe en sessions : des clips séparés de moins de 2 h appartiennent à la même session. */
export function sessions(clips: Clip[]): { label: string; games: string[]; clips: Clip[] }[] {
  const sorted = [...clips].sort((a, b) => b.created - a.created);
  const out: { label: string; games: string[]; clips: Clip[] }[] = [];
  for (const c of sorted) {
    const last = out[out.length - 1];
    const prev = last?.clips[last.clips.length - 1];
    if (last && prev && prev.created - c.created < 2 * 3600) last.clips.push(c);
    else out.push({ label: "", games: [], clips: [c] });
  }
  for (const s of out) {
    const start = new Date(s.clips[s.clips.length - 1].created * 1000);
    const h = start.getHours();
    const moment = h < 6 ? "nuit" : h < 12 ? "matin" : h < 18 ? "après-midi" : "soir";
    const day = dayGroup(s.clips[0].created);
    const now = new Date();
    const ageDays = (now.getTime() - start.getTime()) / 86400000;
    if (day === t("Aujourd'hui")) s.label = t(moment === "nuit" ? "Cette nuit" : moment === "après-midi" ? "Cet après-midi" : moment === "matin" ? "Ce matin" : "Ce soir");
    else if (ageDays < 7) s.label = `${day} ${t(moment)}`;
    else s.label = `${start.toLocaleDateString(locale(), { day: "numeric", month: "short" })} · ${t(moment)}`;
    s.label += ` · ${start.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" })}`;
    const counts = new Map<string, number>();
    for (const c of s.clips) counts.set(gameLabel(c.game), (counts.get(gameLabel(c.game)) ?? 0) + 1);
    s.games = [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([g]) => g);
  }
  return out;
}
