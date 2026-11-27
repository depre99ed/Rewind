"""Affinage du graphe graphify de Rewind (2026-11-24) : à lancer après l'extraction, avant la construction.

    python tools/graphify_affiner.py graphify-out/.graphify_extract.json

Ce que l'analyse automatique (AST) ne sait pas faire, et que ce script fait de façon vérifiable :
 1. Pont interface ↔ moteur : chaque `api.x(...)` / `invoke("commande")` côté interface est relié à la commande
    Rust `#[tauri::command] fn commande` (relation `invokes`), et chaque `listen("évènement")` au fichier Rust
    qui fait `emit("évènement")` (relation `listens_to`). Lu dans le texte source : EXTRACTED.
 2. Éléments « fantômes » (un nom de type non rattaché à un fichier) : rattachés à leur seule définition dans
    le projet (même langage, ou celle du fichier importé) ; sinon marqués « externe » (bibliothèque) ou
    « ambigu » avec la raison.
 3. Liens devinés (INFERRED) du code : un appel n'est gardé que si le nom appelé figure bien sur la ligne
    source indiquée (il devient alors EXTRACTED, vérifié) ; un appel d'un langage vers un autre est retiré.
Le résumé (combien de liens ajoutés, vérifiés, retirés) est écrit dans graphify-out/affinage.json.
"""
import json
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
NODES = {}
LANG = {".rs": "rust", ".ts": "ts", ".svelte": "ts", ".js": "ts", ".mjs": "ts", ".py": "py", ".sql": "sql", ".ps1": "ps"}


def lang_of(src):
    return LANG.get(Path(src or "").suffix.lower()) if src else None


def rel(src):
    if not src:
        return src
    p = Path(src)
    try:
        return p.relative_to(ROOT).as_posix() if p.is_absolute() else p.as_posix()
    except ValueError:
        return p.as_posix()


def file_id(relpath):
    """Identifiant du nœud « fichier » graphify : chemin relatif sans extension, segments joints par _."""
    stem = str(Path(relpath).with_suffix("")).replace("\\", "/")
    return "_".join(re.sub(r"[^0-9a-z]+", "_", s.lower()).strip("_") for s in stem.split("/"))


_lines = {}


def lines(relpath):
    if relpath not in _lines:
        try:
            _lines[relpath] = (ROOT / relpath).read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            _lines[relpath] = []
    return _lines[relpath]


def edge(src, tgt, relation, source_file, line, **extra):
    return {"source": src, "target": tgt, "relation": relation, "confidence": "EXTRACTED", "confidence_score": 1.0,
            "source_file": source_file, "source_location": f"L{line}", "weight": 1.0, "_origin": "affinage", **extra}


