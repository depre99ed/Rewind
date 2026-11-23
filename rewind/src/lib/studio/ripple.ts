// Éléments liés à l'image : quand la piste principale change de durée (ralenti, vitesse, coupe, arrêt sur image,
// morceau déplacé ou supprimé), les textes, effets, médias, bruitages et voix off restent sur le MÊME MOMENT de la
// vidéo au lieu de rester à la même seconde du montage. La musique ne bouge pas (calage sur le rythme).
// Décidé avec l'utilisateur le 2026-11-23 : il avait tout monté, ajouté un ralenti au début, et tout s'était décalé.
import { type AudioItem, type Layer, type Piece, type Project, layout, localAt, outLen, srcAt } from "./model";

type Item = Layer | AudioItem;

/** Où un élément est accroché : sur un morceau (au temps source `src`), ou après la fin de la piste principale. */
export type Anchor =
  | { kind: "piece"; piece: string; path: string; index: number; src: number; local: number; next: string | null }
  | { kind: "end"; delta: number };

/** Les éléments qui suivent l'image (la musique reste fixe). */
export function follows(trackKind: "layer" | "audio", it: Item): boolean {
  return trackKind === "layer" || (it as AudioItem).kind !== "music";
}

/** Morceau visible à l'instant t (pendant une transition : celui qui arrive), -1 après la fin. */
function pieceAt(offsets: number[], lens: number[], t: number): number {
  let k = -1;
  for (let i = 0; i < offsets.length; i++) if (offsets[i] <= t + 1e-6) k = i;
  if (k >= 0 && t > offsets[k] + lens[k] + 1e-6) return -1;
  return k;
}

/** Accroche de chaque élément qui suit l'image, d'après la piste principale actuelle. */
export function anchorItems(p: Project): Map<string, Anchor> {
  const lay = layout(p.pieces);
  const out = new Map<string, Anchor>();
  for (const tr of p.tracks) {
    for (const it of tr.items as Item[]) {
      if (!follows(tr.kind, it)) continue;
      const k = pieceAt(lay.offsets, lay.lens, it.at);
      if (k < 0) {
        if (p.pieces.length && it.at >= lay.total - 1e-6) out.set(it.uid, { kind: "end", delta: it.at - lay.total });
        continue;
      }
      const pc = p.pieces[k];
      const local = it.at - lay.offsets[k];
      out.set(it.uid, { kind: "piece", piece: pc.uid, path: pc.path, index: k, src: srcAt(pc, local), local, next: p.pieces[k + 1]?.uid ?? null });
    }
  }
  return out;
}

/** La piste principale a-t-elle changé de minutage ? (sinon rien à recaler) */
export function timingKey(pieces: Piece[]): string {
  return pieces.map((x) => `${x.uid}:${x.start}:${x.end}:${x.speed}:${x.hold}:${x.kind}:${x.transition}:${x.transitionDur}:${JSON.stringify(x.ramp)}:${JSON.stringify(x.killRamps ?? [])}:${JSON.stringify(x.curve ?? [])}`).join("|");
}

/**
 * Nouveau départ d'un élément après le changement. Le morceau d'origine d'abord ; s'il a été coupé en deux, le
 * morceau du même fichier qui contient ce moment (le plus proche de l'ancienne place) ; s'il a été supprimé,
 * l'élément ne bouge pas (la vidéo sous lui a disparu : c'est à l'utilisateur de décider).
 */
function newStart(a: Anchor, p: Project, lay: ReturnType<typeof layout>, oldAt: number): number {
  if (a.kind === "end") return Math.max(0, lay.total + a.delta);
  const at = (i: number, local: number) => lay.offsets[i] + Math.max(0, Math.min(local, lay.lens[i]));
  const same = p.pieces.findIndex((x) => x.uid === a.piece);
  if (same >= 0) {
    const pc = p.pieces[same];
    if (pc.kind === "freeze") return at(same, Math.min(a.local, outLen(pc)));
    const l = localAt(pc, a.src);
    if (l !== null) return at(same, l);
  }
  // Coupé, rogné ou dupliqué : le morceau du même fichier qui montre encore ce moment
  let best = -1;
  p.pieces.forEach((pc, i) => {
    if (pc.kind !== "clip" || pc.path !== a.path || a.src < pc.start - 1e-6 || a.src > pc.end + 1e-6) return;
    if (best < 0 || Math.abs(i - a.index) < Math.abs(best - a.index)) best = i;
  });
  if (best >= 0) return at(best, localAt(p.pieces[best], a.src) ?? 0);
  // Moment rogné : l'élément se pose au bord du morceau qui reste
  if (same >= 0) {
    const pc = p.pieces[same];
    return at(same, a.src < pc.start ? 0 : outLen(pc));
  }
  return oldAt;
}

/** Recale les éléments accrochés ; renvoie le nombre d'éléments déplacés. */
export function reanchor(p: Project, anchors: Map<string, Anchor>): number {
  const lay = layout(p.pieces);
  let moved = 0;
  for (const tr of p.tracks) {
    for (const it of tr.items as Item[]) {
      const a = anchors.get(it.uid);
      if (!a) continue;
      const at = Math.round(newStart(a, p, lay, it.at) * 1000) / 1000;
      if (Math.abs(at - it.at) > 1e-3) {
        it.at = at;
        moved++;
      }
    }
  }
  return moved;
}

/**
 * Zooms animés d'un morceau (points clés en temps local de sortie) : quand sa vitesse ou son ralenti change,
 * chaque point reste sur la même image du clip.
 */
export function remapZoom(before: Piece, after: Piece): Piece["zoom"] {
  if (!after.zoom.length || after.kind === "freeze") return after.zoom;
  return after.zoom
    .map((k) => {
      const src = srcAt(before, k.t);
      const t = localAt(after, src);
      return t === null ? null : { ...k, t: Math.round(t * 1000) / 1000 };
    })
    .filter((k): k is Piece["zoom"][number] => k !== null);
}
