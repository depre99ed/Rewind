// Test du suivi de l'image (ripple.ts) : `npm run test:ripple`
import { newProject, normalizePiece, newText, newAudio, newFx, layout, localAt, outLen, speedAt, srcAt, mediaMotion, mergePunches, punchAt, type Project } from "../src/lib/studio/model";
import { anchorItems, reanchor, remapZoom } from "../src/lib/studio/ripple";
import { killPassages } from "../src/lib/studio/killfx";

let fails = 0;
const eq = (name: string, got: number, want: number, tol = 0.01) => {
  const ok = Math.abs(got - want) <= tol;
  if (!ok) fails++;
  console.log(`${ok ? "ok  " : "ÉCHEC"} ${name} : ${got.toFixed(3)} (attendu ${want.toFixed(3)})`);
};

function project(): Project {
  const p = newProject();
  p.pieces = [
    normalizePiece({ uid: "A", path: "a.mp4", srcDur: 10, start: 0, end: 10 }),
    normalizePiece({ uid: "B", path: "b.mp4", srcDur: 10, start: 0, end: 10 }),
  ];
  const text = { ...newText(12), uid: "T" }; // 2 s dans le clip B
  const early = { ...newText(4), uid: "E" }; // 4 s dans le clip A
  const after = { ...newText(21), uid: "Z" }; // 1 s après la fin
  const music = { ...newAudio("music", "m.mp3", "m", 60, 0, 20), uid: "M" };
  const sfx = { ...newAudio("sfx", "s.wav", "s", 1, 12, 20), uid: "S" };
  p.tracks = [
    { id: "l", kind: "layer", name: "", hidden: false, locked: false, items: [text, early, after] },
    { id: "a", kind: "audio", name: "", muted: false, locked: false, items: [music, sfx] },
  ];
  return p;
}
const at = (p: Project, uid: string) => p.tracks.flatMap((t) => t.items as { uid: string; at: number }[]).find((i) => i.uid === uid)!.at;

