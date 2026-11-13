//! Counter-Strike 2 : détection des actions par Game State Integration (API officielle de Valve).
//!
//! CS2 envoie son état en JSON à `http://127.0.0.1:<port>/` toutes les ~0,1 s. Rewind compare chaque
//! état au précédent : compteur de kills, headshots du round, munitions de chaque arme (une balle
//! en moins = un tir), arme en main… Aucune lecture de mémoire, aucune injection : zéro risque VAC.

use crate::{settings::Cs2Settings, util};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::Sender,
    time::Duration,
};

/// Une action détectée (heure absolue en secondes).
#[derive(Clone, Debug, Serialize)]
pub struct Moment {
    pub time: f64,
    /// kill, headshot, onetap, knife, zeus, collateral, death, assist
    pub kind: String,
    /// 2k, 3k, 4k, ace (vide sinon)
    pub multi: String,
    pub weapon: String,
    pub round: i64,
    /// Série de kills : nombre de kills de la série
    pub count: u32,
}

/// État en direct de la liaison avec CS2 (affiché dans l'interface, sert au diagnostic).
#[derive(Clone, Debug, Serialize, Default)]
pub struct Cs2Live {
    pub last_message: f64,
    pub messages: u64,
    pub map: String,
    pub mode: String,
    pub map_phase: String,
    pub round_phase: String,
    pub spectating: bool,
    pub token_mismatch: bool,
    pub kills: i64,
    #[serde(skip)]
    pub last_raw: String,
}

/// Demande de clip automatique envoyée au moteur.
#[derive(Clone, Debug)]
pub struct AutoClip {
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub markers: Vec<(f64, String, String)>,
}

#[derive(Clone, Debug, Serialize)]
pub enum Cs2Event {
    Connected { map: String, mode: String },
    Moment(Moment),
    Clip { title: String },
}

pub fn label(kind: &str) -> &'static str {
    match kind {
        "kill" => "Kill",
        "headshot" => "Headshot",
        "onetap" => "One Tap",
        "knife" => "Couteau",
        "zeus" => "Zeus",
        "collateral" => "Collateral",
        "death" => "Mort",
        "assist" => "Assist",
        "2k" => "2K",
        "3k" => "3K",
        "4k" => "4K",
        "ace" => "ACE",
        "clutch" => "Clutch",
        "streak" => "Série",
        _ => "Action",
    }
}

pub fn is_kill(kind: &str) -> bool {
    matches!(kind, "kill" | "headshot" | "onetap" | "knife" | "zeus" | "collateral")
}

/// Fenêtres (avant, après) en secondes pendant lesquelles aucun autre tir ne doit avoir lieu
/// pour qu'un headshot compte comme « one tap ». Valeurs choisies pour Rewind.
fn onetap_window(class: &str, snipers: bool) -> Option<(f64, f64)> {
    match class {
        "weapon_revolver" => Some((1.2, 0.7)),
        "weapon_deagle" => Some((1.0, 0.6)),
        "pistol" => Some((0.8, 0.5)),
        "rifle" | "submachinegun" => Some((0.6, 0.4)),
        "sniperrifle" if snipers => Some((1.5, 0.8)),
        _ => None,
    }
}

fn weapon_class(name: &str, kind: &str) -> String {
    if name == "weapon_deagle" || name == "weapon_revolver" {
        name.to_string()
    } else {
        kind.replace(' ', "").to_ascii_lowercase()
    }
}

#[derive(Default)]
struct Detector {
    steamid: String,
    map: String,
    mode: String,
    map_phase: String,
    round_phase: String,
    round: i64,
    kills: i64,
    deaths: i64,
    assists: i64,
    round_hs: i64,
    round_kills: i64,
    team: String,
    ammo: HashMap<String, i64>,
    /// tirs récents (heure, classe d'arme)
    shots: Vec<(f64, String)>,
    weapon: String,
    weapon_type: String,
    drawing: String,
    drawing_type: String,
    drawing_since: f64,
    round_moments: Vec<Moment>,
    /// one-taps en attente de jugement : index dans round_moments, heure du tir, classe
    pending: Vec<(usize, f64, String)>,
    /// mode « instantané » : clip en cours d'accumulation
    open_clip: Option<(f64, f64, Vec<Moment>)>,
    round_start: f64,
    /// mode instantané : nombre de moments déjà examinés
    seen: usize,
    /// série de kills en cours
    streak_last: f64,
    streak_len: u32,
    streak_idx: Option<usize>,
}

impl Detector {
    fn clear_moments(&mut self) {
        self.round_moments.clear();
        self.pending.clear();
        self.seen = 0;
        self.streak_idx = None;
    }
}

pub struct Cs2 {
    det: Detector,
    pub settings: Cs2Settings,
    token: String,
    clips: Sender<AutoClip>,
    events: Sender<Cs2Event>,
    pub live: std::sync::Arc<parking_lot::Mutex<Cs2Live>>,
}

impl Cs2 {
    pub fn new(settings: Cs2Settings, token: String, clips: Sender<AutoClip>, events: Sender<Cs2Event>) -> Self {
        Self { det: Detector::default(), settings, token, clips, events, live: Default::default() }
    }

    fn rule_on(&self, kind: &str) -> bool {
        self.settings.events.get(kind).is_some_and(|r| r.enabled)
    }

    fn rule(&self, kind: &str) -> (f64, f64) {
        self.settings.events.get(kind).map(|r| (r.before, r.after)).unwrap_or((6.0, 2.0))
    }

    fn mode_enabled(&self) -> bool {
        let m = if self.det.mode.is_empty() { "other" } else { self.det.mode.as_str() };
        match self.settings.modes.get(m) {
            Some(v) => *v,
            None => self.settings.modes.get("other").copied().unwrap_or(true),
        }
    }

    /// Modes à rounds : décidé par le MODE de jeu (comme Medal), jamais par un message isolé.
    /// L'échauffement (entraînement, attente sur serveur) se traite comme un deathmatch.
    fn round_based(&self) -> bool {
        if self.det.map_phase == "warmup" {
            return false;
        }
        !matches!(self.det.mode.as_str(), "deathmatch" | "gungameprogressive" | "dm" | "ffa")
    }

    /// Marges autour des kills (réglage « kill » : 5 s avant, 3 s après par défaut).
    fn margins(&self) -> (f64, f64) {
        self.rule("kill")
    }

    /// Règle d'un moment : celle du multi-kill (2K…ACE) s'il en porte un, sinon celle de son type.
    fn rule_key(m: &Moment) -> &str {
        if m.multi.is_empty() { &m.kind } else { &m.multi }
    }

