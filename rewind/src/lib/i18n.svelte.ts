// Traduction de l'interface. La phrase française sert de clé : sans traduction, le français s'affiche
// (jamais de texte vide). Langue : réglage « language » (auto = langue de Windows / du navigateur).
import { app } from "./state.svelte";
import { EN } from "./i18n-en";

export type Lang = "fr" | "en";

export function lang(): Lang {
  const l = app.settings?.language ?? "auto";
  if (l === "fr" || l === "en") return l;
  const nav = typeof navigator !== "undefined" ? navigator.language || "fr" : "fr";
  return nav.toLowerCase().startsWith("fr") ? "fr" : "en";
}

/** Traduit une phrase française ; `{nom}` est remplacé par vars.nom.
 *  « mot§contexte » : même mot français, traductions différentes (ex. « Sortie§audio »). */
export function t(fr: string, vars?: Record<string, string | number>): string {
  const plain = fr.includes("§") ? fr.slice(0, fr.indexOf("§")) : fr;
  let s = lang() === "fr" ? plain : (EN[fr] ?? plain);
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  // Typographie française : espace insécable avant « : ; ! ? » % » et après « « » (une ligne ne commence
  // jamais par « : », comme on le voyait quand la fenêtre est étroite)
  if (lang() === "fr") s = s.replace(/ ([:;!?»%])/g, " $1").replace(/« /g, "« ");
  return s;
}

/** Locale pour les dates et nombres. */
export function locale(): string {
  return lang() === "fr" ? "fr-FR" : "en-US";
}