def bridge(nodes, report):
    """1. Interface ↔ moteur (commandes Tauri et évènements)."""
    ids = set(nodes)
    out = []
    # Commandes Rust : #[tauri::command] suivi de fn nom
    commands = {}
    for rs in sorted((ROOT / "src-tauri" / "src").glob("*.rs")):
        r = rel(rs)
        ls = lines(r)
        for i, l in enumerate(ls):
            if "#[tauri::command" in l:
                for j in range(i + 1, min(i + 6, len(ls))):
                    m = re.search(r"\bfn\s+(\w+)", ls[j])
                    if m:
                        nid = f"{file_id(r)}_{m.group(1).lower()}"
                        if nid in ids:
                            commands[m.group(1)] = nid
                        break
    # Méthodes de api.ts → commande appelée
    method_cmd = {}
    for i, l in enumerate(lines("src/lib/api.ts")):
        m = re.match(r"\s*(\w+)\s*:\s*(?:async\s*)?\(.*?invoke(?:<[^(]*>)?\(\s*\"(\w+)\"", l)
        if m:
            method_cmd[m.group(1)] = (m.group(2), i + 1)
    api_id = "src_lib_api_api"
    for meth, (cmd, ln) in method_cmd.items():
        if cmd in commands and api_id in ids:
            out.append(edge(api_id, commands[cmd], "invokes", "src/lib/api.ts", ln, command=cmd, via=f"api.{meth}"))
    # Fichiers d'interface qui appellent api.x(...) ou invoke("cmd") directement
    seen = set()
    for f in sorted(list((ROOT / "src").rglob("*.ts")) + list((ROOT / "src").rglob("*.svelte"))):
        r = rel(f)
        fid = file_id(r)
        if fid not in ids or r == "src/lib/api.ts":
            continue
        for i, l in enumerate(lines(r)):
            for meth in re.findall(r"\bapi\.(\w+)\s*\(", l):
                cmd = method_cmd.get(meth, (None,))[0]
                if cmd in commands and (fid, cmd) not in seen:
                    seen.add((fid, cmd))
                    out.append(edge(fid, commands[cmd], "invokes", r, i + 1, command=cmd, via=f"api.{meth}"))
            for cmd in re.findall(r"\binvoke(?:<[^(]*>)?\(\s*\"(\w+)\"", l):
                if cmd in commands and (fid, cmd) not in seen:
                    seen.add((fid, cmd))
                    out.append(edge(fid, commands[cmd], "invokes", r, i + 1, command=cmd))
    report["commandes_rust"] = len(commands)
    report["liens_invokes"] = len(out)
    # Évènements : emit("x") côté Rust, listen("x") côté interface
    emitters = defaultdict(list)
    for rs in sorted((ROOT / "src-tauri" / "src").glob("*.rs")):
        r = rel(rs)
        for i, l in enumerate(lines(r)):
            for ev in re.findall(r"\.emit(?:_to)?\([^\"]*\"([\w:-]+)\"", l):
                emitters[ev].append((file_id(r), r, i + 1))
    n_ev = 0
    for f in sorted(list((ROOT / "src").rglob("*.ts")) + list((ROOT / "src").rglob("*.svelte"))):
        r = rel(f)
        fid = file_id(r)
        if fid not in ids:
            continue
        for i, l in enumerate(lines(r)):
            for ev in re.findall(r"\blisten(?:<[^(]*>)?\(\s*\"([\w:-]+)\"", l):
                for tgt, _, _ in {e[0]: e for e in emitters.get(ev, [])}.values():
                    if tgt in ids:
                        out.append(edge(fid, tgt, "listens_to", r, i + 1, event=ev))
                        n_ev += 1
    report["liens_evenements"] = n_ev
    return out


def ghosts(nodes, edges, report):
    """2. Éléments sans fichier : rattachés à leur définition, ou marqués externe / ambigu."""
    by_label = defaultdict(list)
    for n in nodes.values():
        if n.get("source_file"):
            by_label[n.get("label")].append(n["id"])
    file_imports = defaultdict(set)  # fichier → fichiers importés
    for e in edges:
        if e.get("relation") in ("imports_from", "imports"):
            s, t = nodes.get(e["source"]), nodes.get(e["target"])
            if s and t and t.get("source_file"):
                file_imports[rel(s.get("source_file"))].add(rel(t["source_file"]))
    redirect = {}
    marked = {"externe": 0, "ambigu": 0}
    users = defaultdict(set)
    for e in edges:
        for end in ("source", "target"):
            if e[end] in nodes and not nodes[e[end]].get("source_file"):
                users[e[end]].add(rel(e.get("source_file")))
    for gid, g in list(nodes.items()):
        if g.get("source_file"):
            continue
        cands = by_label.get(g.get("label"), [])
        langs = {lang_of(u) for u in users[gid]}
        same = [c for c in cands if lang_of(nodes[c].get("source_file")) in langs]
        if len(same) == 1:
            redirect[gid] = same[0]
            continue
        if len(same) > 1:
            imported = set().union(*(file_imports[u] for u in users[gid])) if users[gid] else set()
            pick = [c for c in same if rel(nodes[c]["source_file"]) in imported]
            if len(pick) == 1:
                redirect[gid] = pick[0]
                continue
            # La ligne d'import dit d'où vient le nom (Rust : `use crate::library::{Clip, …}`)
            mods = set()
            for e in edges:
                if e.get("target") == gid and e.get("relation") in ("imports_from", "imports"):
                    m = re.match(r"L(\d+)", e.get("source_location") or "")
                    text = lines(rel(e.get("source_file")))
                    if m and 0 < int(m.group(1)) <= len(text):
                        mods |= set(re.findall(r"(\w+)::", text[int(m.group(1)) - 1]))
            pick = [c for c in same if Path(nodes[c]["source_file"]).stem in mods]
            if len(pick) == 1:
                redirect[gid] = pick[0]
                continue
            g["note"] = f"ambigu : {len(same)} définitions possibles ({', '.join(rel(nodes[c]['source_file']) for c in same[:4])})"
            g["confidence"] = "AMBIGUOUS"
            marked["ambigu"] += 1
        else:
            g["note"] = "externe : type ou module d'une bibliothèque (pas défini dans Rewind)"
            g["external"] = True
            marked["externe"] += 1
    for e in edges:
        e["source"] = redirect.get(e["source"], e["source"])
        e["target"] = redirect.get(e["target"], e["target"])
    for gid in redirect:
        nodes.pop(gid, None)
    report["fantomes_rattaches"] = len(redirect)
    report["fantomes_detail"] = {nid: tgt for nid, tgt in list(redirect.items())[:40]}
    report.update({f"fantomes_{k}": v for k, v in marked.items()})


