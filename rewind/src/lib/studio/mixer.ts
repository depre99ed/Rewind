// Son de l'aperçu du Studio : chaque lecteur (vidéo, musique, piste séparée) passe par un gain WebAudio,
// pour entendre tout de suite un volume au-dessus de 100 %, une piste coupée ou la musique qui baisse.
// Les fichiers viennent du protocole « asset » de Tauri, qui autorise la lecture par l'interface (CORS) :
// les lecteurs doivent avoir crossOrigin = "anonymous" AVANT de recevoir leur adresse.

class Mixer {
  private ctx: AudioContext | null = null;
  private out: AnalyserNode | null = null;
  /** Volume général (fondu au noir de fin du montage) */
  private main: GainNode | null = null;
  private gains = new WeakMap<HTMLMediaElement, GainNode>();
  private routed = 0;
  private broken = false;

  /** À appeler sur un geste de l'utilisateur (lecture) : crée ou réveille le moteur son. */
  unlock() {
    if (this.broken) return;
    try {
      if (!this.ctx) {
        this.ctx = new AudioContext({ latencyHint: "interactive" });
        // Sortie commune (mesure du niveau pour le diagnostic)
        this.out = this.ctx.createAnalyser();
        this.out.fftSize = 1024;
        this.main = this.ctx.createGain();
        this.out.connect(this.main).connect(this.ctx.destination);
      }
      if (this.ctx.state === "suspended") this.ctx.resume().catch(() => {});
    } catch {
      this.broken = true;
    }
  }

  /** Diagnostic : état du moteur son, lecteurs branchés, niveau actuel de la sortie (0..1). */
  debug() {
    let level = 0;
    if (this.out) {
      const buf = new Float32Array(this.out.fftSize);
      this.out.getFloatTimeDomainData(buf);
      for (const v of buf) level = Math.max(level, Math.abs(v));
    }
    return { state: this.ctx?.state ?? "aucun", routed: this.routed, broken: this.broken, level };
  }

  /** Volume général de l'aperçu (0..1) : le son s'éteint avec l'image pendant le fondu de fin. */
  master(v: number) {
    const g = this.main;
    if (g && Math.abs(g.gain.value - v) > 0.002) g.gain.setTargetAtTime(Math.max(0, Math.min(1, v)), g.context.currentTime, 0.015);
  }

  /** Volume d'un lecteur (1 = 100 %, jusqu'à 400 %). Sans WebAudio : volume du lecteur, limité à 100 %. */
  set(el: HTMLMediaElement, v: number) {
    const vol = Math.max(0, Math.min(4, Number.isFinite(v) ? v : 0));
    const g = this.gain(el);
    if (g) {
      if (el.volume !== 1) el.volume = 1;
      if (Math.abs(g.gain.value - vol) > 0.002) g.gain.setTargetAtTime(vol, g.context.currentTime, 0.015);
    } else {
      const c = Math.min(1, vol);
      if (Math.abs(el.volume - c) > 0.002) el.volume = c;
    }
  }

  private gain(el: HTMLMediaElement): GainNode | null {
    const g = this.gains.get(el);
    if (g) return g;
    // Seulement un lecteur chargé en mode CORS (sinon WebAudio rendrait du silence)
    if (this.broken || !this.ctx || !this.out || el.crossOrigin !== "anonymous" || el.readyState < 1 || el.error) return null;
    try {
      const src = this.ctx.createMediaElementSource(el);
      const node = this.ctx.createGain();
      src.connect(node).connect(this.out);
      this.gains.set(el, node);
      this.routed++;
      return node;
    } catch {
      return null;
    }
  }
}

export const mixer = new Mixer();

/** Lecteur son prêt pour le mixeur (CORS demandé avant l'adresse). */
export function newAudioEl(): HTMLAudioElement {
  const el = new Audio();
  el.crossOrigin = "anonymous";
  el.preload = "auto";
  return el;
}

/**
 * Baisse de la musique quand le jeu est fort : même réglage que l'export (sidechaincompress
 * threshold=0.04 ratio=8 attack=30 ms release=400 ms), appliqué au niveau lu dans la forme d'onde.
 */
export class Ducker {
  private env = 0;
  gain(level: number, dt: number): number {
    const tau = level > this.env ? 0.03 : 0.4;
    this.env += (level - this.env) * (1 - Math.exp(-Math.max(0, dt) / tau));
    const thr = 0.04;
    if (this.env <= thr) return 1;
    return (thr * Math.pow(this.env / thr, 1 / 8)) / this.env;
  }
  reset() {
    this.env = 0;
  }
}