    /// Un événement coché déclenche-t-il un clip ? (headshot, one tap, couteau, 2K, ACE, série…)
    fn event_triggers(&self, m: &Moment) -> bool {
        if m.kind != "kill" && self.rule_on(&m.kind) {
            return true;
        }
        // Un one tap est aussi un headshot
        if m.kind == "onetap" && self.rule_on("headshot") {
            return true;
        }
        !m.multi.is_empty() && self.rule_on(&m.multi)
    }

    /// Un groupe de moments (un round, ou une séquence sans rounds) mérite-t-il un clip ?
    /// Réglable comme avant : « N kills » et chaque événement coché. Le groupe donne au plus
    /// UN clip (jamais deux clips pour le même round). Sans rounds, les événements ne comptent
    /// que si l'option « special_always » est active (sinon le deathmatch HvH clipperait tout).
    fn worth_clip(&self, moments: &[Moment], min_kills: u32, with_events: bool) -> bool {
        let kills = moments.iter().filter(|m| is_kill(&m.kind)).count() as u32;
        if self.rule_on("kill") && kills >= min_kills.max(1) {
            return true;
        }
        // La série de kills est faite pour le deathmatch : elle compte toujours
        moments.iter().any(|m| (with_events || m.kind == "streak") && self.event_triggers(m))
    }

    /// Traite un message JSON envoyé par CS2.
    pub fn handle(&mut self, body: &str) {
        self.handle_at(body, util::now());
    }

    /// Même chose avec une heure imposée (rejeu d'une partie enregistrée, tests).
    pub fn handle_at(&mut self, body: &str, now: f64) {
        let Ok(v) = serde_json::from_str::<Value>(body) else { return };
        let s = |p: &Value| p.as_str().unwrap_or("").to_string();
        let i = |p: &Value| p.as_i64().unwrap_or(0);
        {
            // Le serveur n'écoute que 127.0.0.1 : on accepte même si le jeton diffère (CS2 garde
            // l'ancien fichier en mémoire tant qu'il n'est pas relancé), mais on le signale.
            let mut l = self.live.lock();
            l.last_message = now;
            l.messages += 1;
            l.token_mismatch = !self.token.is_empty() && v["auth"]["token"].as_str() != Some(self.token.as_str());
            l.map = s(&v["map"]["name"]);
            l.mode = s(&v["map"]["mode"]);
            l.map_phase = s(&v["map"]["phase"]);
            l.round_phase = s(&v["round"]["phase"]);
            l.spectating = !v["player"].is_null() && s(&v["provider"]["steamid"]) != s(&v["player"]["steamid"]);
            l.kills = i(&v["player"]["match_stats"]["kills"]);
            l.last_raw = body.chars().take(30_000).collect();
        }

        // Comme Medal : un message sans carte n'est utile que s'il annonce le menu principal,
        // et un message sans bloc « round » est ignoré (il faisait croire à un mode sans rounds).
        if !v["map"].is_object() {
            if v["player"]["activity"].as_str() == Some("menu") {
                self.judge_onetaps(now, true);
                self.close_round(now);
                self.sequence(now, true);
                let d = &mut self.det;
                d.map.clear();
                d.mode.clear();
                d.ammo.clear();
                d.shots.clear();
                d.clear_moments();
            }
            return;
        }
        if !v["round"].is_object() {
            return;
        }
        let map = s(&v["map"]["name"]);
        let mode = s(&v["map"]["mode"]);
        if map != self.det.map || mode != self.det.mode {
            self.judge_onetaps(now, true);
            self.close_round(now);
            self.sequence(now, true);
            self.det.map = map.clone();
            self.det.mode = mode.clone();
            self.det.clear_moments();
            self.det.ammo.clear();
            self.det.shots.clear();
            self.det.steamid.clear();
            let _ = self.events.send(Cs2Event::Connected { map, mode });
        }

        let player = &v["player"];
        let own = !player.is_null() && s(&v["provider"]["steamid"]) == s(&player["steamid"]);
        let mut new_kills = 0i64;
        let mut new_hs = 0i64;
        let mut new_deaths = 0i64;
        let mut new_assists = 0i64;
        let mut collateral = false;
        if own {
            let sid = s(&player["steamid"]);
            let st = &player["match_stats"];
            let state = &player["state"];
            self.det.team = s(&player["team"]);
            if sid != self.det.steamid {
                // Nouveau joueur suivi : on prend ses compteurs comme base
                self.det.steamid = sid;
                self.det.kills = i(&st["kills"]);
                self.det.deaths = i(&st["deaths"]);
                self.det.assists = i(&st["assists"]);
                self.det.round_hs = i(&state["round_killhs"]);
                self.det.round_kills = i(&state["round_kills"]);
                self.det.ammo.clear();
                self.det.shots.clear();
            } else {
                let k = i(&st["kills"]);
                let hs = i(&state["round_killhs"]);
                // Deux compteurs : le total du match (absent à l'échauffement) et celui du round
                let rk_delta = (i(&state["round_kills"]) - self.det.round_kills).max(0);
                new_kills = (k - self.det.kills).max(0).max(rk_delta);
                new_hs = (hs - self.det.round_hs).max(0).min(new_kills);
                new_deaths = (i(&st["deaths"]) - self.det.deaths).max(0);
                new_assists = (i(&st["assists"]) - self.det.assists).max(0);
                self.det.kills = k;
                self.det.round_hs = hs;
                self.det.deaths = i(&st["deaths"]);
                self.det.assists = i(&st["assists"]);
            }
            self.det.round_kills = i(&state["round_kills"]);
            let shots_before = self.det.shots.iter().filter(|(t, _)| *t >= now - 0.15).count() as i64;
            self.track_weapons(&player["weapons"], now);
            let shots_now = self.det.shots.iter().filter(|(t, _)| *t >= now - 0.15).count() as i64;
            let _ = shots_before;
            collateral = new_kills >= 2 && shots_now > 0 && shots_now < new_kills;
        }

        // Phases
        let round_phase = s(&v["round"]["phase"]);
        let map_phase = s(&v["map"]["phase"]);
        let round = i(&v["map"]["round"]);
        if map_phase != self.det.map_phase {
            if self.det.map_phase == "warmup" {
                // Fin de l'échauffement : on clôt la séquence en cours
                self.judge_onetaps(now, true);
                self.sequence(now, true);
                self.det.clear_moments();
            }
            if map_phase == "gameover" || map_phase == "intermission" {
                self.judge_onetaps(now, true);
                self.close_round(now);
            }
            self.det.map_phase = map_phase;
        }
        if round_phase != self.det.round_phase {
            if round_phase == "live" {
                self.det.round_start = now;
            }
            if round_phase == "over" || round_phase == "freezetime" {
                self.judge_onetaps(now, true);
                let won = s(&v["round"]["win_team"]);
                let alive = i(&player["state"]["health"]) > 0;
                self.mark_clutch(now, own && !won.is_empty() && won == self.det.team && alive);
                self.close_round(now);
            }
            self.det.round_phase = round_phase;
        }
        if round < self.det.round {
            self.det.clear_moments(); // match relancé
        }
        self.det.round = round;

        if !own {
            self.judge_onetaps(now, false);
            self.sequence(now, false);
            return;
        }

        // Création des moments
        let weapon = self.det.weapon.clone();
        let wtype = self.det.weapon_type.clone();
        let mut fresh: Vec<Moment> = Vec::new();
        let base_kills = self.det.round_kills - new_kills;
        for n in 0..new_kills {
            let hs = n < new_hs;
            let kind = if collateral {
                "collateral"
            } else if wtype.eq_ignore_ascii_case("knife") {
                "knife"
            } else if weapon == "weapon_taser" {
                "zeus"
            } else if hs {
                "headshot"
            } else {
                "kill"
            };
            let count = base_kills + n + 1;
            let multi = match count {
                2 => "2k",
                3 => "3k",
                4 => "4k",
                c if c >= 5 => "ace",
                _ => "",
            };
            fresh.push(Moment {
                time: now,
                kind: kind.into(),
                multi: if self.round_based() { multi.into() } else { String::new() },
                weapon: weapon.trim_start_matches("weapon_").to_string(),
                round,
                count: 0,
            });
        }
        for _ in 0..new_deaths {
            fresh.push(Moment { time: now, kind: "death".into(), multi: String::new(), weapon: String::new(), round, count: 0 });
        }
        for _ in 0..new_assists {
            fresh.push(Moment { time: now, kind: "assist".into(), multi: String::new(), weapon: String::new(), round, count: 0 });
        }
        for m in fresh {
            let idx = self.det.round_moments.len();
            if m.kind == "headshot" {
                let class = weapon_class(&weapon, &wtype);
                if let Some(shot) = self.attributed_shot(now, &class) {
                    self.det.pending.push((idx, shot, class));
                }
            }
            let _ = self.events.send(Cs2Event::Moment(m.clone()));
            let killed = is_kill(&m.kind);
            self.det.round_moments.push(m);
            if killed {
                self.bump_streak(now, round);
            }
        }
        self.judge_onetaps(now, false);
        self.sequence(now, false);
    }

