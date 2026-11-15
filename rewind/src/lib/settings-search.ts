// Recherche dans les Réglages (2026-11-15).
// L'index est lu dans le code de SettingsView.svelte lui-même (titres de section, groupes <h3>, réglages
// <strong> + leur explication <span>) : un réglage ajouté plus tard est trouvable sans rien maintenir.
// La recherche se fait dans la langue affichée (t() traduit chaque texte), sans tenir compte des accents.
import RAW from "./components/SettingsView.svelte?raw";
import { t } from "$lib/i18n.svelte";

export type SettingHit = { section: string; label: string; hint: string; group: string; kind: "section" | "group" | "item" };

type Raw = { section: string; label: string; hint: string; group: string; kind: SettingHit["kind"] };

// Mots que les gens tapent mais qui ne sont pas dans les libellés (français et anglais)
const SYNONYMS: Record<string, string> = {
  video: "qualité quality fps résolution resolution bitrate débit encodeur encoder nvenc écran screen capture",
  clips: "durée length duration dossier folder disque disk ssd ram mémoire memory stockage storage medal import importer buffer tampon",
  audio: "son sound micro mic microphone discord volume piste track",
  keys: "touche key bind binds hotkey raccourci clavier keyboard souris mouse voix voice vocal admin",
  notif: "son sound bruit notification bandeau banner",
  overlay: "touches keys clavier keyboard affichage",
  share: "discord webhook partage share envoyer send",
  look: "thème theme couleur color colour accent sombre dark clair light animation langue language",
  general: "démarrage startup windows mise à jour update version accueil welcome",
  diag: "rapport report journal log bug problème problem",
};

let cache: Raw[] | null = null;

/** Réglages trouvés dans le code de la page (textes français, traduits à l'affichage). */
function index(sections: readonly (readonly [string, string, unknown])[]): Raw[] {
  if (cache) return cache;
  const out: Raw[] = sections.map(([id, label]) => ({ section: id, label, hint: "", group: "", kind: "section" as const }));
  let section = "";
  let group = "";
  const lines = RAW.split("\n");
  for (const [n, line] of lines.entries()) {
    const sec = line.match(/\{[#:](?:else )?if section === "(\w+)"\}/);
    if (sec) {
      section = sec[1];
      group = "";
      continue;
    }
    if (!section) continue;
    const h3 = line.match(/<h3>\{t\("([^"]+)"/);
    if (h3) {
      group = h3[1];
      out.push({ section, label: group, hint: "", group: "", kind: "group" });
      continue;
    }
    const strong = line.match(/<strong>\{(?:[^}]*?\?\s*)?t\("([^"]+)"/);
    if (strong) {
      // Explication sur la même ligne ou sur les deux suivantes
      const after = line.slice((strong.index ?? 0) + strong[0].length) + "\n" + (lines[n + 1] ?? "") + "\n" + (lines[n + 2] ?? "");
      const hint = after.match(/<span[^>]*>\{(?:[^}]*?\?\s*)?t\("([^"]+)"/);
      out.push({ section, label: strong[1], hint: hint?.[1] ?? "", group, kind: "item" });
    }
  }
  cache = out;
  return out;
}

/** Texte affiché sans ses valeurs variables (« Débit : {n} Mb/s » → « Débit »). */
export function shown(fr: string): string {
  if (!fr) return "";
  const s = t(fr);
  return (s.includes("{") ? s.split(/\s*[:{]/)[0] : s).trim();
}

const norm = (s: string) => s.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();

export function searchSettings(query: string, sections: readonly (readonly [string, string, unknown])[]): SettingHit[] {
  const words = norm(query).split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const scored: { hit: SettingHit; score: number }[] = [];
  const seen = new Set<string>();
  for (const r of index(sections)) {
    const label = shown(r.label);
    const hint = r.hint ? t(r.hint).replace(/\{\w+\}/g, "…") : "";
    const group = shown(r.group);
    const sectionName = t(sections.find((s) => s[0] === r.section)?.[1] ?? "");
    const extra = r.kind === "section" ? SYNONYMS[r.section] ?? "" : "";
    // Début de mot seulement (« ram » ne doit pas trouver « programme »)
    const hay = norm(`${label} ${hint} ${group} ${sectionName} ${extra}`).split(/[^a-z0-9]+/);
    if (!words.every((w) => hay.some((h) => h.startsWith(w)))) continue;
    const key = `${r.section}|${label}`;
    if (seen.has(key)) continue;
    seen.add(key);
    // Libellé qui commence par le mot cherché > libellé qui le contient > explication seulement
    const l = norm(label);
    const lw = l.split(/[^a-z0-9]+/);
    const score = words.reduce((n, w) => n + (l.startsWith(w) ? 3 : lw.some((x) => x.startsWith(w)) ? 2 : 0), 0) + (r.kind === "item" ? 0.5 : 0);
    scored.push({ hit: { section: r.section, label, hint, group, kind: r.kind }, score });
  }
  return scored.sort((a, b) => b.score - a.score).slice(0, 30).map((s) => s.hit);
}
