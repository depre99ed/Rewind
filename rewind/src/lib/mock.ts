// Mode aperçu : quand l'interface tourne dans un navigateur ordinaire (sans le moteur Rust),
// on simule les réponses du moteur avec des données d'exemple. Sert uniquement à vérifier le rendu.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { Clip, Settings } from "./api";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window && !(window as any).__REWIND_MOCK__;

if (typeof window !== "undefined" && !inTauri) {
  (window as any).__REWIND_MOCK__ = true;
  mockWindows("main");
  const now = Date.now() / 1000;
  const h = 3600;
  const mk = (i: number, title: string, game: string, ago: number, kind: string, dur: number, markers: [number, string, string][] = [], fav = false): Clip => ({
    id: `m${i}`,
    path: "/mock/clip.mp4",
    thumb: `/mock/t${i}.jpg`,
    title,
    game,
    created: now - ago,
    duration: dur,
    size: Math.round(dur * 2.6e6),
    kind,
    markers: markers.map(([time, k, label]) => ({ time, kind: k, label })),
    favorite: fav,
    tags: [],
  });
  const clips: Clip[] = [
    mk(1, "CS2 ACE · 2× One Tap", "Counter-Strike 2", 120, "auto", 34, [[8, "headshot", "Headshot · AK47"], [12, "onetap", "One Tap · AK47"], [15, "headshot", "Headshot · AK47"], [21, "onetap", "One Tap · DEAGLE"], [26, "kill", "Kill · DEAGLE"]], true),
    mk(3, "Fou rire en vocal", "Discord", 1500, "manual", 30, [[29, "manual", "Raccourci"]]),
    mk(4, "CS2 4K · 3× Headshot", "Counter-Strike 2", 2.1 * h, "auto", 27, [[6, "headshot", "Headshot"], [10, "headshot", "Headshot"], [14, "kill", "Kill"], [19, "headshot", "Headshot"]]),
    mk(5, "CS2 One Tap", "Counter-Strike 2", 3 * h, "auto", 9, [[6, "onetap", "One Tap · USP-S"]]),
    mk(7, "CS2 Collateral", "Counter-Strike 2", 26 * h, "auto", 12, [[7, "collateral", "Collateral · AWP"], [7, "collateral", "Collateral · AWP"], [7.6, "2k", "2K"], [9.5, "death", "Mort"], [10.2, "assist", "Assist"]]),
    mk(8, "Clutch 1v3 inferno", "Counter-Strike 2", 27 * h, "edit", 41, [], true),
    mk(9, "CS2 3K · Headshot", "Counter-Strike 2", 28 * h, "auto", 22, [[5, "kill", "Kill"], [11, "headshot", "Headshot"], [16, "kill", "Kill"]]),
    mk(10, "Chrome", "Chrome", 30 * h, "manual", 30),
    mk(11, "CS2 Couteau", "Counter-Strike 2", 3 * 24 * h, "auto", 10, [[7, "knife", "Couteau"]]),
    mk(12, "Bug de physique", "Bureau", 4 * 24 * h, "manual", 60),
    // Clips reçus d'amis (onglet Amis seulement)
    mk(15, "CS2 Clutch 1v2", "Counter-Strike 2", 5 * h, "received", 27, [[6, "headshot", "Headshot"], [14, "kill", "Kill"]]),
    mk(16, "Fou rire Valorant", "Valorant", 26 * h, "received", 18),
    mk(13, "CS2 2K · One Tap", "Counter-Strike 2", 9 * 24 * h, "auto", 14, [[6, "onetap", "One Tap"], [9, "kill", "Kill"]]),
    mk(14, "Capture Counter-Strike 2", "Counter-Strike 2", 10 * 24 * h, "screenshot", 0),
  ];
  let settings: Settings = {
    launch_at_startup: true, start_minimized: false, close_to_tray: true,
    clip_folder: "C:\\Users\\User\\Videos\\Rewind", folder_per_game: true, recording_enabled: true,
    capture_mode: "monitor", monitor: "", window_exe: "", buffer_seconds: 120, buffer_in_ram: false, clip_seconds: 30, clip_long_seconds: 120,
    fps: 60, resolution: 1080, bitrate_mbps: 20, encoder: "auto", show_cursor: true, eco_mode: false,
    audio_sources: [
      { id: "system", kind: "system", device: "", app: "", label: "Son du PC", enabled: true, volume: 100 },
      { id: "mic", kind: "mic", device: "", app: "", label: "Micro", enabled: true, volume: 100 },
      { id: "discord", kind: "app", device: "", app: "Discord.exe", label: "Discord", enabled: true, volume: 100 },
    ],
    separate_tracks: true,
    audio_offset_ms: 0,
    mic_denoise: false,
    input_overlay: true,
    overlay_layout: "auto",
    overlay_keys: "standard",
    voice_command: true,
    auto_update: true,
    language: new URLSearchParams(location.search).get("lang") ?? "auto",
    hotkeys: { clip: "Alt+F10", clip_long: "Alt+Shift+F10", clip_custom: "Alt+F11", session: "Alt+F7", screenshot: "Alt+F12", toggle_recording: "Alt+F9", bookmark: "Alt+F8" },
    sound_on_clip: true, sound_volume: 60, toast_on_clip: true, notify_overlay: true, notify_position: "left-middle", welcome_banner: true,
    cs2: {
      enabled: true, port: 12762, timing: "round", merge_gap: 6, onetap_ignore_snipers: true, streak_count: 3, streak_window: 10, min_round_kills: 3, min_sequence_kills: 3, max_sequence: 45, special_always: false,
      events: Object.fromEntries(
        [["kill", 1], ["streak", 1], ["headshot", 1], ["onetap", 1], ["knife", 1], ["zeus", 1], ["collateral", 1], ["2k", 1], ["3k", 1], ["4k", 1], ["ace", 1], ["clutch", 1], ["death", 0], ["assist", 0]].map(
          ([k, on]) => [k, { enabled: !!on, before: k === "ace" ? 10 : 6, after: 3 }],
        ),
      ),
      modes: { competitive: true, scrimcomp2v2: true, casual: true, deathmatch: false, gungameprogressive: false, retakes: false, survival: true, other: true },
      profile: "legit",
      hvh: {
        merge_gap: 6, onetap_ignore_snipers: true, streak_count: 3, streak_window: 10, min_round_kills: 3, min_sequence_kills: 3, max_sequence: 45, special_always: false,
        events: Object.fromEntries(
          [["kill", 1], ["streak", 1], ["headshot", 0], ["onetap", 0], ["knife", 0], ["zeus", 0], ["collateral", 0], ["2k", 0], ["3k", 1], ["4k", 1], ["ace", 1], ["clutch", 0], ["death", 0], ["assist", 0]].map(
            ([k, on]) => [k, { enabled: !!on, before: 5, after: 3 }],
          ),
        ),
      },
    },
    theme: "dark", accent: "#c8f031", animations: true, density: "comfortable", webhooks: [], discord_max_mb: 20, share_enabled: true, share_auto_download: true, discord_presence: true, max_library_gb: 0, first_run_done: true, clip_sound: "shutter", clip_sound_name: "", welcome_done: !new URLSearchParams(location.search).has("welcome"), medal_prompt_dismissed: false,
  };
  const params = new URLSearchParams(location.search);
  if (params.get("theme") === "light") settings.theme = "light";
  if (params.get("lang")) settings.language = params.get("lang")!;
  if (params.get("accent")) settings.accent = "#" + params.get("accent");
  const empty = params.get("empty") === "1";
  const projects: Record<string, string> = {};
  const ms = Date.now();
  let friends: any = {
    enabled: params.get("amis") !== "0", ready: params.get("amis") !== "0", online: true, error: "", synced_at: ms / 1000,
    me: params.get("amis") === "0" ? null : { id: "moi", code: "RW-7KQ2-9XZ4", name: "Ethan", avatar: 1, show_online: true, recovery: "x" },
    recovery_here: params.get("secours") !== "0", syncing: false,
    friends: [{ id: "f1", name: "Lucas", since: ms - 9 * 86400e3, avatar: 1, online: true }, { id: "f2", name: "Noah", since: ms - 3 * 86400e3, avatar: 1, online: false }, { id: "f3", name: "Sacha", since: ms - 86400e3, avatar: 0, online: true }],
    incoming: [{ id: "f4", name: "Inès", since: ms - 2 * 3600e3, avatar: 0, online: false }],
    outgoing: [{ id: "f5", name: "Tom", since: ms - 20 * 3600e3, avatar: 0, online: false }],
    blocked: [],
    inbox: [
      { id: "i1", sender: "f1", sender_name: "Lucas", title: "CS2 ACE · 2× One Tap", game: "Counter-Strike 2", duration: 34, size: 48_200_000, sha256: "", markers: [], has_thumb: true, created: ms - 25 * 60e3 },
      { id: "i2", sender: "f2", sender_name: "Noah", title: "Clutch 1v3 Inferno", game: "Counter-Strike 2", duration: 41, size: 61_000_000, sha256: "", markers: [], has_thumb: true, created: ms - 8 * 60e3 },
    ],
    sent: [
      { id: "s1", title: "CS2 4K · 3× Headshot", game: "Counter-Strike 2", size: 52_000_000, state: "done", created: ms - 3 * 3600e3, to: [{ id: "f1", name: "Lucas", state: "received" }, { id: "f2", name: "Noah", state: "received" }] },
      { id: "s2", title: "Fou rire en vocal", game: "Discord", size: 22_000_000, state: "ready", created: ms - 40 * 60e3, to: [{ id: "f3", name: "Sacha", state: "waiting" }] },
    ],
    limits: { max_clip: 500 * 1048576, daily_bytes: 2048 * 1048576, used_today: 74 * 1048576, part: 50 * 1048576, keep_days: 7 },
    transfers: [
      { key: "down-i1", share_id: "i1", kind: "down", title: "CS2 ACE · 2× One Tap", peer: "Lucas", progress: 0.42, state: "running", error: "", stage: "" },
      ...(params.get("envoi") ? [{ key: "local-m1", share_id: "", kind: "up", title: "CS2 Deagle 1 tap", peer: "Lucas, Noah", progress: 0.14, state: "running", error: "", stage: "compress" }] : []),
    ],
    received: [
      { share_id: "r1", clip_id: "m15", from_id: "f2", from_name: "Noah", title: "CS2 Clutch 1v2", game: "Counter-Strike 2", at: now - 5 * h },
      { share_id: "r2", clip_id: "m16", from_id: "f1", from_name: "Lucas", title: "Fou rire Valorant", game: "Valorant", at: now - 26 * h },
    ],
  };
  let t = 0;
  mockIPC(
    (cmd, args: any) => {
      switch (cmd) {
        case "get_settings": return settings;
        case "save_settings": settings = args.settings; return { hotkey_errors: [], cs2_files: [] };
        case "list_clips": case "refresh_library": return empty ? [] : params.get("clips") ? clips.slice(0, Number(params.get("clips"))) : clips;
        case "get_status":
          t += 0.4;
          return {
            recording: settings.recording_enabled, starting: false, buffered: Math.min(120, 84 + t), buffer_target: 120,
            encoder: "h264_nvenc", width: 1920, height: 1080, fps: 60, buffer_mb: 290, error: "", app: "Counter-Strike 2",
            custom_since: 0, session_since: params.get("session") === "1" ? now - 754 : 0, voice: 2,
            audio: [
              { label: "Son du PC", level: 0.25 + Math.abs(Math.sin(t)) * 0.4, active: true },
              { label: "Micro", level: Math.abs(Math.sin(t * 1.7)) * 0.3, active: true },
              { label: "Discord", level: Math.abs(Math.sin(t * 0.6)) * 0.5, active: true },
            ],
          };
        case "get_hardware":
          return {
            monitors: [
              { index: 0, handle: 1, name: "\\\\.\\DISPLAY6", width: 1080, height: 1920, primary: false, x: -1080, y: 0 },
              { index: 1, handle: 2, name: "\\\\.\\DISPLAY7", width: 1920, height: 1080, primary: true, x: 0, y: 0 },
            ],
            outputs: [{ id: "o1", name: "Casque (USB Audio)", default: true }],
            inputs: [{ id: "i1", name: "Micro (USB Audio)", default: true }],
            encoders: ["h264_nvenc", "hevc_nvenc", "av1_nvenc", "libx264"], ram_gb: 31.9, apps: ["cs2.exe", "Discord.exe", "chrome.exe", "Spotify.exe"],
            ffmpeg: "C:\\Program Files\\Rewind\\bin\\ffmpeg.exe",
          };
        case "cs2_info": return { installs: ["D:\\cfg", "E:\\cfg"], configured: ["D:\\cfg", "E:\\cfg"], running: true, needs_restart: params.get("restart") === "1", since_last: params.get("restart") === "1" ? -1 : 0.4,
          live: { messages: 812, map: "de_mirage", mode: "deathmatch", map_phase: "live", round_phase: "live", spectating: false, token_mismatch: false, kills: 23 } };
        case "update_clip": {
          const c = clips.find((x) => x.id === args.id)!;
          if (args.favorite !== undefined && args.favorite !== null) c.favorite = args.favorite;
          if (args.title) c.title = args.title;
          return { ...c };
        }
        case "engine_log": return "";
        case "hotkeys_pause": return null;
        case "key_labels": return params.get("azerty") ? { "87": "Z", "65": "Q", "81": "A" } : {};
        case "voice_last": return { at: Date.now() / 1000 - 2, confidence: 0.91, accepted: true };
        // Studio v2 : projets gardés en mémoire pendant l'aperçu
        case "projects_list":
          return Object.values(projects).map((j: string) => {
            const p = JSON.parse(j);
            return { id: p.id, name: p.name, updated: p.updated, duration: (p.pieces ?? []).reduce((a: number, x: any) => a + (x.end - x.start) / (x.speed || 1), 0), thumb: p.pieces?.[0]?.thumb ?? "", pieces: p.pieces?.length ?? 0 };
          }).sort((a, b) => b.updated - a.updated);
        case "project_load": return projects[args.id] ?? Promise.reject("projet introuvable");
        case "project_save": projects[args.id] = args.json; return null;
        case "project_delete": delete projects[args.id]; return null;
        case "studio_png": return "/mock/png.png";
        case "voiceover_start": return null;
        case "voiceover_stop": return { path: "/mock/voix.wav", duration: 3.2 };
        case "studio_render":
          return new Promise((r) => {
            let v = 0;
            const id = setInterval(() => {
              v += 0.1;
              (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__?.emit?.("edit-progress", { op: "studio", value: Math.min(1, v) });
              if (v >= 1) { clearInterval(id); r({ ...clips[0], id: "export2", title: args.spec.title, kind: "edit", size: 21_000_000 }); }
            }, 150);
          });
        case "diagnostic_report": return new Promise((r) => setTimeout(() => r({ text: "REWIND · RAPPORT DE DIAGNOSTIC (démo)", problems: 3, path: null, copied: true }), 700));
        case "copy_for_discord": return new Promise((r) => setTimeout(() => r({ compressed: true, size_mb: 17.9, height: 1080, max_mb: 20 }), 900));
        case "clip_inputs": {
          // Exemple : avance (W), strafe (A/D), tirs (clic gauche), saut (espace)
          const ev: { t: number; key: number; down: boolean }[] = [];
          const press = (key: number, t: number, len: number) => { ev.push({ t, key, down: true }, { t: t + len, key, down: false }); };
          for (let t = 0.5; t < 30; t += 3) { press(0x57, t, 2.2); press(t % 6 < 3 ? 0x41 : 0x44, t + 0.4, 0.5); press(0x01, t + 1.1, 0.12); press(0x01, t + 1.5, 0.12); }
          for (let t = 2; t < 30; t += 7) press(0x20, t, 0.25);
          return ev;
        }
        case "media_info": return { duration: 34, width: 1920, height: 1080, fps: 50, tracks: ["Mixage", "Son du PC", "Micro", "Discord"] };
        case "media_filmstrip": throw "pas de vignettes en aperçu";
        case "media_stems": return [];
        case "medal_detect": if (new URLSearchParams(location.search).has("medalailleurs") && !(args as any)?.folder) return { found: false, installed: true, folder: "", folders: [], clips: 0, size_mb: 0, imported: 0, hotkey: "F8", clip_seconds: 30, fps: 60, resolution: 1080, bitrate: 10 };
          return { found: true, installed: true, folder: "C:\\Medal", folders: ["C:\\Medal", "C:\\Users\\Moi\\Videos\\Medal"], clips: 39, size_mb: 2278, imported: new URLSearchParams(location.search).has("medalok") ? 39 : 0, hotkey: "F8", clip_seconds: 30, fps: 60, resolution: 1080, bitrate: 10 };
        case "medal_import": return { copied: 39, skipped: 0, failed: 0 };
        case "sound_preview": return null;
        case "sound_import": return "mon-son";
        case "admin_input_status": return { game_admin: new URLSearchParams(location.search).has("admin"), helper: false, enabled: false, rewind_admin: false };
        case "admin_input_set": return null;
        case "media_waveform": {
          // Forme d'onde d'exemple : bruit de fond + « tirs »
          const peaks: number[] = [];
          for (let i = 0; i < 50 * 60; i++) peaks.push(Math.round(40 + 30 * Math.abs(Math.sin(i / 7)) + (i % 173 < 6 ? 170 : 0) + (i % 97 < 3 ? 120 : 0)));
          return { rate: 50, peaks };
        }
        case "studio_export":
          return new Promise((r) => {
            let v = 0;
            const id = setInterval(() => {
              v += 0.08;
              (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__?.emit?.("edit-progress", { op: "studio", value: Math.min(1, v) });
              if (v >= 1) {
                clearInterval(id);
                r({ ...clips[0], id: "export1", title: args.options.title, kind: "edit", size: 18_400_000 });
              }
            }, 200);
          });
        case "plugin:window|is_maximized": return false;
        // Partage entre amis (démo : « ?amis=0 » = pas encore activé)
        case "friends_view": return friends;
        case "friends_enable": friends = { ...friends, enabled: true, ready: true, online: true, me: { id: "moi", code: "RW-7KQ2-9XZ4", name: args.name } }; return friends;
        case "friends_disable": friends = { ...friends, enabled: false }; return null;
        case "friends_rename": friends = { ...friends, me: { ...friends.me, name: args.name } }; return null;
        case "friends_add": if (String(args.code).toUpperCase().includes("ZZZZ")) throw "ami:code_unknown"; friends = { ...friends, outgoing: [...friends.outgoing, { id: "n" + Date.now(), name: "Nouveau", since: Date.now() }] }; return "pending";
        case "friends_answer": { const p = friends.incoming.find((x: any) => x.id === args.user); friends = { ...friends, incoming: friends.incoming.filter((x: any) => x.id !== args.user), friends: args.accept && p ? [...friends.friends, p] : friends.friends }; return null; }
        case "friends_remove": friends = { ...friends, friends: friends.friends.filter((x: any) => x.id !== args.user), outgoing: friends.outgoing.filter((x: any) => x.id !== args.user), blocked: args.block ? [...friends.blocked, friends.friends.find((x: any) => x.id === args.user)].filter(Boolean) : friends.blocked.filter((x: any) => x.id !== args.user) }; return null;
        case "friends_avatar": return ({ f1: "/mock/t3.jpg", f2: "/mock/t5.jpg", moi: "/mock/t12.jpg" } as Record<string, string>)[args.id] ?? Promise.reject("ami:not_found");
        case "friends_set_avatar": friends = { ...friends, me: { ...friends.me, avatar: Date.now() } }; return null;
        case "friends_remove_avatar": friends = { ...friends, me: { ...friends.me, avatar: 0 } }; return null;
        case "friends_show_online": friends = { ...friends, me: { ...friends.me, show_online: args.show } }; return null;
        case "friends_new_recovery": friends = { ...friends, recovery_here: true }; return "RWK-NEW2-K7MZ-2PLD-WX8N-4HVB-RT6C-YJ2E-9GKU-SM5Q-AZ7D-NP3L-XW4F-B2RT";
        case "friends_refresh": friends = { ...friends, syncing: true }; setTimeout(() => { friends = { ...friends, syncing: false, synced_at: Date.now() / 1000 }; (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__?.emit?.("friends-changed", null); }, 700); return null;
        case "friends_recovery_key": if (!friends.recovery_here) throw "ami:recovery_missing"; return "RWK-FQ3A-K7MZ-2PLD-WX8N-4HVB-RT6C-YJ2E-9GKU-SM5Q-AZ7D-NP3L-XW4F-B2RT";
        case "friends_save_recovery": return null;
        case "friends_restore": if (!String(args.key).toUpperCase().startsWith("RWK")) throw "ami:recovery_invalid"; friends = { ...friends, enabled: true, ready: true, me: { id: "moi", code: "RW-7KQ2-9XZ4", name: "Ethan", avatar: 1, show_online: true } }; return friends;
        case "friends_delete_identity": friends = { ...friends, enabled: false, ready: false, me: null, friends: [], incoming: [], outgoing: [], blocked: [], inbox: [], sent: [] }; return null;
        case "share_send": {
          const key = "local-" + Date.now();
          const names = (args.to as string[]).map((id) => friends.friends.find((f: any) => f.id === id)?.name).join(", ");
          let v = 0;
          const tick = setInterval(() => {
            v = Math.min(1, v + 0.12);
            friends = { ...friends, transfers: [{ key, share_id: "s" + key, kind: "up", title: clips.find((c) => c.id === args.id)?.title ?? "Clip", peer: names, progress: v, state: v >= 1 ? "done" : "running", error: "" }, ...friends.transfers.filter((t: any) => t.key !== key)] };
            (window as any).__TAURI_EVENT_PLUGIN_INTERNALS__?.emit?.("friends-changed", null);
            if (v >= 1) clearInterval(tick);
          }, 300);
          return null;
        }
        case "share_cancel": friends = { ...friends, transfers: friends.transfers.filter((t: any) => t.key !== args.key) }; return null;
        case "share_dismiss": friends = { ...friends, inbox: friends.inbox.filter((i: any) => i.id !== args.id) }; return null;
        case "share_report": return null;
        case "share_download": return null;
        case "share_thumb": return clips[3]?.thumb ?? "";
        // Mode démo : « ?update=1 » simule une version plus récente (pour les captures)
        case "check_update":
          return {
            configured: true, current: "0.2.1",
            available: new URLSearchParams(location.search).get("update") ? "0.2.2" : null, notes: "",
          };
        default: return null;
      }
    },
    { shouldMockEvents: true },
  );
}

export {};