// 1) Ralenti au début : A passe à 0,5× (20 s) → tout ce qui suit glisse de 10 s, la musique reste à 0
{
  const p = project();
  const a = anchorItems(p);
  p.pieces[0] = { ...p.pieces[0], speed: 0.5 };
  reanchor(p, a);
  eq("ralenti 0,5× : texte sur B", at(p, "T"), 22);
  eq("ralenti 0,5× : texte sur A (même image)", at(p, "E"), 8);
  eq("ralenti 0,5× : élément après la fin", at(p, "Z"), 31);
  eq("ralenti 0,5× : bruitage suit", at(p, "S"), 22);
  eq("ralenti 0,5× : musique fixe", at(p, "M"), 0);
}
// 2) Rampe de ralenti dans A (0,3× sur 1 s autour de 2 s) → B glisse de la durée ajoutée
{
  const p = project();
  const a = anchorItems(p);
  p.pieces[0] = { ...p.pieces[0], ramp: { at: 2, dur: 1, speed: 0.3, ease: 0.4 } };
  reanchor(p, a);
  const lay = layout(p.pieces);
  eq("rampe : texte sur B suit", at(p, "T"), lay.offsets[1] + 2);
  eq("rampe : texte sur A après la rampe", at(p, "E"), localAt(p.pieces[0], 4)!);
}
// 3) Début de A rogné de 3 s → tout recule de 3 s, le texte de A garde son image
{
  const p = project();
  const a = anchorItems(p);
  p.pieces[0] = { ...p.pieces[0], start: 3 };
  reanchor(p, a);
  eq("rognage : texte sur B", at(p, "T"), 9);
  eq("rognage : texte sur A", at(p, "E"), 1);
}
// 4) A coupé en deux à 6 s (nouveau morceau A2) puis A2 passe à 2× → le texte de A (4 s) ne bouge pas
{
  const p = project();
  const a = anchorItems(p);
  p.pieces.splice(0, 1, { ...p.pieces[0], end: 6 }, { ...p.pieces[0], uid: "A2", start: 6, speed: 2 });
  reanchor(p, a);
  eq("coupe + 2× : texte avant la coupe", at(p, "E"), 4);
  eq("coupe + 2× : texte sur B recule de 2 s", at(p, "T"), 10);
}
// 5) Texte sur la 2e moitié d'un morceau coupé : il retrouve le bon morceau (même fichier)
{
  const p = project();
  (p.tracks[0].items[1] as { at: number }).at = 8; // 8 s dans A
  const a = anchorItems(p);
  p.pieces.splice(0, 1, { ...p.pieces[0], end: 6 }, { ...p.pieces[0], uid: "A2", start: 6, speed: 0.5 });
  reanchor(p, a);
  eq("coupe + ralenti : texte sur la 2e moitié", at(p, "E"), 6 + 2 / 0.5);
}
// 6) Zoom animé d'un morceau ralenti : le point clé reste sur la même image
{
  const before = normalizePiece({ uid: "A", path: "a.mp4", srcDur: 10, start: 0, end: 10, zoom: [{ t: 5, z: 1.5, x: 0.5, y: 0.5 }] });
  const after = { ...before, speed: 0.5 };
  eq("zoom : point clé à 5 s → 10 s", remapZoom(before, after)[0].t, 10);
}
// 7) Mini-ralentis sur les kills : deux ralentis sur un même morceau s'additionnent ; les textes suivent
{
  const p = project();
  const a = anchorItems(p);
  p.pieces[0] = { ...p.pieces[0], killRamps: [{ at: 2, dur: 0.5, speed: 0.5, ease: 0.2 }, { at: 6, dur: 0.5, speed: 0.5, ease: 0.2 }] };
  const one = outLen({ ...p.pieces[0], killRamps: [p.pieces[0].killRamps[0]] }) - 10;
  eq("deux mini-ralentis : durée ajoutée ×2", outLen(p.pieces[0]) - 10, one * 2);
  reanchor(p, a);
  eq("deux mini-ralentis : texte sur B suit", at(p, "T"), outLen(p.pieces[0]) + 2);
}
// 7b) Accéléré sur un kill : le morceau raccourcit d'autant que le ralenti symétrique l'allonge ; un ralenti
//     et un accéléré qui se chevauchent : le plus marqué l'emporte
{
  const p = project();
  const base = p.pieces[0];
  const fast = { ...base, killRamps: [{ at: 4, dur: 1, speed: 2, ease: 0.2 }] };
  eq("accéléré : morceau plus court (1 s jouée en 0,5 s, plus les rampes)", outLen(fast), 9.4, 0.15);
  eq("accéléré : vitesse 2 au milieu", speedAt(fast, 4), 2);
  const both = { ...base, killRamps: [{ at: 4, dur: 1, speed: 1.25, ease: 0.2 }, { at: 4.2, dur: 1, speed: 0.3, ease: 0.2 }] };
  eq("ralenti + accéléré : le plus marqué", speedAt(both, 4.2), 0.3);
}
// 7c) Courbe de vitesse : ralenti qui repart en accéléré, en douceur ; la durée suit l'intégrale de la courbe
{
  const p = project();
  const c = { ...p.pieces[0], curve: [{ t: 0, v: 0.5 }, { t: 5, v: 0.5 }, { t: 10, v: 2 }] };
  eq("courbe : ralenti au début", speedAt(c, 2), 0.5);
  eq("courbe : vitesse normale au milieu de la montée (×2 ÷2 symétrique)", speedAt(c, 7.5), 1);
  eq("courbe : accéléré à la fin", speedAt(c, 9.99), 2, 0.02);
  // Durée = intégrale de 1 / vitesse (calcul fin) ; les marches de 1/15 s de l'export doivent coller
  let exact = 0;
  for (let k = 0; k < 100000; k++) exact += 1e-4 / speedAt(c, (k + 0.5) * 1e-4);
  eq("courbe : durée du morceau = intégrale exacte", outLen(c), exact, 0.03);
  eq("courbe : aller-retour temps source ↔ montage", srcAt(c, localAt(c, 7.3)!), 7.3, 0.001);
}
// 7d) Animations d'image : arrive en tourbillon (part de rien, finit droite), tourne en continu, sort à droite
{
  const a = { in: "spin", out: "slideR", loop: "spin", dur: 0.5, speed: 1 } as const;
  eq("image : taille presque nulle au départ", mediaMotion(a, 4, 0).s, 0, 0.02);
  eq("image : un tour de moins au départ", mediaMotion(a, 4, 0).r, -360);
  eq("image : rotation continue (½ tour/s) à 2 s", mediaMotion(a, 4, 2).r, 360);
  eq("image : sortie vers la droite à la fin", mediaMotion(a, 4, 4).dx, 0.7);
  eq("image : au milieu, pas de décalage", mediaMotion(a, 4, 2).dx, 0);
}
// 8) Zoom punch : fusionné au zoom de l'export, même valeur que l'aperçu
{
  const p = project();
  const fx = { ...newFx("punch", 3, 1), dur: 0.4, strength: 0.5 };
  p.tracks[0].items.push(fx);
  const zoom: { t: number; z: number; x: number; y: number }[] = [];
  mergePunches(p, zoom);
  const peak = zoom.reduce((m, k) => Math.max(m, k.z), 0);
  eq("zoom punch : pic à +15 % (force 50 %)", peak, 1.15, 0.01);
  eq("zoom punch : aperçu = export au pic", punchAt(p, 3 + 0.4 * 0.18), 1.15, 0.001);
  eq("zoom punch : retour à 1 à la fin", zoom[zoom.length - 1].z, 1, 0.001);
}
// 9) Fin de A rognée à 3 s (2026-11-25) : un effet entièrement dans le passage retiré part avec lui ; un texte qui
//    commence dans le passage retiré mais continue sur B est posé au bord ; ce qui précède ne bouge pas
{
  const p = project();
  const fx = { ...newFx("flash", 5, 0.3), uid: "F" }; // flash à 5 s dans A : entièrement retiré
  const long = { ...newText(9), uid: "L", dur: 3 }; // 9 → 12 s : déborde sur B
  p.tracks[0].items.push(fx, long);
  const a = anchorItems(p);
  p.pieces[0] = { ...p.pieces[0], end: 3 };
  const cut = reanchor(p, a);
  eq("fin rognée : le flash du passage coupé est signalé", cut.includes("F") ? 1 : 0, 1);
  eq("fin rognée : texte qui déborde sur B gardé", cut.includes("L") ? 1 : 0, 0);
  eq("fin rognée : texte qui déborde posé au bord", at(p, "L"), 3);
  eq("fin rognée : texte sur B recule de 7 s", at(p, "T"), 5);
  eq("fin rognée : le texte E (4 → 7 s, dans le passage retiré) part aussi", cut.includes("E") ? 1 : 0, 1);
  eq("fin rognée : rien d'autre de signalé", cut.length, 2);
}
// 10) Début de A rogné à 5 s : le texte à 4 s (2 s de long, finit à 6 s) est posé au début, pas retiré ;
//     un effet de 1 à 1,5 s est retiré
{
  const p = project();
  const fx = { ...newFx("flash", 1, 0.5), uid: "F", dur: 0.5 };
  p.tracks[0].items.push(fx);
  (p.tracks[0].items[1] as { dur: number }).dur = 2; // texte E : 4 → 6 s
  const a = anchorItems(p);
  p.pieces[0] = { ...p.pieces[0], start: 5 };
  const cut = reanchor(p, a);
  eq("début rogné : effet du passage coupé signalé", cut.includes("F") ? 1 : 0, 1);
  eq("début rogné : texte à cheval gardé", cut.includes("E") ? 1 : 0, 0);
  eq("début rogné : texte à cheval posé au début", at(p, "E"), 0);
}
// 11) Morceau A dupliqué (copie D à la fin) : rogner la fin de A retire l'effet du passage coupé au lieu de le
//     faire sauter sur la copie ; une vraie coupe (A2 nouveau) le garde sur A2
{
  const p = project();
  p.pieces.push({ ...p.pieces[0], uid: "D" });
  const fx = { ...newFx("flash", 8, 0.3), uid: "F" };
  p.tracks[0].items.push(fx);
  const known = new Set(p.pieces.map((x) => x.uid));
  const a = anchorItems(p);
  p.pieces[0] = { ...p.pieces[0], end: 5 };
  const cut = reanchor(p, a, known);
  eq("copie : effet du passage coupé retiré (pas déplacé sur la copie)", cut.includes("F") ? 1 : 0, 1);
  const q = project();
  q.tracks[0].items.push({ ...newFx("flash", 8, 0.3), uid: "F" });
  const known2 = new Set(q.pieces.map((x) => x.uid));
  const b = anchorItems(q);
  q.pieces.splice(0, 1, { ...q.pieces[0], end: 6 }, { ...q.pieces[0], uid: "A2", start: 6 });
  const cut2 = reanchor(q, b, known2);
  eq("vraie coupe : effet gardé sur le nouveau morceau", cut2.includes("F") ? 0 : 1, 1);
  eq("vraie coupe : effet à la même image", at(q, "F"), 8);
}
// 12) « Garder les kills » : passages autour des kills, blancs entre deux kills retirés ; un élément dans un blanc
//     part, un élément dans un passage gardé suit son image sur le nouveau morceau
{
  const o = { before: 2.5, after: 2, cutGaps: true, gap: 6 };
  const parts = killPassages([1, 8.5], 0, 10, o);
  eq("kills : deux passages", parts.length, 2, 0);
  eq("kills : 1er passage fin", parts[0].end, 3);
  eq("kills : 2e passage début", parts[1].start, 6);
  eq("kills : 3 s entre deux kills, rien ne change", killPassages([3, 6], 0, 20, o).length, 1, 0);
  eq("kills : écart sous le minimum choisi (8 s), pas de coupe", killPassages([1, 8.5], 0, 10, { ...o, gap: 8 }).length, 1, 0);
  eq("kills : blancs non coupés si l'option est décochée", killPassages([1, 18], 0, 20, { ...o, cutGaps: false }).length, 1, 0);
  eq("kills : marges choisies (1 s avant)", killPassages([5], 0, 10, { ...o, before: 1 })[0].start, 4);
  eq("kills : bornés au morceau", killPassages([0.5], 0.2, 10, o)[0].start, 0.2);
  const p = project();
  p.tracks[0].items.push({ ...newFx("flash", 8, 0.3), uid: "F" });
  const known = new Set(p.pieces.map((x) => x.uid));
  const a = anchorItems(p);
  p.pieces.splice(0, 1, ...parts.map((r, j) => ({ ...p.pieces[0], uid: j ? "A2" : "A", start: r.start, end: r.end })));
  const cut = reanchor(p, a, known);
  eq("kills : texte dans le blanc retiré", cut.includes("E") ? 1 : 0, 1);
  eq("kills : effet suit son image (3 s + 2 s)", at(p, "F"), 5);
  eq("kills : texte du clip B recalé (7 s + 2 s)", at(p, "T"), 9);
}
console.log(fails ? `\n${fails} échec(s)` : "\nTout est bon.");
process.exit(fails ? 1 : 0);