    /// Série de kills : N kills avec moins de X secondes entre chacun.
    fn bump_streak(&mut self, now: f64, round: i64) {
        let window = self.settings.streak_window.max(1.0);
        if now - self.det.streak_last > window {
            self.det.streak_len = 0;
            self.det.streak_idx = None;
        }
        self.det.streak_last = now;
        self.det.streak_len += 1;
        let len = self.det.streak_len;
        // Série décochée dans les réglages : pas de moment « série » (ni clip, ni titre « Série de N »)
        if len < self.settings.streak_count.max(2) || !self.rule_on("streak") {
            return;
        }
        match self.det.streak_idx {
            Some(i) if i < self.det.round_moments.len() => {
                let m = &mut self.det.round_moments[i];
                m.count = len;
                m.time = now;
            }
            _ => {
                self.det.streak_idx = Some(self.det.round_moments.len());
                let m = Moment { time: now, kind: "streak".into(), multi: String::new(), weapon: String::new(), round, count: len };
                let _ = self.events.send(Cs2Event::Moment(m.clone()));
                self.det.round_moments.push(m);
            }
        }
        // Si le clip instantané contient déjà la série, on la prolonge
        let after = self.rule("streak").1;
        if let Some((_, e, list)) = self.det.open_clip.as_mut()
            && let Some(m) = list.iter_mut().find(|m| m.kind == "streak")
        {
            m.count = len;
            m.time = now;
            *e = e.max(now + after);
        }
    }

    /// Appelé régulièrement même sans message (jugement différé des one-taps, séquences).
    pub fn tick(&mut self) {
        self.tick_at(util::now());
    }

    pub fn tick_at(&mut self, now: f64) {
        self.judge_onetaps(now, false);
        self.sequence(now, false);
        // Un round bloqué plus de 4 minutes (serveur perso sans fin de round) : on n'attend plus
        if self.round_based() && self.det.round_moments.last().is_some_and(|m| now - m.time > 240.0) {
            self.judge_onetaps(now, true);
            self.close_round(now);
        }
    }

    fn track_weapons(&mut self, weapons: &Value, now: f64) {
        let Some(map) = weapons.as_object() else { return };
        let mut carried = Vec::new();
        for w in map.values() {
            let name = w["name"].as_str().unwrap_or("").to_string();
            let kind = w["type"].as_str().unwrap_or("").to_string();
            if w["state"].as_str() == Some("active") {
                // On attend 0,7 s avant de considérer une nouvelle arme « en main » (temps de sortie)
                if self.det.weapon.is_empty() || name == self.det.weapon {
                    self.det.weapon = name.clone();
                    self.det.weapon_type = kind.clone();
                    self.det.drawing.clear();
                } else {
                    if name != self.det.drawing {
                        self.det.drawing = name.clone();
                        self.det.drawing_type = kind.clone();
                        self.det.drawing_since = now;
                    }
                    if now - self.det.drawing_since >= 0.7 {
                        self.det.weapon = std::mem::take(&mut self.det.drawing);
                        self.det.weapon_type = std::mem::take(&mut self.det.drawing_type);
                    }
                }
            }
            let Some(clip) = w["ammo_clip"].as_i64() else { continue };
            carried.push(name.clone());
            if let Some(prev) = self.det.ammo.get(&name).copied()
                && clip < prev
            {
                // Des balles en moins : ce sont des tirs (et l'arme est forcément en main)
                self.det.weapon = name.clone();
                self.det.weapon_type = kind.clone();
                let class = weapon_class(&name, &kind);
                for _ in 0..(prev - clip).min(30) {
                    self.det.shots.push((now, class.clone()));
                }
            }
            self.det.ammo.insert(name, clip);
        }
        self.det.ammo.retain(|k, _| carried.contains(k));
        self.det.shots.retain(|(t, _)| now - t < 3.0);
    }

    fn attributed_shot(&self, kill_time: f64, class: &str) -> Option<f64> {
        onetap_window(class, !self.settings.onetap_ignore_snipers)?;
        self.det
            .shots
            .iter()
            .rev()
            .find(|(t, c)| *t <= kill_time + 0.01 && kill_time - t <= 0.4 && c == class)
            .map(|(t, _)| *t)
    }

