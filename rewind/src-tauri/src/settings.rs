//! Réglages de Rewind, enregistrés dans `%LOCALAPPDATA%\Rewind\settings.json`.
//! Chaque champ a une valeur par défaut : un ancien fichier incomplet reste lisible.

use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioSource {
    pub id: String,
    /// "system" (tout le PC), "mic" (micro), "app" (une application précise),
    /// "game" (le jeu seul : « Son du PC » contient alors tout le reste, musique, navigateur…)
    pub kind: String,
    /// Identifiant du périphérique (vide = celui par défaut de Windows)
    pub device: String,
    /// Exécutable pour kind = "app" (ex. "Discord.exe")
    pub app: String,
    pub label: String,
    pub enabled: bool,
    /// 0 à 200 %
    pub volume: u32,
}

impl Default for AudioSource {
    fn default() -> Self {
        Self {
            id: "system".into(),
            kind: "system".into(),
            device: String::new(),
            app: String::new(),
            label: "Autres sons du PC".into(),
            enabled: true,
            volume: 100,
        }
    }
}

impl AudioSource {
    /// Piste « Jeu » : le son du jeu seul, séparé de la musique et du navigateur (2026-11-24)
    pub fn game() -> Self {
        Self { id: "game".into(), kind: "game".into(), label: "Jeu".into(), ..Default::default() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Hotkeys {
    pub clip: String,
    pub clip_long: String,
    /// Clip sur mesure : 1er appui = début, 2e appui = fin
    pub clip_custom: String,
    /// Session complète : 1er appui = début, 2e appui = fin (un seul fichier avec repères)
    pub session: String,
    pub screenshot: String,
    pub toggle_recording: String,
    /// Repère (pendant une session ou un clip sur mesure)
    pub bookmark: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            clip: "Alt+F10".into(),
            clip_long: "Alt+Shift+F10".into(),
            clip_custom: "Alt+F11".into(),
            session: "Alt+F7".into(),
            screenshot: "Alt+F12".into(),
            toggle_recording: "Alt+F9".into(),
            bookmark: "Alt+F8".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct EventRule {
    pub enabled: bool,
    /// Secondes gardées avant l'action
    pub before: f64,
    /// Secondes gardées après l'action
    pub after: f64,
}

impl Default for EventRule {
    fn default() -> Self {
        Self { enabled: true, before: 8.0, after: 3.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Cs2Settings {
    pub enabled: bool,
    pub port: u16,
    /// "round" = un clip par round à la fin du round (comme Medal), "instant" = un clip par action
    pub timing: String,
    /// Deux actions séparées de moins de N secondes finissent dans le même clip
    pub merge_gap: f64,
    /// Règles par événement : kill, headshot, onetap, knife, zeus, collateral, 2k, 3k, 4k, ace, clutch, death, assist
    pub events: BTreeMap<String, EventRule>,
    /// Modes de jeu actifs (competitive, scrimcomp2v2, casual, deathmatch, gungameprogressive, retakes, other…)
    pub modes: BTreeMap<String, bool>,
    /// Exclure les snipers des one-tap
    pub onetap_ignore_snipers: bool,
    /// Avec rounds : nombre de kills dans le round pour faire un clip
    pub min_round_kills: u32,
    /// Sans rounds (deathmatch, échauffement) : nombre de kills dans une séquence
    pub min_sequence_kills: u32,
    /// Sans rounds : durée maximale d'un clip (s)
    pub max_sequence: f64,
    /// Toujours clipper one tap, couteau, zeus, collateral, même seuls
    pub special_always: bool,
    /// Série : nombre de kills…
    pub streak_count: u32,
    /// …avec au plus N secondes entre deux kills
    pub streak_window: f64,
    /// Profil utilisé : "legit" (les champs ci-dessus) ou "hvh" (les règles de `hvh`)
    pub profile: String,
    /// Règles du profil HvH (le profil legit utilise les champs ci-dessus)
    pub hvh: Cs2Rules,
}

/// Règles d'un profil (mêmes noms que dans `Cs2Settings`)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Cs2Rules {
    pub merge_gap: f64,
    pub events: BTreeMap<String, EventRule>,
    pub onetap_ignore_snipers: bool,
    pub min_round_kills: u32,
    pub min_sequence_kills: u32,
    pub max_sequence: f64,
    pub special_always: bool,
    pub streak_count: u32,
    pub streak_window: f64,
}

impl Default for Cs2Rules {
    /// Valeurs conseillées en HvH : presque tout est en one tap, donc seulement 3 kills ou plus
    fn default() -> Self {
        let mut events = Cs2Settings::default_events();
        for k in ["onetap", "knife", "zeus", "collateral"] {
            if let Some(r) = events.get_mut(k) {
                r.enabled = false;
            }
        }
        Self {
            merge_gap: 6.0,
            events,
            onetap_ignore_snipers: true,
            min_round_kills: 3,
            min_sequence_kills: 3,
            max_sequence: 45.0,
            special_always: false,
            streak_count: 3,
            streak_window: 10.0,
        }
    }
}

impl Cs2Rules {
    fn sanitize(&mut self) {
        self.merge_gap = self.merge_gap.clamp(2.0, 60.0);
        self.min_round_kills = self.min_round_kills.clamp(1, 5);
        self.min_sequence_kills = self.min_sequence_kills.clamp(1, 20);
        self.max_sequence = self.max_sequence.clamp(10.0, 180.0);
        self.streak_count = self.streak_count.clamp(2, 20);
        self.streak_window = self.streak_window.clamp(1.0, 60.0);
        for (k, v) in Cs2Settings::default_events() {
            self.events.entry(k).or_insert(v);
        }
    }
}

impl Cs2Settings {
    /// Réglages que le moteur CS2 doit utiliser : ceux du profil choisi (legit ou HvH)
    pub fn effective(&self) -> Cs2Settings {
        let mut s = self.clone();
        if self.profile == "hvh" {
            let r = &self.hvh;
            s.merge_gap = r.merge_gap;
            s.events = r.events.clone();
            s.onetap_ignore_snipers = r.onetap_ignore_snipers;
            s.min_round_kills = r.min_round_kills;
            s.min_sequence_kills = r.min_sequence_kills;
            s.max_sequence = r.max_sequence;
            s.special_always = r.special_always;
            s.streak_count = r.streak_count;
            s.streak_window = r.streak_window;
        }
        s
    }

    fn default_events() -> BTreeMap<String, EventRule> {
        let mut events = BTreeMap::new();
        // 5 s avant, 3 s après pour tout (choix de l'utilisateur, 2026-11-13)
        let on = |_b: f64, _a: f64| EventRule { enabled: true, before: 5.0, after: 3.0 };
        let off = |_b: f64, _a: f64| EventRule { enabled: false, before: 5.0, after: 3.0 };
        // « kill » = règle du nombre de kills par round (min_round_kills) ; les autres déclenchent seuls.
        // Headshot et 2K coupés par défaut : sinon presque chaque round ferait un clip.
        events.insert("kill".into(), on(6.0, 2.0));
        events.insert("streak".into(), on(12.0, 3.0));
        events.insert("headshot".into(), off(6.0, 2.0));
        events.insert("onetap".into(), on(6.0, 2.5));
        events.insert("knife".into(), on(6.0, 3.0));
        events.insert("zeus".into(), on(6.0, 3.0));
        events.insert("collateral".into(), on(6.0, 3.0));
        events.insert("2k".into(), off(6.0, 2.0));
        events.insert("3k".into(), on(8.0, 3.0));
        events.insert("4k".into(), on(8.0, 3.0));
        events.insert("ace".into(), on(10.0, 4.0));
        // Clutch estimé : CS2 ne dit pas au joueur si ses coéquipiers sont vivants → souvent faux, désactivé
        events.insert("clutch".into(), off(10.0, 4.0));
        events.insert("death".into(), off(6.0, 2.0));
        events.insert("assist".into(), off(5.0, 2.0));
        events
    }
}

impl Default for Cs2Settings {
    fn default() -> Self {
        let events = Self::default_events();
        let mut modes = BTreeMap::new();
        for (m, v) in [
            ("competitive", true),
            ("premier", true),
            ("scrimcomp2v2", true),
            ("casual", true),
            ("deathmatch", true),
            ("gungameprogressive", true),
            ("retakes", true),
            ("survival", true),
            ("other", true),
        ] {
            modes.insert(m.into(), v);
        }
        Self {
            enabled: true,
            port: 12762,
            timing: "round".into(),
            merge_gap: 6.0,
            events,
            modes,
            onetap_ignore_snipers: true,
            streak_count: 3,
            streak_window: 10.0,
            min_round_kills: 3,
            min_sequence_kills: 3,
            max_sequence: 45.0,
            special_always: false,
            profile: "legit".into(),
            hvh: Cs2Rules::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Webhook {
    pub name: String,
    pub url: String,
}

impl Default for Webhook {
    fn default() -> Self {
        Self { name: "Mon serveur".into(), url: String::new() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    // --- Général
    pub launch_at_startup: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub clip_folder: String,
    pub folder_per_game: bool,
    // --- Enregistrement
    pub recording_enabled: bool,
    /// "monitor" ou "window"
    pub capture_mode: String,
    /// Nom de l'écran (ex. \.\DISPLAY1) ; vide = écran principal
    pub monitor: String,
    /// Pour capture_mode = "window" : exécutable de la fenêtre (ex. cs2.exe)
    pub window_exe: String,
    /// Durée gardée en mémoire tampon (secondes)
    pub buffer_seconds: u32,
    /// Mémoire tampon en RAM plutôt que sur le disque (aucune écriture sur le SSD)
    pub buffer_in_ram: bool,
    /// Durée du clip « court » (raccourci principal)
    pub clip_seconds: u32,
    /// Durée du clip « long »
    pub clip_long_seconds: u32,
    pub fps: u32,
    /// Petit côté de l'image (720, 1080, 1440, 2160) ; 0 = taille réelle de l'écran
    pub resolution: u32,
    pub bitrate_mbps: u32,
    /// "auto" ou nom d'encodeur FFmpeg (h264_nvenc, hevc_nvenc, av1_nvenc, h264_amf, h264_qsv, libx264)
    pub encoder: String,
    pub show_cursor: bool,
    /// Mode économie : limite automatiquement à 720p30 sur les petits PC
    pub eco_mode: bool,
    // --- Audio
    pub audio_sources: Vec<AudioSource>,
    pub separate_tracks: bool,
    /// Décalage du son en millisecondes (négatif = son plus tôt), réglage fin
    pub audio_offset_ms: i32,
    /// Suppression du bruit du micro (souffle, ventilateur, clavier léger)
    pub mic_denoise: bool,
    /// Touches clavier/souris à l'écran (notées en jeu seulement, gardées avec chaque clip)
    pub input_overlay: bool,
    /// Lettres de la superposition : "auto" (clavier de Windows), "qwerty" ou "azerty"
    pub overlay_layout: String,
    /// Nombre de touches affichées : "compact", "standard" ou "complet"
    pub overlay_keys: String,
    /// Commande vocale « Rewind clip » (reconnaissance vocale de Windows, hors ligne)
    pub voice_command: bool,
    /// Recherche automatique des mises à jour au démarrage
    pub auto_update: bool,
    /// Langue de l'interface : "auto" (celle de Windows), "fr" ou "en"
    pub language: String,
    // --- Raccourcis
    pub hotkeys: Hotkeys,
    // --- Notifications
    pub sound_on_clip: bool,
    pub sound_volume: u32,
    /// Son du clip : « shutter » (déclic), « chime » (carillon), « rewind » (rembobinage), « ding » (ancien)
    pub clip_sound: String,
    /// Nom du son importé par l'utilisateur (fichier : <données>\son-perso.wav)
    pub clip_sound_name: String,
    /// Fenêtre d'accueil vue (import Medal, dossier, raccourcis, qualité, options)
    pub welcome_done: bool,
    /// « Clips Medal trouvés » masqué par l'utilisateur
    pub medal_prompt_dismissed: bool,
    pub toast_on_clip: bool,
    /// Mini-bandeau à l'écran quand un clip est gardé
    pub notify_overlay: bool,
    /// left-middle, top-left, top-right, top-center, bottom-left, bottom-right
    pub notify_position: String,
    /// Bandeau de 6 s avec les raccourcis quand un jeu se lance
    pub welcome_banner: bool,
    // --- Jeux
    pub cs2: Cs2Settings,
    // --- Apparence
    pub theme: String,
    pub accent: String,
    pub animations: bool,
    pub density: String,
    // --- Partage
    pub webhooks: Vec<Webhook>,
    /// « Copier pour Discord » : taille maximale du fichier (Mo)
    pub discord_max_mb: u32,
    /// Partage de clips entre amis (rien n'est envoyé tant que ce n'est pas activé)
    pub share_enabled: bool,
    /// Clips reçus d'amis : téléchargés tout seuls (sinon au clic)
    pub share_auto_download: bool,
    /// Présence Discord : « Enregistre Counter-Strike 2 » sur le profil (comme Medal ou Spotify)
    pub discord_presence: bool,
    // --- Stockage
    /// Taille max de la bibliothèque en Go (0 = pas de limite)
    pub max_library_gb: u32,
    pub first_run_done: bool,
    /// Version des réglages (mise à niveau automatique des anciens fichiers)
    #[serde(default)]
    pub config_version: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            launch_at_startup: true,
            start_minimized: false,
            close_to_tray: true,
            clip_folder: util::videos_dir().join("Rewind").to_string_lossy().to_string(),
            folder_per_game: true,
            recording_enabled: true,
            capture_mode: "monitor".into(),
            monitor: String::new(),
            window_exe: String::new(),
            buffer_seconds: 120,
            buffer_in_ram: false,
            clip_seconds: 30,
            clip_long_seconds: 120,
            fps: 60,
            resolution: 1080,
            bitrate_mbps: 20,
            encoder: "auto".into(),
            show_cursor: true,
            eco_mode: false,
            audio_sources: vec![
                AudioSource::default(),
                AudioSource::game(),
                AudioSource {
                    id: "mic".into(),
                    kind: "mic".into(),
                    label: "Micro".into(),
                    ..Default::default()
                },
                AudioSource {
                    id: "discord".into(),
                    kind: "app".into(),
                    app: "Discord.exe".into(),
                    label: "Discord".into(),
                    ..Default::default()
                },
            ],
            separate_tracks: true,
            audio_offset_ms: 0,
            mic_denoise: false,
            input_overlay: false,
            overlay_layout: "auto".into(),
            overlay_keys: "standard".into(),
            voice_command: false,
            auto_update: true,
            language: "auto".into(),
            hotkeys: Hotkeys::default(),
            sound_on_clip: true,
            sound_volume: 60,
            clip_sound: "shutter".into(),
            clip_sound_name: String::new(),
            welcome_done: false,
            medal_prompt_dismissed: false,
            toast_on_clip: true,
            notify_overlay: true,
            notify_position: "top-left".into(),
            welcome_banner: true,
            cs2: Cs2Settings::default(),
            theme: "dark".into(),
            accent: "#c8f031".into(),
            animations: true,
            density: "comfortable".into(),
            webhooks: Vec::new(),
            discord_max_mb: 20,
            share_enabled: false,
            share_auto_download: true,
            discord_presence: true,
            max_library_gb: 0,
            first_run_done: false,
            config_version: 7,
        }
    }
}

impl Settings {
    fn path() -> std::path::PathBuf {
        util::data_dir().join("settings.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::path())
    }

    fn load_from(path: &std::path::Path) -> Self {
        let path = path.to_path_buf();
        let backup = path.with_extension("bak.json");
        let parse = |p: &std::path::Path| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<Settings>(&t).ok());
        let loaded = match parse(&path) {
            Some(s) => Some(s),
            None if path.exists() => {
                // Fichier abîmé : on le met de côté et on repart de la sauvegarde de secours
                let aside = path.with_extension(format!("abime-{}.json", chrono::Local::now().format("%Y%m%d-%H%M%S")));
                let _ = std::fs::rename(&path, &aside);
                log::warn!("réglages illisibles, mis de côté dans {}", aside.display());
                parse(&backup)
            }
            None => None,
        };
        let mut s = loaded.unwrap_or_default();
        s.sanitize();
        s
    }

    fn save_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        if path.exists() {
            let _ = std::fs::copy(&path, path.with_extension("bak.json"));
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Garde des valeurs raisonnables même si le fichier a été modifié à la main.
    pub fn sanitize(&mut self) {
        self.fps = self.fps.clamp(10, 240);
        self.bitrate_mbps = self.bitrate_mbps.clamp(1, 150);
        self.clip_seconds = self.clip_seconds.clamp(5, 3600);
        self.clip_long_seconds = self.clip_long_seconds.clamp(5, 3600);
        let longest = self.clip_seconds.max(self.clip_long_seconds);
        self.buffer_seconds = self.buffer_seconds.clamp(10, 3600).max(longest);
        self.sound_volume = self.sound_volume.min(100);
        if !["shutter", "chime", "rewind", "ding", "custom"].contains(&self.clip_sound.as_str()) {
            self.clip_sound = "shutter".into();
        }
        if self.discord_max_mb == 0 {
            self.discord_max_mb = 20;
        }
        self.discord_max_mb = self.discord_max_mb.clamp(5, 500);
        self.audio_offset_ms = self.audio_offset_ms.clamp(-500, 500);
        if !["auto", "qwerty", "azerty"].contains(&self.overlay_layout.as_str()) {
            self.overlay_layout = "auto".into();
        }
        if !["compact", "standard", "complet"].contains(&self.overlay_keys.as_str()) {
            self.overlay_keys = "standard".into();
        }
        for s in &mut self.audio_sources {
            s.volume = s.volume.min(300);
        }
        if self.config_version < 2 {
            // v2 : tous les modes CS2 actifs (HvH, DM, entraînement), chaque kill clippé, 60 i/s conseillé
            for v in self.cs2.modes.values_mut() {
                *v = true;
            }
            if let Some(k) = self.cs2.events.get_mut("kill") {
                k.enabled = true;
            }
            if self.cs2.streak_count == 0 {
                self.cs2.streak_count = 3;
                self.cs2.streak_window = 10.0;
            }
            // 60 i/s par défaut (choix de l'utilisateur) : l'écran ne suit souvent pas au-delà
            if self.fps > 60 {
                self.fps = 60;
                self.bitrate_mbps = self.bitrate_mbps.min(20);
            }
            self.config_version = 2;
        }
        if self.config_version < 3 {
            // v3 : marges 5 s / 3 s, séquences de 12 s sans kill, notification en haut à gauche
            for r in self.cs2.events.values_mut() {
                r.before = 5.0;
                r.after = 3.0;
            }
            self.cs2.timing = "round".into();
            self.cs2.merge_gap = 12.0;
            self.notify_position = "top-left".into();
            self.config_version = 3;
        }
        if self.config_version < 4 {
            // v4 (HvH) : clip à partir de 3 kills, séquences de 6 s, 45 s maximum
            self.cs2.min_round_kills = 3;
            self.cs2.min_sequence_kills = 3;
            self.cs2.max_sequence = 45.0;
            self.cs2.merge_gap = 6.0;
            if let Some(c) = self.cs2.events.get_mut("clutch") {
                c.enabled = false;
            }
            self.config_version = 4;
        }
        if self.config_version < 5 {
            // v5 : les réglages par événement comptent de nouveau (CS2 normal). Un clip par round reste
            // la règle. Valeurs de départ : N kills + one tap, couteau, zeus, collateral, 3K, 4K, ACE.
            for (k, on) in [
                ("kill", true), ("headshot", false), ("2k", false), ("onetap", true), ("knife", true), ("zeus", true),
                ("collateral", true), ("3k", true), ("4k", true), ("ace", true), ("streak", true), ("death", false), ("assist", false),
            ] {
                if let Some(r) = self.cs2.events.get_mut(k) {
                    r.enabled = on;
                }
            }
            self.config_version = 5;
        }
        if self.config_version < 6 {
            // v6 : deux profils, « legit » (réglages actuels) et « HvH » (valeurs conseillées HvH)
            self.cs2.profile = "legit".into();
            self.cs2.hvh = Cs2Rules::default();
            self.config_version = 6;
        }
        if self.config_version < 7 {
            // v7 : piste « Jeu » séparée (demande du 2026-11-24 : garder le son du jeu sans la musique)
            if !self.audio_sources.iter().any(|a| a.kind == "game") {
                let at = self.audio_sources.iter().position(|a| a.kind == "system").map_or(0, |i| i + 1);
                self.audio_sources.insert(at, AudioSource::game());
            }
            // Avec la piste « Jeu », la sortie du PC garde tout le reste (musique, navigateur…)
            for a in &mut self.audio_sources {
                if a.kind == "system" && a.label == "Son du PC" {
                    a.label = "Autres sons du PC".into();
                }
            }
            self.config_version = 7;
        }
        if self.cs2.profile != "hvh" {
            self.cs2.profile = "legit".into();
        }
        self.cs2.hvh.sanitize();
        self.cs2.merge_gap = self.cs2.merge_gap.clamp(2.0, 60.0);
        self.cs2.min_round_kills = self.cs2.min_round_kills.clamp(1, 5);
        self.cs2.min_sequence_kills = self.cs2.min_sequence_kills.clamp(1, 20);
        self.cs2.max_sequence = self.cs2.max_sequence.clamp(10.0, 180.0);
        self.cs2.streak_count = self.cs2.streak_count.clamp(2, 20);
        self.cs2.streak_window = self.cs2.streak_window.clamp(1.0, 60.0);
        // Ajoute les événements CS2 manquants (nouvelle version) sans toucher aux réglages existants
        let defaults = Cs2Settings::default();
        for (k, v) in defaults.events {
            self.cs2.events.entry(k).or_insert(v);
        }
        for (k, v) in defaults.modes {
            self.cs2.modes.entry(k).or_insert(v);
        }
    }

    /// Réglages vidéo effectifs (le mode éco abaisse la charge).
    pub fn effective_video(&self) -> (u32, u32, u32) {
        if self.eco_mode {
            (self.fps.min(30), if self.resolution == 0 { 720 } else { self.resolution.min(720) }, self.bitrate_mbps.min(8))
        } else {
            (self.fps, self.resolution, self.bitrate_mbps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rewind-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reglages_abimes_repris_de_la_sauvegarde() {
        let dir = temp("reglages");
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        s.fps = 30;
        s.save_to(&path).unwrap();
        s.fps = 48;
        s.save_to(&path).unwrap(); // l'ancienne version (30) part en sauvegarde de secours
        std::fs::write(&path, "{ cassé").unwrap();
        let back = Settings::load_from(&path);
        assert_eq!(back.fps, 30, "la sauvegarde de secours doit être relue");
        let aside = std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("abime-"));
        assert!(aside, "le fichier abîmé doit être gardé de côté");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reglages_absents_valeurs_par_defaut() {
        let dir = temp("absents");
        let s = Settings::load_from(&dir.join("settings.json"));
        assert_eq!(s.config_version, 7);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn profil_hvh_utilise_ses_propres_regles() {
        let mut s = Settings::default();
        s.cs2.min_round_kills = 1;
        s.cs2.hvh.min_round_kills = 4;
        assert!(s.cs2.effective().events["onetap"].enabled, "legit : one tap coché");
        assert_eq!(s.cs2.effective().min_round_kills, 1);
        s.cs2.profile = "hvh".into();
        let e = s.cs2.effective();
        assert!(!e.events["onetap"].enabled, "HvH : one tap décoché");
        assert_eq!(e.min_round_kills, 4);
        s.cs2.profile = "n'importe quoi".into();
        s.sanitize();
        assert_eq!(s.cs2.profile, "legit");
    }

    #[test]
    fn migration_v6_garde_les_reglages_legit() {
        let mut s = Settings::default();
        s.config_version = 5;
        s.cs2.min_round_kills = 2;
        s.cs2.hvh.min_round_kills = 5;
        s.sanitize();
        assert_eq!(s.config_version, 7);
        assert_eq!(s.cs2.min_round_kills, 2, "les réglages legit restent");
        assert_eq!(s.cs2.hvh.min_round_kills, 3, "HvH part des valeurs conseillées");
        assert_eq!(s.cs2.profile, "legit");
    }

    #[test]
    fn migration_v7_ajoute_la_piste_jeu() {
        let mut s = Settings::default();
        s.audio_sources.retain(|a| a.kind != "game");
        s.audio_sources[0].label = "Son du PC".into();
        s.audio_sources[1].volume = 80; // micro réglé par l'utilisateur
        s.config_version = 6;
        s.sanitize();
        assert_eq!(s.config_version, 7);
        let kinds: Vec<&str> = s.audio_sources.iter().map(|a| a.kind.as_str()).collect();
        assert_eq!(kinds, ["system", "game", "mic", "app"]);
        assert_eq!(s.audio_sources[0].label, "Autres sons du PC");
        assert_eq!(s.audio_sources[2].volume, 80, "les autres sources ne bougent pas");
        // Piste « Jeu » retirée par l'utilisateur : elle ne revient pas
        s.audio_sources.retain(|a| a.kind != "game");
        s.sanitize();
        assert!(!s.audio_sources.iter().any(|a| a.kind == "game"));
    }

    #[test]
    fn valeurs_folles_corrigees() {
        let mut s = Settings::default();
        s.fps = 5000;
        s.audio_offset_ms = 99_999;
        s.discord_max_mb = 0;
        s.sanitize();
        assert_eq!(s.fps, 240);
        assert_eq!(s.audio_offset_ms, 500);
        assert_eq!(s.discord_max_mb, 20);
    }
}
