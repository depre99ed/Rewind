//! Sons de kill fournis avec Rewind (2026-11-24) : hitmarker, ding, boom… synthétisés ici, aucun fichier tiers
//! (donc pas de droits à respecter). Écrits une fois dans `data_dir/sons-kill` ; la version dans le nom de fichier
//! force leur réécriture quand un son est retouché.

use crate::util;
use serde::Serialize;
use std::f64::consts::PI;
use std::path::PathBuf;

const RATE: f64 = 48_000.0;
const VERSION: u32 = 1;

#[derive(Serialize, Clone, Debug)]
pub struct KillSound {
    pub id: String,
    /// Nom affiché (traduit par l'interface)
    pub name: String,
    pub path: String,
    /// Durée (s)
    pub dur: f64,
}

/// Sons proposés, dans l'ordre d'affichage.
pub const LIST: [(&str, &str); 7] = [
    ("hitmarker", "Hitmarker"),
    ("ding", "Ding headshot"),
    ("boom", "Boom grave"),
    ("snap", "Claquement"),
    ("laser", "Laser"),
    ("whoosh", "Whoosh"),
    ("bell", "Cloche"),
];

/// Bruit blanc reproductible (xorshift), dans [-1, 1].
struct Noise(u64);
impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}

/// Échantillons du son `id` (mono, 48 kHz), crête ramenée à 0,9.
pub fn synth(id: &str) -> Vec<f64> {
    let len = match id {
        "hitmarker" => 0.12,
        "ding" => 1.0,
        "boom" => 0.7,
        "snap" => 0.16,
        "laser" => 0.4,
        "whoosh" => 0.5,
        "bell" => 1.6,
        _ => return Vec::new(),
    };
    let n = (len * RATE) as usize;
    let mut out = Vec::with_capacity(n);
    let mut noise = Noise(0x9E37_79B9_7F4A_7C15);
    let (mut phase, mut lp, mut prev) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let t = i as f64 / RATE;
        let attack = (t / 0.002).min(1.0);
        let v = match id {
            // Petit « tik » sec : claquement de bruit très court + deux tons aigus qui s'éteignent vite
            "hitmarker" => {
                noise.next() * (-t / 0.0015).exp() * 0.5
                    + (2.0 * PI * 3300.0 * t).sin() * (-t / 0.02).exp() * 0.8
                    + (2.0 * PI * 5600.0 * t).sin() * (-t / 0.009).exp() * 0.35
            }
            // Ding clair (sol aigu et ses harmoniques), long et propre
            "ding" => {
                attack
                    * ((2.0 * PI * 1568.0 * t).sin() * 0.7 * (-t / 0.45).exp()
                        + (2.0 * PI * 3136.0 * t).sin() * 0.25 * (-t / 0.25).exp()
                        + (2.0 * PI * 4704.0 * t).sin() * 0.1 * (-t / 0.12).exp())
            }
            // Coup grave : fréquence qui tombe de 155 à 45 Hz, légère saturation
            "boom" => {
                let f = 45.0 + 110.0 * (-t / 0.06).exp();
                phase += 2.0 * PI * f / RATE;
                let x = phase.sin() * (-t / 0.22).exp() + noise.next() * (-t / 0.004).exp() * 0.35;
                (1.8 * x).tanh() / 1.8f64.tanh()
            }
            // Claquement : bruit filtré vers l'aigu, très bref
            "snap" => {
                let x = noise.next();
                let hp = x - prev;
                prev = x;
                hp * (-t / 0.014).exp() * attack.max(0.3)
            }
            // Laser : tonalité qui plonge, un peu carrée
            "laser" => {
                let f = 180.0 + 1700.0 * (-t / 0.07).exp();
                phase += 2.0 * PI * f / RATE;
                (3.0 * phase.sin()).tanh() * 0.6 * (-t / 0.14).exp() * attack
            }
            // Souffle qui passe : bruit adouci dont le filtre s'ouvre puis se referme
            "whoosh" => {
                let p = t / len;
                let k = 0.02 + 0.25 * (PI * p).sin().powi(2);
                lp += k * (noise.next() - lp);
                lp * (PI * p).sin().powi(2) * 2.5
            }
            // Cloche : partiels non harmoniques, extinction lente
            "bell" => {
                let parts = [(880.0, 1.0, 1.3), (2428.0, 0.5, 0.6), (4752.0, 0.25, 0.35), (7858.0, 0.12, 0.2)];
                attack * parts.iter().map(|(f, a, d)| (2.0 * PI * f * t).sin() * a * (-t / d).exp()).sum::<f64>()
            }
            _ => 0.0,
        };
        out.push(v);
    }
    // Petit fondu de fin (pas de clic) et crête à 0,9
    let fade = (0.005 * RATE) as usize;
    for k in 0..fade.min(out.len()) {
        let i = out.len() - 1 - k;
        out[i] *= k as f64 / fade as f64;
    }
    let peak = out.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    out.iter().map(|v| v / peak * 0.9).collect()
}

fn wav(samples: &[f64]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data as usize);
    w.extend(b"RIFF");
    w.extend((36 + data).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend((RATE as u32).to_le_bytes());
    w.extend((RATE as u32 * 2).to_le_bytes());
    w.extend(2u16.to_le_bytes());
    w.extend(16u16.to_le_bytes());
    w.extend(b"data");
    w.extend(data.to_le_bytes());
    for s in samples {
        w.extend(((s.clamp(-1.0, 1.0) * i16::MAX as f64) as i16).to_le_bytes());
    }
    w
}

/// Les sons fournis, écrits sur le disque si besoin.
pub fn list() -> Vec<KillSound> {
    let dir: PathBuf = util::data_dir().join("sons-kill");
    let _ = std::fs::create_dir_all(&dir);
    LIST.iter()
        .filter_map(|(id, name)| {
            let path = dir.join(format!("{id}-v{VERSION}.wav"));
            let samples = synth(id);
            if !path.is_file() {
                std::fs::write(&path, wav(&samples)).ok()?;
            }
            Some(KillSound { id: id.to_string(), name: name.to_string(), path: path.to_string_lossy().to_string(), dur: samples.len() as f64 / RATE })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sons_propres() {
        for (id, _) in LIST {
            let s = synth(id);
            assert!(s.len() > 1000, "{id} trop court");
            let peak = s.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            assert!((peak - 0.9).abs() < 1e-6, "{id} : crête {peak}");
            assert!(s.last().unwrap().abs() < 1e-3, "{id} : clic en fin de son");
            assert!(s.iter().all(|v| v.is_finite()));
        }
        let l = list();
        assert_eq!(l.len(), LIST.len());
        assert!(l.iter().all(|k| std::path::Path::new(&k.path).is_file() && k.dur > 0.1));
    }
}