    /// Un headshot devient « one tap » si son tir est isolé (pas d'autre tir juste avant ni juste après).
    fn judge_onetaps(&mut self, now: f64, force: bool) {
        let snipers = !self.settings.onetap_ignore_snipers;
        let mut keep = Vec::new();
        let pending = std::mem::take(&mut self.det.pending);
        for (idx, shot, class) in pending {
            let Some((before, after)) = onetap_window(&class, snipers) else { continue };
            if !force && now < shot + after {
                keep.push((idx, shot, class));
                continue;
            }
            let others = self
                .det
                .shots
                .iter()
                .filter(|(t, _)| (*t - shot).abs() > 0.001 && *t >= shot - before && *t <= shot + after)
                .count();
            let same_time = self.det.shots.iter().filter(|(t, _)| (*t - shot).abs() <= 0.001).count();
            if others == 0 && same_time <= 1 {
                if let Some(m) = self.det.round_moments.get_mut(idx)
                    && m.kind == "headshot"
                {
                    m.kind = "onetap".into();
                    let _ = self.events.send(Cs2Event::Moment(m.clone()));
                }
                if let Some(c) = self.det.open_clip.as_mut() {
                    for m in c.2.iter_mut() {
                        if (m.time - shot).abs() < 0.5 && m.kind == "headshot" {
                            m.kind = "onetap".into();
                        }
                    }
                }
            }
        }
        self.det.pending = keep;
    }

    /// Clutch « probable » : round gagné, joueur en vie, ≥ 2 kills, tous dans les 40 dernières secondes.
    fn mark_clutch(&mut self, now: f64, won_alive: bool) {
        if !won_alive || !self.rule_on("clutch") {
            return;
        }
        let kills: Vec<&Moment> = self.det.round_moments.iter().filter(|m| is_kill(&m.kind)).collect();
        if kills.len() >= 2 && kills.iter().all(|m| now - m.time <= 40.0) {
            let round = self.det.round;
            self.det.round_moments.push(Moment {
                time: now,
                kind: "clutch".into(),
                multi: String::new(),
                weapon: String::new(),
                round,
                count: 0,
            });
        }
    }

    /// Fin de round : UN seul clip pour tout le round, s'il le mérite (≥ N kills, clutch…).
    fn close_round(&mut self, _now: f64) {
        let moments = std::mem::take(&mut self.det.round_moments);
        self.det.clear_moments();
        if !self.settings.enabled || !self.mode_enabled() || !self.round_based() {
            return;
        }
        if !self.worth_clip(&moments, self.settings.min_round_kills, true) {
            return;
        }
        // Un seul clip couvrant toutes les actions du round, avec les marges de chaque événement
        let relevant: Vec<&Moment> = moments
            .iter()
            .filter(|m| is_kill(&m.kind) || matches!(m.kind.as_str(), "clutch" | "streak") || self.event_triggers(m))
            .collect();
        if relevant.is_empty() {
            return;
        }
        let start = relevant.iter().map(|m| m.time - self.rule(Self::rule_key(m)).0).fold(f64::MAX, f64::min);
        let end = relevant.iter().map(|m| m.time + self.rule(Self::rule_key(m)).1).fold(f64::MIN, f64::max);
        let top = moments.iter().filter(|m| !m.multi.is_empty()).map(|m| m.multi.clone()).last().unwrap_or_default();
        self.send_clip(start, end, &moments, &top);
    }

    /// Sans rounds (deathmatch, échauffement) : les kills espacés de moins de « merge_gap » s forment
    /// une séquence ; elle devient un clip si elle compte assez de kills. Durée plafonnée.
    fn sequence(&mut self, now: f64, force: bool) {
        if !self.settings.enabled || !self.mode_enabled() {
            self.det.open_clip = None;
            return;
        }
        let gap = self.settings.merge_gap.max(1.0);
        let (before, after) = self.margins();
        let max_len = self.settings.max_sequence.max(10.0);
        if !self.round_based() {
            // Nouveaux moments, sans dépasser un headshot dont le one-tap n'est pas encore jugé
            let limit = self.det.pending.iter().map(|(i, _, _)| *i).min().unwrap_or(usize::MAX).min(self.det.round_moments.len());
            let from = self.det.seen.min(limit);
            let fresh: Vec<Moment> = self.det.round_moments[from..limit].to_vec();
            self.det.seen = limit;
            for m in fresh {
                let kill = is_kill(&m.kind) || m.kind == "streak";
                let last_kill = self.det.open_clip.as_ref().and_then(|(_, _, l)| l.iter().filter(|x| is_kill(&x.kind)).map(|x| x.time).reduce(f64::max));
                let first_kill = self.det.open_clip.as_ref().and_then(|(_, _, l)| l.iter().filter(|x| is_kill(&x.kind)).map(|x| x.time).reduce(f64::min));
                let fits = match (last_kill, first_kill) {
                    (Some(l), Some(f)) => m.time - l <= gap && (m.time - f) + before + after <= max_len,
                    _ => false,
                };
                if self.det.open_clip.is_some() && (fits || !kill) {
                    let (_, e, list) = self.det.open_clip.as_mut().unwrap();
                    if kill {
                        *e = e.max(m.time + after);
                    }
                    list.push(m);
                } else if kill {
                    if let Some((st, e, list)) = self.det.open_clip.take() {
                        self.finish_sequence(st, e, list);
                    }
                    self.det.open_clip = Some((m.time - before, m.time + after, vec![m]));
                }
            }
            if self.det.round_moments.len() > 300 && self.det.pending.is_empty() {
                self.det.clear_moments();
            }
        }
        let ready = self.det.open_clip.as_ref().is_some_and(|(_, _, list)| {
            let last = list.iter().filter(|x| is_kill(&x.kind) || x.kind == "streak").map(|x| x.time).fold(f64::MIN, f64::max);
            force || now >= last + gap
        });
        if ready {
            let (st, e, list) = self.det.open_clip.take().unwrap();
            self.finish_sequence(st, e, list);
        }
    }

    fn finish_sequence(&self, start: f64, end: f64, list: Vec<Moment>) {
        if self.worth_clip(&list, self.settings.min_sequence_kills, self.settings.special_always) {
            self.send_clip(start, end, &list, "");
        }
    }

    fn send_clip(&self, start: f64, end: f64, moments: &[Moment], multi: &str) {
        // Tous les moments de la période deviennent des marqueurs (pas seulement ceux qui ont déclenché le clip)
        let mut all: Vec<Moment> = moments.to_vec();
        for m in &self.det.round_moments {
            if m.time >= start && m.time <= end && !all.iter().any(|x| x.time == m.time && x.kind == m.kind) {
                all.push(m.clone());
            }
        }
        all.sort_by(|a, b| a.time.total_cmp(&b.time));
        let title = clip_title(&all, multi);
        let markers = all
            .iter()
            .map(|m| {
                let mut l = label(&m.kind).to_string();
                if m.kind == "streak" {
                    l = format!("Série de {} kills", m.count);
                } else if !m.weapon.is_empty() && is_kill(&m.kind) {
                    l = format!("{l} · {}", m.weapon.to_uppercase());
                }
                (m.time, m.kind.clone(), l)
            })
            .collect();
        let _ = self.events.send(Cs2Event::Clip { title: title.clone() });
        let _ = self.clips.send(AutoClip { start, end: end.max(start + 2.0), title, markers });
    }
}

