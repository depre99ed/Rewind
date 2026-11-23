// Test du suivi de l'image (ripple.ts) : `npm run test:ripple`
import { newProject, normalizePiece, newText, newAudio, newFx, layout, localAt, outLen, speedAt, srcAt, mergePunches, punchAt, type Project } from "../src/lib/studio/model";
import { anchorItems, reanchor, remapZoom } from "../src/lib/studio/ripple";

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
console.log(fails ? `\n${fails} échec(s)` : "\nTout est bon.");
process.exit(fails ? 1 : 0);
