// Sons « digitaux » à la Nothing : courts bips synthétisés en mémoire, aucun fichier audio

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

/// Ctrl+M coupe ou remet les sons
pub static ACTIFS: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy)]
pub enum Son {
    /// Opération terminée : deux bips montants
    Fini,
    /// Mis à la corbeille : glissando descendant
    Corbeille,
    /// Erreur : bip grave
    Erreur,
    /// Petite action réussie (renommer, nouveau dossier, copier) : un tic
    Tic,
}

const FREQ: u32 = 44_100;

/// Notes : (fréquence de départ, fréquence d'arrivée, durée en ms) ; fréquence 0 = silence
fn notes(s: Son) -> &'static [(f32, f32, u32)] {
    match s {
        Son::Fini => &[(1568.0, 1568.0, 45), (0.0, 0.0, 35), (2093.0, 2093.0, 70)],
        Son::Corbeille => &[(1400.0, 500.0, 120)],
        Son::Erreur => &[(330.0, 330.0, 70), (0.0, 0.0, 30), (262.0, 262.0, 110)],
        Son::Tic => &[(2637.0, 2637.0, 18)],
    }
}

/// Fichier WAV complet (16 bits mono), onde carrée adoucie pour le timbre « numérique »
fn wav(s: Son) -> Vec<u8> {
    wav_notes(notes(s))
}

fn wav_notes(notes: &[(f32, f32, u32)]) -> Vec<u8> {
    let mut echantillons: Vec<i16> = Vec::new();
    for &(f0, f1, ms) in notes {
        let n = (FREQ * ms / 1000) as usize;
        let mut phase = 0.0f32;
        for k in 0..n {
            let t = k as f32 / n as f32;
            let f = f0 + (f1 - f0) * t;
            if f <= 0.0 {
                echantillons.push(0);
                continue;
            }
            phase = (phase + f / FREQ as f32).fract();
            // Carré + un peu de sinus : net sans être agressif
            let carre = if phase < 0.5 { 1.0 } else { -1.0 };
            let sinus = (phase * std::f32::consts::TAU).sin();
            // Enveloppe : attaque 2 ms, extinction douce
            let attaque = (k as f32 / (FREQ as f32 * 0.002)).min(1.0);
            let fin = (1.0 - t).powf(1.5);
            let v = (0.35 * carre + 0.65 * sinus) * attaque * fin * 0.16;
            echantillons.push((v * i16::MAX as f32) as i16);
        }
    }
    let donnees = (echantillons.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + donnees as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + donnees).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&FREQ.to_le_bytes());
    w.extend_from_slice(&(FREQ * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&donnees.to_le_bytes());
    for e in echantillons {
        w.extend_from_slice(&e.to_le_bytes());
    }
    w
}

pub fn jouer(s: Son) {
    if !ACTIFS.load(Ordering::Relaxed) {
        return;
    }
    // Chaque son est synthétisé une seule fois et reste en mémoire (PlaySound le lit en arrière-plan)
    static SONS: OnceLock<[Vec<u8>; 4]> = OnceLock::new();
    let sons = SONS.get_or_init(|| [wav(Son::Fini), wav(Son::Corbeille), wav(Son::Erreur), wav(Son::Tic)]);
    let i = match s {
        Son::Fini => 0,
        Son::Corbeille => 1,
        Son::Erreur => 2,
        Son::Tic => 3,
    };
    unsafe {
        let _ = PlaySoundW(PCWSTR(sons[i].as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}

/// Poids sonore (O4) : un tic dont la hauteur descend avec la taille du dossier,
/// de 3 kHz (quelques Ko) à 260 Hz (1 To et plus), sur 16 paliers
pub fn jouer_poids(octets: u64) {
    if !ACTIFS.load(Ordering::Relaxed) {
        return;
    }
    const PALIERS: usize = 16;
    static TICS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    let tics = TICS.get_or_init(|| {
        (0..PALIERS)
            .map(|k| {
                let f = 3000.0 * (260.0f32 / 3000.0).powf(k as f32 / (PALIERS - 1) as f32);
                wav_notes(&[(f, f, 26)])
            })
            .collect()
    });
    // 10 Ko → palier 0, 1 To → dernier palier (échelle logarithmique)
    let l = ((octets.max(1) as f64).log10() - 4.0) / 8.0;
    let k = ((l.clamp(0.0, 1.0) * (PALIERS - 1) as f64).round()) as usize;
    unsafe {
        let _ = PlaySoundW(PCWSTR(tics[k].as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}
