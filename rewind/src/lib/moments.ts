// Ton de chaque type d'événement CS2. Les pictos eux-mêmes sont dans MomentIcon.svelte.
// Budget couleur (demande du 2026-11-15) : les repères sont BLANCS ; seule la couleur signale les gros
// moments : or pour 2K / 3K / 4K / ACE, vert pour un clutch. La mort est grisée.
export type MomentTone = "plain" | "gold" | "green" | "dim";

const TONES: Record<string, MomentTone> = {
  "2k": "gold",
  "3k": "gold",
  "4k": "gold",
  ace: "gold",
  clutch: "green",
  death: "dim",
};

export function momentTone(kind: string): MomentTone {
  return TONES[kind] ?? "plain";
}

/** Couleurs réelles (pour le dessin sur canvas et l'export). */
export const TONE_COLOR: Record<MomentTone, string> = {
  plain: "#f2f3f5",
  gold: "#ffd25a",
  green: "#3ddc97",
  dim: "#9aa3b5",
};
