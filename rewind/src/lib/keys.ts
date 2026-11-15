// Affichage des raccourcis (« Alt+F10 », « Mouse4 ») : noms lisibles et traduits.
import { t } from "./i18n.svelte";

const NAMES: Record<string, string> = {
  Mouse4: "Souris 4",
  Mouse5: "Souris 5",
  MouseMiddle: "Clic molette",
  Shift: "Maj",
  Space: "Espace",
};

/** Morceaux d'un raccourci prêts à afficher en touches : « Alt+F10 » → ["Alt", "F10"]. */
export function keyParts(combo: string | undefined | null): string[] {
  if (!combo) return [];
  return combo
    .split("+")
    .map((k) => k.trim())
    .filter(Boolean)
    .map((k) => (NAMES[k] ? t(NAMES[k]) : k));
}

/** Raccourci en texte court : « Alt + F10 ». */
export function keyText(combo: string | undefined | null): string {
  return keyParts(combo).join(" + ");
}
