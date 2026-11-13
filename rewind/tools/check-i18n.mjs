// Vérifie que chaque phrase passée à t("…") (et à studio.say("…")) a sa traduction anglaise.
// Usage : node tools/check-i18n.mjs   (code de sortie 1 s'il en manque)
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL("../src/", import.meta.url).pathname.replace(/^\/([A-Z]:)/, "$1");
const files = [];
(function walk(d) {
  for (const f of readdirSync(d)) {
    const p = join(d, f);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(svelte|ts)$/.test(f) && !f.startsWith("i18n")) files.push(p);
  }
})(root);

const dict = readFileSync(join(root, "lib", "i18n-en.ts"), "utf8");
const keys = new Set([...dict.matchAll(/^\s*"((?:[^"\\]|\\.)*)":/gm)].map((m) => m[1].replace(/\\"/g, '"')));
const missing = new Map();
for (const f of files) {
  const src = readFileSync(f, "utf8");
  for (const m of src.matchAll(/\b(?:t|say)\(\s*"((?:[^"\\]|\\.)*)"/g)) {
    const k = m[1].replace(/\\"/g, '"');
    if (!keys.has(k)) missing.set(k, f.replace(root, ""));
  }
  // Libellés des listes traduites à l'affichage (TRANSITIONS, modèles d'export)
  for (const m of src.matchAll(/\b(?:label|hint|desc): "((?:[^"\\]|\\.)*)"/g)) {
    if (/studio\.svelte\.ts|ExportDialog|Cs2View/.test(f) && !keys.has(m[1])) missing.set(m[1], f.replace(root, ""));
  }
}
if (missing.size) {
  console.log(`${missing.size} traduction(s) manquante(s) :`);
  for (const [k, f] of missing) console.log(`  [${f}] ${k}`);
  process.exit(1);
}
console.log(`OK : toutes les phrases ont leur traduction (${keys.size} entrées).`);
