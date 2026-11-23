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
// Polices d'edit ajoutées le 2026-11-23 (gothique type « Old English », graffiti, sport, Y2K)
import "@fontsource/unifrakturcook/latin-700.css";
import "@fontsource/germania-one/latin-400.css";
import "@fontsource/fruktur/latin-400.css";
import "@fontsource/sedgwick-ave-display/latin-400.css";
import "@fontsource/bungee/latin-400.css";
import "@fontsource/audiowide/latin-400.css";
import "@fontsource/black-ops-one/latin-400.css";
import "@fontsource/monoton/latin-400.css";
// Toutes les gothiques et médiévales libres (2026-11-23, demande : « vraiment toutes, un large choix »)
import "@fontsource/jacquard-12/latin-400.css";
import "@fontsource/jacquarda-bastarda-9/latin-400.css";
import "@fontsource/jacquarda-bastarda-9-charted/latin-400.css";
import "@fontsource/texturina/latin-900.css";
import "@fontsource/blaka/latin-400.css";
import "@fontsource/blaka-hollow/latin-400.css";
import "@fontsource/almendra-display/latin-400.css";
import "@fontsource/medievalsharp/latin-400.css";
import "@fontsource/uncial-antiqua/latin-400.css";
import "@fontsource/kings/latin-400.css";
import "@fontsource/eagle-lake/latin-400.css";
import "@fontsource/metamorphous/latin-400.css";
import "@fontsource/fondamento/latin-400.css";
import "@fontsource/im-fell-english-sc/latin-400.css";
import "@fontsource/cinzel-decorative/latin-900.css";

export type FontGroup = "impact" | "clean" | "gothic" | "medieval" | "fun";
export type FontDef = { id: string; family: string; weight: number; label: string; group: FontGroup };

export const FONT_GROUPS: { id: FontGroup; label: string }[] = [
  { id: "gothic", label: "Gothiques" },
  { id: "medieval", label: "Médiévales" },
  { id: "impact", label: "Impact" },
  { id: "clean", label: "Propres" },
  { id: "fun", label: "Fun" },
];

export const FONTS: FontDef[] = [
  { id: "anton", family: "Anton", weight: 400, label: "Anton", group: "impact" },
  { id: "bebas", family: "Bebas Neue", weight: 400, label: "Bebas Neue", group: "impact" },
  { id: "oswald", family: "Oswald", weight: 700, label: "Oswald", group: "impact" },
  { id: "russo", family: "Russo One", weight: 400, label: "Russo One", group: "impact" },
  { id: "bungee", family: "Bungee", weight: 400, label: "Bungee", group: "impact" },
  { id: "blackops", family: "Black Ops One", weight: 400, label: "Black Ops", group: "impact" },
  { id: "montserrat", family: "Montserrat", weight: 900, label: "Montserrat", group: "clean" },
  { id: "inter", family: "Inter", weight: 800, label: "Inter", group: "clean" },
  { id: "poppins", family: "Poppins", weight: 800, label: "Poppins", group: "clean" },
  { id: "oldenglish", family: "UnifrakturCook", weight: 700, label: "Old English", group: "gothic" },
  { id: "fraktur", family: "UnifrakturMaguntia", weight: 400, label: "Fraktur", group: "gothic" },
  { id: "germania", family: "Germania One", weight: 400, label: "Germania", group: "gothic" },
  { id: "fruktur", family: "Fruktur", weight: 400, label: "Fruktur", group: "gothic" },
  { id: "pirata", family: "Pirata One", weight: 400, label: "Pirata", group: "gothic" },
  { id: "grenze", family: "Grenze Gotisch", weight: 900, label: "Grenze", group: "gothic" },
  { id: "jacquard", family: "Jacquard 24", weight: 400, label: "Jacquard", group: "gothic" },
  { id: "metal", family: "Metal Mania", weight: 400, label: "Metal Mania", group: "gothic" },
  { id: "rocker", family: "New Rocker", weight: 400, label: "New Rocker", group: "gothic" },
  { id: "jacquard12", family: "Jacquard 12", weight: 400, label: "Jacquard 12", group: "gothic" },
  { id: "bastarda", family: "Jacquarda Bastarda 9", weight: 400, label: "Bastarda", group: "gothic" },
  { id: "bastardach", family: "Jacquarda Bastarda 9 Charted", weight: 400, label: "Bastarda brodée", group: "gothic" },
  { id: "texturina", family: "Texturina", weight: 900, label: "Texturina", group: "gothic" },
  { id: "blaka", family: "Blaka", weight: 400, label: "Blaka", group: "gothic" },
  { id: "blakahollow", family: "Blaka Hollow", weight: 400, label: "Blaka creuse", group: "gothic" },
  { id: "almendra", family: "Almendra Display", weight: 400, label: "Almendra", group: "medieval" },
  { id: "medieval", family: "MedievalSharp", weight: 400, label: "Medieval Sharp", group: "medieval" },
  { id: "uncial", family: "Uncial Antiqua", weight: 400, label: "Uncial", group: "medieval" },
  { id: "kings", family: "Kings", weight: 400, label: "Kings", group: "medieval" },
  { id: "eaglelake", family: "Eagle Lake", weight: 400, label: "Eagle Lake", group: "medieval" },
  { id: "metamorphous", family: "Metamorphous", weight: 400, label: "Metamorphous", group: "medieval" },
  { id: "fondamento", family: "Fondamento", weight: 400, label: "Fondamento", group: "medieval" },
  { id: "imfell", family: "IM Fell English SC", weight: 400, label: "Fell English", group: "medieval" },
  { id: "cinzel", family: "Cinzel Decorative", weight: 900, label: "Cinzel", group: "medieval" },
  { id: "bangers", family: "Bangers", weight: 400, label: "Bangers", group: "fun" },
  { id: "marker", family: "Permanent Marker", weight: 400, label: "Marker", group: "fun" },
  { id: "pixel", family: "Press Start 2P", weight: 400, label: "Pixel", group: "fun" },
  { id: "graffiti", family: "Sedgwick Ave Display", weight: 400, label: "Graffiti", group: "fun" },
  { id: "y2k", family: "Audiowide", weight: 400, label: "Y2K", group: "fun" },
  { id: "monoton", family: "Monoton", weight: 400, label: "Monoton", group: "fun" },
];

export const fontOf = (id: string): FontDef => FONTS.find((f) => f.id === id) ?? FONTS[0];

export const cssFont = (f: FontDef, px: number) => `${f.weight} ${px}px "${f.family}"`;

/** Attend que toutes les polices soient prêtes (avant de dessiner le texte pour l'export). */
export async function loadFonts(): Promise<void> {
  if (typeof document === "undefined" || !document.fonts) return;
  await Promise.all(FONTS.map((f) => document.fonts.load(cssFont(f, 48), "AÉazé09").catch(() => [])));
}