def spans(nodes):
    """Lignes [début, fin) du corps de chaque symbole : jusqu'au symbole suivant du même fichier qui n'est pas
    plus indenté (une fonction imbriquée reste dans le corps de sa fonction) ; un nœud « fichier » couvre tout."""
    per_file = defaultdict(list)
    for n in nodes.values():
        m = re.match(r"L(\d+)", n.get("source_location") or "")
        if m and n.get("source_file"):
            per_file[rel(n["source_file"])].append((int(m.group(1)), n["id"]))
    out = {}
    for f, items in per_file.items():
        items.sort()
        text = lines(f)
        indent = lambda ln: len(text[ln - 1]) - len(text[ln - 1].lstrip()) if 0 < ln <= len(text) else 0
        for k, (ln, nid) in enumerate(items):
            if nid == file_id(f) or nodes[nid].get("label") == Path(f).name:
                out[nid] = (f, 1, len(text) + 1)
                continue
            nxt = next((l for l, _ in items[k + 1:] if l > ln and indent(l) <= indent(ln)), len(text) + 1)
            out[nid] = (f, ln, nxt)
    return out


def rust_path(fs, ft, label):
    """Rust : un autre module est utilisé par son chemin (`cs2::Cs2Live`, `util::wide`, `crate::tr`),
    sans ligne d'import que l'analyse verrait."""
    if lang_of(fs) != "rust" or lang_of(ft) != "rust":
        return False
    text = "\n".join(lines(fs))
    stem = Path(ft).stem
    name = re.sub(r"[()]", "", label).lstrip(".").strip()
    if stem in ("lib", "main"):
        return bool(re.search(rf"\bcrate::{re.escape(name)}\b", text))
    return bool(re.search(rf"\b{re.escape(stem)}::", text))