/// Titre : le meilleur moment, puis un détail. Ex. « 2K · One Tap », « ACE · Clutch », « Série de 5 ».
/// Jamais deux clips « Headshot » et « 2K » pour le même round : un seul titre résume tout.
pub fn clip_title(moments: &[Moment], multi: &str) -> String {
    let kills = moments.iter().filter(|m| is_kill(&m.kind)).count();
    let has = |k: &str| moments.iter().any(|m| m.kind == k);
    let rank = |m: &str| ["2k", "3k", "4k", "ace"].iter().position(|x| *x == m);
    let top = if multi.is_empty() {
        moments.iter().filter_map(|m| rank(&m.multi).map(|r| (r, m.multi.clone()))).max().map(|(_, m)| m).unwrap_or_default()
    } else {
        multi.to_string()
    };
    let streak = moments.iter().filter(|m| m.kind == "streak").map(|m| m.count).max();
    // 1) le meilleur
    let best: String = if matches!(top.as_str(), "ace" | "4k" | "3k") {
        label(&top).into()
    } else if let Some(n) = streak {
        format!("Série de {n}")
    } else if top == "2k" {
        "2K".into()
    } else if kills >= 2 {
        format!("{kills} kills")
    } else if let Some(k) = ["collateral", "onetap", "knife", "zeus", "headshot", "kill"].iter().find(|k| has(k)) {
        label(k).into()
    } else if has("death") {
        "Mort".into()
    } else if has("assist") {
        "Assist".into()
    } else {
        "Counter-Strike 2".into()
    };
    let mut parts = vec![best.clone()];
    // 2) le clutch passe toujours en premier détail
    if has("clutch") {
        parts.push("Clutch".into());
    }
    // 3) un détail : l'action la plus rare, sinon la part de headshots
    let detail = ["collateral", "onetap", "knife", "zeus"]
        .iter()
        .find(|k| has(k) && label(k) != best)
        .map(|k| {
            let n = moments.iter().filter(|m| m.kind == **k).count();
            let n = if *k == "collateral" { 1 } else { n };
            if n > 1 { format!("{n}× {}", label(k)) } else { label(k).to_string() }
        })
        .or_else(|| {
            let hs = moments.iter().filter(|m| matches!(m.kind.as_str(), "headshot" | "onetap")).count();
            match (kills, hs) {
                (k, h) if k >= 2 && h == k => Some("100 % HS".into()),
                (k, 1) if k >= 2 => Some("Headshot".into()),
                (k, h) if k >= 2 && h > 0 => Some(format!("{h} HS")),
                _ => None,
            }
        });
    if let Some(d) = detail
        && parts.len() < 3
    {
        parts.push(d);
    }
    parts.join(" · ")
}

// ---------------------------------------------------------------------------
// Installation du fichier GSI dans CS2
// ---------------------------------------------------------------------------

/// Dossiers `…\game\csgo\cfg` de toutes les installations de CS2 trouvées.
pub fn cs2_cfg_dirs() -> Vec<PathBuf> {
    let mut libraries: Vec<PathBuf> = Vec::new();
    let steam = steam_path();
    if let Some(steam) = &steam {
        libraries.push(steam.clone());
        let vdf = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")).unwrap_or_default();
        for line in vdf.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("\"path\"") {
                let p = rest.trim().trim_matches('"').replace("\\\\", "\\");
                libraries.push(PathBuf::from(p));
            }
        }
    }
    for drive in b'C'..=b'Z' {
        for name in ["SteamLibrary", "Steam", "Program Files (x86)\\Steam", "Games\\Steam"] {
            libraries.push(PathBuf::from(format!("{}:\\{name}", drive as char)));
        }
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for lib in libraries {
        let cfg = lib.join("steamapps").join("common").join("Counter-Strike Global Offensive").join("game").join("csgo").join("cfg");
        if cfg.is_dir() && !out.iter().any(|o| o.to_string_lossy().eq_ignore_ascii_case(&cfg.to_string_lossy())) {
            out.push(cfg);
        }
    }
    out
}

fn steam_path() -> Option<PathBuf> {
    let out = util::command(std::path::Path::new("reg"))
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("SteamPath"))?;
    let value = line.split("REG_SZ").nth(1)?.trim().replace('/', "\\");
    Some(PathBuf::from(value))
}

/// Écrit `gamestate_integration_rewind.cfg` dans chaque installation. Renvoie les dossiers modifiés.
pub fn install_config(port: u16, token: &str) -> Vec<String> {
    let content = format!(
        "\"Rewind GSI\"\n{{\n\t\"uri\"\t\"http://127.0.0.1:{port}/\"\n\t\"timeout\"\t\"0.5\"\n\t\"buffer\"\t\"0.0\"\n\t\"throttle\"\t\"0.05\"\n\t\"heartbeat\"\t\"20\"\n\t\"auth\"\n\t{{\n\t\t\"token\"\t\"{token}\"\n\t}}\n\t\"data\"\n\t{{\n\t\t\"provider\"\t\"1\"\n\t\t\"map\"\t\"1\"\n\t\t\"round\"\t\"1\"\n\t\t\"player_id\"\t\"1\"\n\t\t\"player_state\"\t\"1\"\n\t\t\"player_match_stats\"\t\"1\"\n\t\t\"player_weapons\"\t\"1\"\n\t}}\n}}\n"
    );
    let mut done = Vec::new();
    for dir in cs2_cfg_dirs() {
        let file = dir.join("gamestate_integration_rewind.cfg");
        let same = std::fs::read_to_string(&file).map(|t| t == content).unwrap_or(false);
        if same || std::fs::write(&file, &content).is_ok() {
            done.push(file.to_string_lossy().to_string());
        }
    }
    done
}

