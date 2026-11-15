// Polices du texte du Studio : libres (licence OFL), intégrées à l'appli, donc identiques sur tous les PC.
// Seul l'alphabet latin est chargé (accents français compris) pour garder l'appli légère.
import "@fontsource/inter/latin-800.css";
import "@fontsource/montserrat/latin-900.css";
import "@fontsource/bebas-neue/latin-400.css";
import "@fontsource/anton/latin-400.css";
import "@fontsource/oswald/latin-700.css";
import "@fontsource/poppins/latin-800.css";
import "@fontsource/bangers/latin-400.css";
import "@fontsource/permanent-marker/latin-400.css";
import "@fontsource/russo-one/latin-400.css";
import "@fontsource/press-start-2p/latin-400.css";
// Gothiques (style des edits TikTok), ajoutées le 2026-11-15
import "@fontsource/unifrakturmaguntia/latin-400.css";
import "@fontsource/pirata-one/latin-400.css";
import "@fontsource/grenze-gotisch/latin-900.css";
import "@fontsource/metal-mania/latin-400.css";
import "@fontsource/new-rocker/latin-400.css";
import "@fontsource/jacquard-24/latin-400.css";

export type FontGroup = "impact" | "clean" | "gothic" | "fun";
export type FontDef = { id: string; family: string; weight: number; label: string; group: FontGroup };

export const FONT_GROUPS: { id: FontGroup; label: string }[] = [
  { id: "gothic", label: "Gothiques" },
  { id: "impact", label: "Impact" },
  { id: "clean", label: "Propres" },
  { id: "fun", label: "Fun" },
];

export const FONTS: FontDef[] = [
  { id: "anton", family: "Anton", weight: 400, label: "Anton", group: "impact" },
  { id: "bebas", family: "Bebas Neue", weight: 400, label: "Bebas Neue", group: "impact" },
  { id: "oswald", family: "Oswald", weight: 700, label: "Oswald", group: "impact" },
  { id: "russo", family: "Russo One", weight: 400, label: "Russo One", group: "impact" },
  { id: "montserrat", family: "Montserrat", weight: 900, label: "Montserrat", group: "clean" },
  { id: "inter", family: "Inter", weight: 800, label: "Inter", group: "clean" },
  { id: "poppins", family: "Poppins", weight: 800, label: "Poppins", group: "clean" },
  { id: "fraktur", family: "UnifrakturMaguntia", weight: 400, label: "Fraktur", group: "gothic" },
  { id: "pirata", family: "Pirata One", weight: 400, label: "Pirata", group: "gothic" },
  { id: "grenze", family: "Grenze Gotisch", weight: 900, label: "Grenze", group: "gothic" },
  { id: "jacquard", family: "Jacquard 24", weight: 400, label: "Jacquard", group: "gothic" },
  { id: "metal", family: "Metal Mania", weight: 400, label: "Metal Mania", group: "gothic" },
  { id: "rocker", family: "New Rocker", weight: 400, label: "New Rocker", group: "gothic" },
  { id: "bangers", family: "Bangers", weight: 400, label: "Bangers", group: "fun" },
  { id: "marker", family: "Permanent Marker", weight: 400, label: "Marker", group: "fun" },
  { id: "pixel", family: "Press Start 2P", weight: 400, label: "Pixel", group: "fun" },
];

export const fontOf = (id: string): FontDef => FONTS.find((f) => f.id === id) ?? FONTS[0];

export const cssFont = (f: FontDef, px: number) => `${f.weight} ${px}px "${f.family}"`;

/** Attend que toutes les polices soient prêtes (avant de dessiner le texte pour l'export). */
export async function loadFonts(): Promise<void> {
  if (typeof document === "undefined" || !document.fonts) return;
  await Promise.all(FONTS.map((f) => document.fonts.load(cssFont(f, 48), "AÉazé09").catch(() => [])));
}