def verify_inferred(nodes, edges, report):
    """3. Liens devinés du code : gardés seulement si le nom visé figure dans le corps du symbole d'origine
    (il devient EXTRACTED, vérifié) ; un appel d'un langage vers un autre est retiré."""
    sp = spans(nodes)
    imported = defaultdict(set)  # fichier → fichiers qu'il importe
    for e in edges:
        if e.get("relation") in ("imports_from", "imports", "dynamic_import"):
            s_, t_ = nodes.get(e["source"]), nodes.get(e["target"])
            if s_ and t_ and s_.get("source_file") and t_.get("source_file"):
                imported[rel(s_["source_file"])].add(rel(t_["source_file"]))
    kept, stats = [], defaultdict(int)
    for e in edges:
        if e.get("confidence") != "INFERRED" or e.get("relation") not in ("calls", "indirect_call", "references") or e.get("_origin") == "semantic":
            kept.append(e)
            continue
        s, t = nodes.get(e["source"]), nodes.get(e["target"])
        ls, lt = lang_of(s and s.get("source_file")), lang_of(t and t.get("source_file"))
        if ls and lt and ls != lt:
            stats["retires_langages_differents"] += 1
            continue
        fs, ft = rel((s or {}).get("source_file")), rel((t or {}).get("source_file"))
        if fs and ft and fs != ft and ft not in imported[fs] and not rust_path(fs, ft, (t or {}).get("label", "")):
            stats["retires_fichier_non_importe"] += 1
            continue
        name = re.sub(r"[()]", "", (t or {}).get("label", "")).lstrip(".").strip()
        body = ""
        if e["source"] in sp:
            f, a, b = sp[e["source"]]
            body = "\n".join(lines(f)[a - 1:b - 1])
        if name and re.search(rf"\b{re.escape(name)}\b", body):
            e.update(confidence="EXTRACTED", confidence_score=1.0, verified="nom présent dans le corps du symbole d'origine")
            stats["verifies"] += 1
            kept.append(e)
        elif not body:
            e.update(confidence="AMBIGUOUS", confidence_score=0.3, note="à vérifier : position du symbole d'origine inconnue")
            stats["ambigus"] += 1
            kept.append(e)
        else:
            stats["retires_non_verifies"] += 1
    edges[:] = kept
    report.update(stats)


def resolve_imports(nodes, edges, report):
    """4. Imports Rust (`use crate::util`, `use crate::AppState`) qui ne pointent sur aucun nœud : rattachés au
    fichier du module, ou au seul symbole du même nom ; les autres sont des bibliothèques externes (laissés)."""
    by_label = defaultdict(list)
    for n in nodes.values():
        if n.get("source_file"):
            by_label[re.sub(r"[()]", "", n.get("label", "")).lower()].append(n["id"])
    stats = defaultdict(int)
    for e in edges:
        t = e["target"]
        if t in nodes or e.get("relation") not in ("imports_from", "imports"):
            continue
        lang = lang_of(e.get("source_file"))
        module = f"src_tauri_src_{t}" if lang == "rust" else None
        if module and module in nodes:
            e["target"] = module
            stats["imports_vers_module"] += 1
            continue
        same = [c for c in by_label.get(t.lower(), []) if lang_of(nodes[c].get("source_file")) == lang]
        if len(same) == 1:
            e["target"] = same[0]
            stats["imports_vers_symbole"] += 1
        else:
            stats["imports_externes"] += 1
    report.update(stats)


def decisions(edges, report):
    """5. Liens incertains relus à la main (tools/graphify_decisions.json) : retirés ou précisés."""
    path = ROOT / "tools" / "graphify_decisions.json"
    if not path.is_file():
        return
    data = json.loads(path.read_text(encoding="utf-8"))
    # Éléments renommés (ex. README réécrit en français le 2026-11-26) : les liens gardés en cache par les
    # anciennes notes visent encore l'ancien identifiant ; on les rebranche sur le nouveau.
    renames = {k: v for k, v in data.get("renommes", {}).items() if not k.startswith("_") and v in NODES}
    for e in edges:
        for side in ("source", "target"):
            if e[side] in renames and e[side] not in NODES:
                e[side] = renames[e[side]]
                n_ren = report.get("renommes_rebranches", 0) + 1
                report["renommes_rebranches"] = n_ren
    for d in data.get("liens", []):
        d["source"], d["target"] = renames.get(d["source"], d["source"]), renames.get(d["target"], d["target"])
    rules = {(d["source"], d["target"]): d for d in data.get("liens", []) if d["action"] != "ajouter"}
    kept, n = [], defaultdict(int)
    for d in data.get("liens", []):
        if d["action"] == "ajouter" and d["source"] in NODES and d["target"] in NODES:
            kept.append(edge(d["source"], d["target"], d["relation"], d.get("source_file", ""), 1, note=d.get("note", "")))
            n["decisions_ajoutes"] += 1
    for d in data.get("noeuds", []):
        if d["id"] in NODES:
            NODES[d["id"]]["note"] = d["note"]
            n["decisions_notes"] += 1
    for e in edges:
        d = rules.get((e["source"], e["target"]))
        if not d:
            kept.append(e)
        elif d["action"] == "retirer":
            n["decisions_retires"] += 1
        else:
            e.update({k: d[k] for k in ("relation", "confidence", "confidence_score", "note") if k in d})
            n["decisions_precises"] += 1
            kept.append(e)
    edges[:] = kept
    report.update(n)