/// Jeton stable propre à ce PC et à cet utilisateur (identifie les messages de CS2).
/// Toujours le même : réinstaller Rewind ne change pas le fichier posé dans CS2.
pub fn token() -> String {
    let seed = format!(
        "rewind|{}|{}",
        std::env::var("COMPUTERNAME").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    // FNV-1a 64 bits (stable d'une version de Rust à l'autre)
    let mut h: u64 = 0xcbf29ce484222325;
    for b in seed.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("rewind-{h:016x}")
}

/// Journal local de tout ce que CS2 envoie (un fichier .jsonl par jour, gardé 3 jours).
/// Sert à rejouer exactement une partie pour comprendre un clip manqué ou en trop.
struct Journal {
    day: String,
    file: Option<std::fs::File>,
}

impl Journal {
    fn dir() -> PathBuf {
        let d = util::data_dir().join("cs2-journal");
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn open() -> Self {
        // Ménage : plus de 3 jours
        if let Ok(entries) = std::fs::read_dir(Self::dir()) {
            for e in entries.flatten() {
                let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a.as_secs() > 3 * 86400);
                if old {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        Self { day: String::new(), file: None }
    }

    fn write(&mut self, body: &str) {
        use std::io::Write;
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        if day != self.day || self.file.is_none() {
            self.day = day.clone();
            self.file = std::fs::OpenOptions::new().create(true).append(true).open(Self::dir().join(format!("{day}.jsonl"))).ok();
        }
        let compact: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{{\"t\":{:.3},\"body\":{}}}", util::now(), if compact.starts_with('{') { compact } else { "null".into() });
        }
    }
}

/// Serveur HTTP local qui reçoit les messages de CS2. Tourne dans son propre fil.
pub fn serve(port: u16, mut cs2: Cs2, settings_rx: std::sync::mpsc::Receiver<Cs2Settings>) {
    std::thread::Builder::new()
        .name("cs2-gsi".into())
        .spawn(move || {
            let server = match tiny_http::Server::http(("127.0.0.1", port)) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("Serveur CS2 impossible sur le port {port} : {e}");
                    return;
                }
            };
            let mut journal = Journal::open();
            loop {
                while let Ok(s) = settings_rx.try_recv() {
                    cs2.settings = s;
                }
                match server.recv_timeout(Duration::from_millis(150)) {
                    Ok(Some(mut request)) => {
                        let mut body = String::new();
                        let _ = request.as_reader().read_to_string(&mut body);
                        journal.write(&body);
                        cs2.handle(&body);
                        let _ = request.respond(tiny_http::Response::from_string("OK"));
                    }
                    Ok(None) => cs2.tick(),
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        })
        .ok();
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread::sleep;

    const ME: &str = "76561198000000001";

    struct P {
        kills: i64,
        hs: i64,
        round_kills: i64,
        deaths: i64,
        weapon: &'static str,
        wtype: &'static str,
        ammo: i64,
        phase: &'static str,
        mode: &'static str,
        provider: &'static str,
        win: &'static str,
        map_phase: &'static str,
    }

    impl Default for P {
        fn default() -> Self {
            Self { kills: 0, hs: 0, round_kills: 0, deaths: 0, weapon: "weapon_ak47", wtype: "Rifle", ammo: 30, phase: "live", mode: "competitive", provider: ME, win: "", map_phase: "live" }
        }
    }

    fn payload(p: &P) -> String {
        serde_json::json!({
            "provider": { "steamid": p.provider },
            "map": { "name": "de_mirage", "mode": p.mode, "phase": p.map_phase, "round": 3 },
            "round": { "phase": p.phase, "win_team": p.win },
            "player": {
                "steamid": ME, "team": "T",
                "state": { "health": 100, "round_kills": p.round_kills, "round_killhs": p.hs },
                "match_stats": { "kills": p.kills, "deaths": p.deaths, "assists": 0 },
                "weapons": { "weapon_0": { "name": p.weapon, "type": p.wtype, "state": "active", "ammo_clip": p.ammo } }
            }
        })
        .to_string()
    }

    fn setup(timing: &str) -> (Cs2, mpsc::Receiver<AutoClip>) {
        let (tx, rx) = mpsc::channel();
        let (etx, _erx) = mpsc::channel();
        let mut s = Cs2Settings::default();
        s.timing = timing.into();
        for r in s.events.values_mut() {
            r.after = 0.2;
            r.before = 1.0;
        }
        s.merge_gap = 1.0;
        s.min_round_kills = 1;
        s.min_sequence_kills = 1;
        (Cs2::new(s, String::new(), tx, etx), rx)
    }

    #[test]
    fn round_avec_3k_onetap_et_clutch() {
        let (mut cs, rx) = setup("round");
        cs.settings.events.get_mut("clutch").unwrap().enabled = true;
        cs.handle(&payload(&P::default()));
        sleep(Duration::from_millis(700));
        // une seule balle, headshot → one tap
        cs.handle(&payload(&P { kills: 1, hs: 1, round_kills: 1, ammo: 29, ..Default::default() }));
        sleep(Duration::from_millis(700));
        // rafale de 4 balles, kill simple
        cs.handle(&payload(&P { kills: 2, hs: 1, round_kills: 2, ammo: 25, ..Default::default() }));
        sleep(Duration::from_millis(700));
        // rafale de 3 balles, headshot (pas un one tap)
        cs.handle(&payload(&P { kills: 3, hs: 2, round_kills: 3, ammo: 22, ..Default::default() }));
        sleep(Duration::from_millis(100));
        cs.handle(&payload(&P { kills: 3, hs: 2, round_kills: 3, ammo: 22, phase: "over", win: "T", ..Default::default() }));
        let clip = rx.try_recv().expect("un clip à la fin du round");
        println!("titre : {}", clip.title);
        let kinds: Vec<&str> = clip.markers.iter().map(|m| m.1.as_str()).collect();
        println!("moments : {kinds:?}");
        assert!(clip.title.contains("3K"), "{}", clip.title);
        assert!(kinds.contains(&"onetap"));
        assert!(kinds.contains(&"headshot"));
        assert!(kinds.contains(&"clutch"));
        assert_eq!(kinds.iter().filter(|k| **k == "onetap").count(), 1);
        assert!(clip.end > clip.start + 1.5);
        assert!(rx.try_recv().is_err(), "un seul clip par round");
    }

    #[test]
    fn collateral_awp() {
        let (mut cs, rx) = setup("round");
        cs.handle(&payload(&P { weapon: "weapon_awp", wtype: "SniperRifle", ammo: 5, ..Default::default() }));
        sleep(Duration::from_millis(200));
        cs.handle(&payload(&P { kills: 2, hs: 0, round_kills: 2, weapon: "weapon_awp", wtype: "SniperRifle", ammo: 4, ..Default::default() }));
        cs.handle(&payload(&P { kills: 2, round_kills: 2, weapon: "weapon_awp", wtype: "SniperRifle", ammo: 4, phase: "over", win: "CT", ..Default::default() }));
        let clip = rx.try_recv().expect("clip");
        println!("titre : {}", clip.title);
        assert!(clip.title.contains("Collateral"));
        assert!(clip.title.contains("2K"));
    }

    #[test]
    fn spectateur_ignore() {
        let (mut cs, rx) = setup("round");
        let other = "76561198000000099";
        cs.handle(&payload(&P { provider: other, ..Default::default() }));
        cs.handle(&payload(&P { provider: other, kills: 1, hs: 1, round_kills: 1, ammo: 29, ..Default::default() }));
        cs.handle(&payload(&P { provider: other, kills: 1, phase: "over", win: "T", ..Default::default() }));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn deathmatch_instantane() {
        let (mut cs, rx) = setup("round");
        cs.settings.modes.insert("deathmatch".into(), true);
        let dm = |p: P| payload(&P { mode: "deathmatch", ..p });
        cs.handle(&dm(P::default()));
        sleep(Duration::from_millis(700));
        cs.handle(&dm(P { kills: 1, hs: 1, round_kills: 1, ammo: 27, ..Default::default() }));
        // pas de rounds en deathmatch : le clip part tout seul après la fenêtre « après »
        let mut got = None;
        for _ in 0..30 {
            sleep(Duration::from_millis(100));
            cs.tick();
            if let Ok(c) = rx.try_recv() {
                got = Some(c);
                break;
            }
        }
        let clip = got.expect("clip instantané en deathmatch");
        println!("titre : {}", clip.title);
        assert!(clip.title.contains("Headshot"));
    }

    #[test]
    fn echauffement_entrainement() {
        // En échauffement (entraînement, attente serveur), le total du match ne bouge pas :
        // seul le compteur du round augmente. Le clip doit partir en direct, sans fin de round.
        let (mut cs, rx) = setup("round");
        let w = |p: P| payload(&P { map_phase: "warmup", ..p });
        cs.handle(&w(P::default()));
        sleep(Duration::from_millis(700));
        cs.handle(&w(P { kills: 0, hs: 1, round_kills: 1, ammo: 29, ..Default::default() }));
        let mut got = None;
        for _ in 0..30 {
            sleep(Duration::from_millis(100));
            cs.tick();
            if let Ok(c) = rx.try_recv() {
                got = Some(c);
                break;
            }
        }
        let clip = got.expect("clip pendant l'échauffement");
        println!("titre : {}", clip.title);
    }

    #[test]
    fn serie_de_kills_deathmatch() {
        let (mut cs, rx) = setup("round");
        cs.settings.events.get_mut("kill").unwrap().enabled = false;
        cs.settings.events.get_mut("headshot").unwrap().enabled = false;
        cs.settings.events.get_mut("onetap").unwrap().enabled = false;
        let dm = |p: P| payload(&P { mode: "deathmatch", ..p });
        cs.handle(&dm(P::default()));
        for k in 1..=3 {
            sleep(Duration::from_millis(300));
            cs.handle(&dm(P { kills: k, round_kills: k, ammo: 30 - 3 * k, ..Default::default() }));
        }
        let mut got = None;
        for _ in 0..30 {
            sleep(Duration::from_millis(100));
            cs.tick();
            if let Ok(c) = rx.try_recv() {
                got = Some(c);
                break;
            }
        }
        let clip = got.expect("clip de série");
        println!("titre : {} ({:.1} s)", clip.title, clip.end - clip.start);
        assert!(clip.title.contains("Série de 3"), "{}", clip.title);
        assert_eq!(clip.markers.iter().filter(|m| m.1 == "kill").count(), 3, "les 3 kills sont marqués");
    }

    #[test]
    fn un_seul_clip_par_round() {
        // Headshot puis kill simple, espacés : aucun clip pendant le round, UN clip à la fin, titré 2K
        let (mut cs, rx) = setup("round");
        cs.handle(&payload(&P::default()));
        sleep(Duration::from_millis(700));
        cs.handle(&payload(&P { kills: 1, hs: 1, round_kills: 1, ammo: 27, ..Default::default() }));
        for _ in 0..15 {
            sleep(Duration::from_millis(100));
            cs.tick();
        }
        cs.handle(&payload(&P { kills: 2, hs: 1, round_kills: 2, ammo: 22, ..Default::default() }));
        for _ in 0..15 {
            sleep(Duration::from_millis(100));
            cs.tick();
        }
        assert!(rx.try_recv().is_err(), "aucun clip avant la fin du round");
        cs.handle(&payload(&P { kills: 2, hs: 1, round_kills: 2, ammo: 22, phase: "over", win: "CT", ..Default::default() }));
        let clip = rx.try_recv().expect("un clip à la fin du round");
        println!("titre : {}", clip.title);
        assert!(clip.title.starts_with("2K"), "{}", clip.title);
        assert!(rx.try_recv().is_err(), "un seul clip");
    }

    /// Réglages réels par défaut (3 kills mini) avec des marges courtes pour aller vite.
    fn setup_defaut() -> (Cs2, mpsc::Receiver<AutoClip>) {
        let (tx, rx) = mpsc::channel();
        let (etx, _erx) = mpsc::channel();
        let mut s = Cs2Settings::default();
        s.merge_gap = 1.0;
        (Cs2::new(s, String::new(), tx, etx), rx)
    }

    /// Le bug du 2026-11-13 : un message sans bloc « round » au milieu du round faisait partir
    /// un clip du premier kill seul, puis un second clip à la fin du round.
    #[test]
    fn message_sans_round_ne_coupe_pas_le_round() {
        let (mut cs, rx) = setup("round");
        let wingman = |p: P| payload(&P { mode: "scrimcomp2v2", ..p });
        cs.handle(&wingman(P::default()));
        sleep(Duration::from_millis(300));
        cs.handle(&wingman(P { kills: 1, round_kills: 1, ammo: 25, ..Default::default() }));
        // message incomplet : pas de bloc round
        let mut v: serde_json::Value = serde_json::from_str(&wingman(P { kills: 1, round_kills: 1, ammo: 25, ..Default::default() })).unwrap();
        v.as_object_mut().unwrap().remove("round");
        cs.handle(&v.to_string());
        for _ in 0..25 {
            sleep(Duration::from_millis(100));
            cs.tick();
        }
        assert!(rx.try_recv().is_err(), "aucun clip au milieu du round");
        cs.handle(&wingman(P { kills: 2, hs: 1, round_kills: 2, ammo: 20, ..Default::default() }));
        for _ in 0..15 {
            sleep(Duration::from_millis(100));
            cs.tick();
        }
        assert!(rx.try_recv().is_err(), "toujours aucun clip avant la fin du round");
        cs.handle(&wingman(P { kills: 2, hs: 1, round_kills: 2, ammo: 20, phase: "over", win: "CT", ..Default::default() }));
        let clip = rx.try_recv().expect("un clip à la fin du round");
        assert_eq!(clip.markers.iter().filter(|m| is_kill(&m.1)).count(), 2, "les deux kills dans le même clip");
        assert!(rx.try_recv().is_err(), "un seul clip");
    }

    #[test]
    fn seuil_trois_kills_par_round() {
        let (mut cs, rx) = setup_defaut();
        cs.handle(&payload(&P::default()));
        cs.handle(&payload(&P { kills: 1, round_kills: 1, ammo: 25, ..Default::default() }));
        cs.handle(&payload(&P { kills: 2, round_kills: 2, ammo: 20, ..Default::default() }));
        cs.handle(&payload(&P { kills: 2, round_kills: 2, ammo: 20, phase: "over", win: "T", ..Default::default() }));
        assert!(rx.try_recv().is_err(), "2 kills : pas de clip");
        cs.handle(&payload(&P { kills: 2, round_kills: 0, ammo: 30, phase: "live", ..Default::default() }));
        for k in 1..=3 {
            cs.handle(&payload(&P { kills: 2 + k, round_kills: k, ammo: 30 - 5 * k, ..Default::default() }));
        }
        cs.handle(&payload(&P { kills: 5, round_kills: 3, ammo: 15, phase: "over", win: "T", ..Default::default() }));
        let clip = rx.try_recv().expect("3 kills : un clip");
        println!("titre : {}", clip.title);
        assert!(clip.title.starts_with("3K"), "{}", clip.title);
        assert!(rx.try_recv().is_err());
    }

    /// CS2 normal (compétitif) avec les réglages par défaut : un one tap seul fait un clip,
    /// et un round avec one tap + headshot + 2 kills ne fait toujours qu'UN clip.
    #[test]
    fn cs2_normal_un_clip_par_round_avec_evenements() {
        let (mut cs, rx) = setup_defaut();
        cs.handle(&payload(&P::default()));
        // One tap : une seule balle tirée, kill en headshot, puis plus aucun tir
        cs.handle(&payload(&P { kills: 1, hs: 1, round_kills: 1, ammo: 29, ..Default::default() }));
        sleep(Duration::from_millis(1200));
        cs.tick();
        cs.handle(&payload(&P { kills: 1, hs: 1, round_kills: 1, ammo: 29, phase: "over", win: "T", ..Default::default() }));
        let clip = rx.try_recv().expect("one tap seul : un clip");
        println!("round 1 : {}", clip.title);
        assert!(clip.title.contains("One Tap"), "{}", clip.title);
        assert!(rx.try_recv().is_err(), "un seul clip");

        // Round 2 : one tap, puis un headshot en rafale, puis un kill : 3 actions, UN clip
        cs.handle(&payload(&P { kills: 1, hs: 1, round_kills: 0, ammo: 30, phase: "live", ..Default::default() }));
        cs.handle(&payload(&P { kills: 2, hs: 2, round_kills: 1, ammo: 29, ..Default::default() }));
        sleep(Duration::from_millis(1200));
        cs.tick();
        cs.handle(&payload(&P { kills: 3, hs: 3, round_kills: 2, ammo: 20, ..Default::default() }));
        cs.handle(&payload(&P { kills: 3, hs: 3, round_kills: 2, ammo: 12, ..Default::default() }));
        for _ in 0..12 {
            sleep(Duration::from_millis(100));
            cs.tick();
        }
        assert!(rx.try_recv().is_err(), "rien pendant le round");
        cs.handle(&payload(&P { kills: 3, hs: 3, round_kills: 2, ammo: 12, phase: "over", win: "T", ..Default::default() }));
        let clip = rx.try_recv().expect("round 2 : un clip");
        println!("round 2 : {} ({} repères)", clip.title, clip.markers.len());
        assert!(rx.try_recv().is_err(), "jamais deux clips pour le même round");
        assert!(clip.markers.iter().any(|m| m.1 == "onetap"), "le one tap est marqué");
    }

    /// Les réglages par événement : headshot et 2K coupés par défaut, actifs si on les coche.
    #[test]
    fn reglages_headshot_et_2k() {
        let mut total = 0i64;
        let mut round = |cs: &mut Cs2, rk: i64, hs: i64| {
            cs.handle(&payload(&P { kills: total, phase: "live", ..Default::default() }));
            for k in 1..=rk {
                // Rafale : plusieurs balles par kill (pas de one tap)
                cs.handle(&payload(&P { kills: total + k, hs: hs.min(k), round_kills: k, ammo: 30 - 6 * k, ..Default::default() }));
            }
            total += rk;
            cs.handle(&payload(&P { kills: total, hs: hs.min(rk), round_kills: rk, ammo: 30 - 6 * rk, phase: "over", win: "T", ..Default::default() }));
        };
        let (mut cs, rx) = setup_defaut();
        // Rounds enchaînés en quelques millisecondes : la « série » n'a pas de sens ici
        cs.settings.events.get_mut("streak").unwrap().enabled = false;
        round(&mut cs, 1, 1);
        assert!(rx.try_recv().is_err(), "headshot seul, réglage coupé : pas de clip");
        cs.settings.events.get_mut("headshot").unwrap().enabled = true;
        round(&mut cs, 1, 1);
        assert!(rx.try_recv().is_ok(), "headshot coché : un clip");
        round(&mut cs, 2, 0);
        assert!(rx.try_recv().is_err(), "2 kills sous le seuil de 3, 2K coupé : pas de clip");
        cs.settings.events.get_mut("2k").unwrap().enabled = true;
        round(&mut cs, 2, 0);
        let c = rx.try_recv().expect("2K coché : un clip");
        assert!(c.title.starts_with("2K"), "{}", c.title);
        cs.settings.min_round_kills = 1;
        cs.settings.events.get_mut("2k").unwrap().enabled = false;
        round(&mut cs, 1, 0);
        assert!(rx.try_recv().is_ok(), "seuil à 1 kill : chaque round avec un kill");
    }

    #[test]
    fn deathmatch_sequences() {
        let (mut cs, rx) = setup_defaut();
        let dm = |p: P| payload(&P { mode: "deathmatch", ..p });
        cs.handle(&dm(P::default()));
        // 2 kills seulement : pas de clip
        cs.handle(&dm(P { kills: 1, ammo: 25, ..Default::default() }));
        sleep(Duration::from_millis(300));
        cs.handle(&dm(P { kills: 2, ammo: 20, ..Default::default() }));
        for _ in 0..15 {
            sleep(Duration::from_millis(100));
            cs.tick();
        }
        assert!(rx.try_recv().is_err(), "2 kills en DM : pas de clip");
        // 3 kills rapprochés : un seul clip
        for k in 3..=5 {
            sleep(Duration::from_millis(300));
            cs.handle(&dm(P { kills: k, ammo: 30 - k, ..Default::default() }));
        }
        let mut got = Vec::new();
        for _ in 0..20 {
            sleep(Duration::from_millis(100));
            cs.tick();
            while let Ok(c) = rx.try_recv() {
                got.push(c);
            }
        }
        assert_eq!(got.len(), 1, "un seul clip pour la série");
        println!("titre DM : {}", got[0].title);
    }

    #[test]
    fn mode_desactive() {
        let (mut cs, rx) = setup("round");
        cs.settings.modes.insert("deathmatch".into(), false);
        let dm = |p: P| payload(&P { mode: "deathmatch", ..p });
        cs.handle(&dm(P::default()));
        cs.handle(&dm(P { kills: 1, hs: 1, round_kills: 1, ammo: 29, ..Default::default() }));
        for _ in 0..12 {
            sleep(Duration::from_millis(100));
            cs.tick();
        }
        assert!(rx.try_recv().is_err());
    }
}