def script_refs(nodes, report):
    """6. Scripts qui en lancent d'autres (package.json, tools/*.ps1, *.mjs, *.py) : graphify ne lit pas le
    PowerShell ; un nom de fichier du dépôt cité dans un script donne un lien `references` (texte : EXTRACTED)."""
    files = [p for p in (ROOT / "tools").glob("*") if p.suffix in (".ps1", ".mjs", ".py", ".ts")] + [ROOT / "package.json"]
    names = {p.name: rel(p) for p in files}
    out = []
    for p in files:
        r = rel(p)
        src = file_id(r) if file_id(r) in nodes else ("package" if p.name == "package.json" and "package" in nodes else None)
        if not src:
            continue
        done = set()
        for i, l in enumerate(lines(r)):
            for name, target in names.items():
                tid = file_id(target)
                if target != r and tid in nodes and tid not in done and re.search(rf"(?<![\w-]){re.escape(name)}\b", l):
                    done.add(tid)
                    out.append(edge(src, tid, "references", r, i + 1, via="script"))
    report["liens_scripts"] = len(out)
    return out


def attach_orphans(nodes, edges, report):
    """7. Élément sans aucun lien : rattaché au fichier qui le contient (`contains`)."""
    linked = {e["source"] for e in edges} | {e["target"] for e in edges}
    n = 0
    for nid, node in nodes.items():
        f = rel(node.get("source_file"))
        fid = file_id(f) if f else None
        if fid == "src_tauri_cargo" and "src_tauri_cargo" not in nodes:
            fid = None
        if nid not in linked and fid and fid in nodes and fid != nid:
            edges.append(edge(fid, nid, "contains", f, 1))
            n += 1
    report["orphelins_rattaches"] = n


def group_bridges(edges):
    """8. Commandes et évènements : un seul lien par paire, avec la liste complète (sinon le graphe simple n'en
    garde qu'un et les autres noms se perdent)."""
    groups, rest = {}, []
    for e in edges:
        if e.get("relation") not in ("invokes", "listens_to"):
            rest.append(e)
            continue
        k = (e["source"], e["target"], e["relation"])
        g = groups.setdefault(k, {**e, "commands": [], "events": [], "lines": []})
        if e.get("command"):
            g["commands"].append(e["command"])
        if e.get("event"):
            g["events"].append(e["event"])
        g["lines"].append(e.get("source_location"))
    for g in groups.values():
        g["commands"] = sorted(set(g["commands"]))
        g["events"] = sorted(set(g["events"]))
        g["weight"] = float(max(1, len(g["commands"]) + len(g["events"])))
        g["label"] = ", ".join(g["commands"] or g["events"])
    edges[:] = rest + list(groups.values())


def affiner(extraction):
    report = {}
    nodes = {n["id"]: n for n in extraction["nodes"]}
    edges = extraction["edges"]
    resolve_imports(nodes, edges, report)
    ghosts(nodes, edges, report)
    global NODES
    NODES = nodes
    decisions(edges, report)
    verify_inferred(nodes, edges, report)
    edges.extend(bridge(nodes, report))
    edges.extend(script_refs(nodes, report))
    group_bridges(edges)
    attach_orphans(nodes, edges, report)
    # Boucles sur soi-même créées par un rattachement : retirées
    edges[:] = [e for e in edges if e["source"] != e["target"]]
    extraction["nodes"] = list(nodes.values())
    extraction["edges"] = edges
    return extraction, report


if __name__ == "__main__":
    path = Path(sys.argv[1])
    data = json.loads(path.read_text(encoding="utf-8"))
    data, rep = affiner(data)
    path.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    (path.parent / "affinage.json").write_text(json.dumps(rep, indent=2, ensure_ascii=False), encoding="utf-8")
    print(json.dumps(rep, indent=1, ensure_ascii=False))
