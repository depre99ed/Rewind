// Animation d'ouverture de Rewind (2026-11-26) : l'icône (le W vert sur sa plaque noire) en 3D,
// dessinée en caractères lumineux, comme les rendus « glowing ASCII » montrés par l'utilisateur.
//
// Scénario « rembobinage » validé par l'utilisateur (moins de 5 s au total) :
//   0,00 → 0,42 s  une poussière de caractères dérive (la fin d'une explosion) ;
//   0,42 → 1,88 s  le temps repart en arrière : chaque grain revient sur sa trajectoire, de plus en plus
//                  vite, avec une traînée, et l'icône se reforme en tournant (une vidéo qu'on rembobine) ;
//   1,88 → 3,35 s  l'icône se fige face à soi, un reflet la balaie, « rewind » se tape dessous ;
//   3,35 → 4,60 s  le vent l'emporte en poussière vers la gauche pendant que l'interface apparaît.
//
// Rendu WebGL 2, tout sur la carte graphique (le processeur ne fait que l'horloge) :
//   1. ~42 000 grains posés sur l'icône sont projetés dans une grille basse résolution : une case = un
//      caractère ; un tampon de profondeur garde le grain le plus proche (surfaces pleines, comme en 3D) ;
//   2. la grille est floutée deux fois (halo serré, halo large) ;
//   3. une passe plein écran dessine dans chaque case le caractère dont « l'encre » correspond à la
//      luminosité (rampe triée sur Geist Mono, la police de l'appli), un léger halo par caractère, puis
//      ajoute les halos. La sortie est prémultipliée : à la fin, la poussière passe au-dessus de l'appli.
// En cas d'échec (pas de WebGL 2, contexte perdu…), l'intro est simplement passée.

/** Tracé du W (design/logo/rewind-symbole.svg), placé comme dans l'icône (rewind-icone.svg). */
const W_PATH =
  "M604.14 515.95C634.88 512.82 660.45 500.95 680.62 480.45C688.68 472.26 698.59 458.61 701.45 451.77C702.27 449.81 721.91 410.02 745.09 363.35C842.3 167.65 835.17 183.43 835.26 164C835.31 151.13 833.54 143.63 827.56 131.5C817.16 110.38 798.71 96.23 775.46 91.53C768.48 90.12 760.16 89.95 712.46 90.24C658.61 90.57 657.35 90.62 650.02 92.86C619.56 102.13 598.64 127.98 596.3 159.23C595.66 167.75 597.17 179.32 599.97 187.35C600.96 190.18 608.4 206 616.51 222.5C675.47 342.56 678.15 348.26 684.61 367.35C693.74 394.39 695.56 422.67 688.67 430.55C686.51 433.02 686.48 433.03 683.02 431.27C681.11 430.3 677.81 427.25 675.7 424.5C673.58 421.75 657.2 390.02 639.3 354C606.95 288.9 586.77 247.8 532.19 135.82C522.61 116.18 521.74 114.77 514.6 107.63C505.91 98.94 496.56 93.84 484.83 91.4C473.85 89.11 385.12 89.87 376.88 92.32C362.78 96.52 351.27 103.58 341.93 113.76C331.93 124.67 326.18 136.52 323.38 152C321.92 160.1 321.84 162.85 322.82 170.65C324.11 180.92 327.48 191.13 333 201.5C347.57 228.89 395.76 323.45 426.59 385.16C440.51 413.02 453.57 438.44 455.61 441.66C466.78 459.29 485.38 478.48 502.07 489.6C533.89 510.78 568.12 519.61 604.14 515.95ZM332.08 513.98C374.42 505.36 406.76 477.24 421.23 436.5C422.6 432.65 424.01 428.26 424.38 426.75L425.04 424L421.11 425.57C417.6 426.98 416.87 426.98 414.29 425.65C406.03 421.38 395.4 405.37 378.33 371.5C371.4 357.75 363.37 342 360.49 336.5C352.08 320.48 301.11 220.13 265.01 148.5C199.28 18.11 202.28 23.59 191.82 14.81C185.02 9.1 180.96 6.69 172.4 3.26L165.5 0.5L105.12 0.21C63.43 0 43.08 0.26 39.36 1.03C17.37 5.59 0.06 26.17 0.01 47.79C-0.01 59.4 1.52 62.76 62.5 184.45C156.38 371.76 190.91 439.82 197.1 449.74C216.94 481.55 246.45 503.93 279.81 512.47C295.62 516.52 316.66 517.13 332.08 513.98Z";

/** Déroulé de l'intro, en secondes (≈ 8 s, choisi par l'utilisateur le 2026-11-27). */
export const TL = {
  rewind: 0.8, // le temps repart en arrière
  assembled: 3.1, // l'icône est reformée
  sweep: 3.35, // reflet qui balaie l'icône
  typeStart: 3.7, // « rewind » se tape lettre par lettre, avec un petit tick par lettre
  glitch: 5.55, // le bug : caractères qui se corrompent, lignes qui sautent
  dissolve: 6.55, // départ en poussière
  reveal: 6.8, // l'interface commence à apparaître
  revealDur: 0.8,
  land: 7.5, // la poussière a reformé le petit W et le mot est posé : le vrai logo prend le relais
  end: 8.0,
};

/** Instant d'apparition de chaque lettre de « rewind » (rythme humain, un peu irrégulier). */
export const TYPE_TIMES = [0, 0.21, 0.39, 0.62, 0.81, 1.03].map((d) => TL.typeStart + d);

const hash1 = (n: number) => {
  const x = Math.sin(n * 127.1 + 311.7) * 43758.5453;
  return x - Math.floor(x);
};

/** Intensité du bug (0 → 1) : montée nette, sursauts irréguliers, de plus en plus fort, puis relais à la dispersion. */
export function glitchAt(t: number) {
  if (t < TL.glitch || t > TL.dissolve + 0.35) return 0;
  const k = Math.floor(t * 26);
  const r = hash1(k);
  const env = smooth(TL.glitch, TL.glitch + 0.1, t) * (1 - smooth(TL.dissolve - 0.05, TL.dissolve + 0.35, t));
  const ramp = 0.45 + 0.55 * smooth(TL.glitch, TL.dissolve, t);
  const burst = r < 0.2 ? 0.2 : r > 0.8 ? 1.3 : 1;
  return clamp(env * ramp * burst, 0, 1);
}

/** Mise en page commune au moteur et au mot « rewind » (pixels CSS). */
export function introLayout(w: number, h: number) {
  const iconPx = Math.max(120, Math.min(h * 0.54, w * 0.42)); // hauteur de la plaque, bord à bord
  const cy = h * 0.452;
  const wordSize = Math.round(Math.max(16, Math.min(40, h * 0.0357)));
  const wordY = cy + iconPx * 0.5 + h * 0.07 + wordSize * 0.62;
  return { cx: w / 2, cy, iconPx, wordSize, wordY };
}

const STREAK = "-=~-_=";
const DUST = ".'`,:*+^";
const GLITCH = "#%&@$*/\\|<>[]{}!?=+~^";
const ROWS = 100; // lignes de caractères sur la hauteur de la fenêtre (cases de 10 à 15 px d'écran)
const PAD = 3; // marge autour de chaque caractère dans l'atlas (le halo ne déborde pas sur le voisin)
const MONO = '"Geist Mono Variable", ui-monospace, monospace';

function clamp(v: number, a: number, b: number) {
  return Math.min(b, Math.max(a, v));
}
function smooth(a: number, b: number, v: number) {
  const x = clamp((v - a) / (b - a), 0, 1);
  return x * x * (3 - 2 * x);
}
/** Trajet d'une explosion freinée (0 → 1) ; rejouée à l'envers, elle donne l'accélération du « rembobinage ». */
const explode = (tau: number) => (1 - Math.exp(-4.2 * tau)) / (1 - Math.exp(-4.2));

function mulberry32(seed: number) {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ---------------------------------------------------------------------------------------------
// Géométrie : des grains posés sur l'icône en volume (dessus du W, plaque, liseré, parois)
// ---------------------------------------------------------------------------------------------

/** Matériaux : 0 dessus du W, 1 paroi du W, 2 face de la plaque, 3 liseré, 4 paroi de la plaque, 5 poussière. */
function buildGeometry(): Float32Array {
  const S = 512;
  const k = S / 1024;
  const mask = (draw: (g: CanvasRenderingContext2D) => void) => {
    const c = document.createElement("canvas");
    c.width = c.height = S;
    const g = c.getContext("2d", { willReadFrequently: true })!;
    draw(g);
    const d = g.getImageData(0, 0, S, S).data;
    const a = new Uint8Array(S * S);
    for (let i = 0; i < a.length; i++) a[i] = d[i * 4 + 3];
    return a;
  };
  const tile = mask((g) => {
    g.beginPath();
    g.roundRect(16 * k, 16 * k, 992 * k, 992 * k, 228 * k);
    g.fill();
  });
  const wm = mask((g) => {
    g.setTransform(0.88191 * k, 0, 0, 0.88191 * k, 143.36 * k, 294.27 * k);
    g.fill(new Path2D(W_PATH));
  });
  const at = (m: Uint8Array, x: number, y: number) => (x < 0 || y < 0 || x >= S || y >= S ? 0 : m[(y | 0) * S + (x | 0)]);
  // Bords d'un masque, avec la normale sortante (dans le repère du masque, y vers le bas)
  const edges = (m: Uint8Array) => {
    const out: number[] = [];
    for (let y = 3; y < S - 3; y++)
      for (let x = 3; x < S - 3; x++) {
        const i = y * S + x;
        if (m[i] < 128) continue;
        if (m[i + 1] >= 128 && m[i - 1] >= 128 && m[i + S] >= 128 && m[i - S] >= 128) continue;
        const gx = m[i + 3] - m[i - 3], gy = m[i + 3 * S] - m[i - 3 * S];
        const l = Math.hypot(gx, gy) || 1;
        out.push(x + 0.5, y + 0.5, -gx / l, -gy / l);
      }
    return out;
  };

  const rnd = mulberry32(20261126);
  const data: number[] = [];
  const toX = (px: number) => (px / S) * 2 - 1;
  const toY = (py: number) => 1 - (py / S) * 2;
  const push = (x: number, y: number, z: number, nx: number, ny: number, nz: number, mat: number) =>
    data.push(x, y, z, nx, ny, nz, mat, rnd(), rnd(), rnd(), rnd());
  const T = 0.16, E = 0.075, zf = T / 2; // épaisseur de la plaque, relief du W

  // Faces : dessus du W, face de la plaque et son liseré (le filet clair de l'icône)
  let nW = 0, nT = 0;
  const wantW = 7000, wantT = 16000;
  for (let guard = 0; (nW < wantW || nT < wantT) && guard < 400000; guard++) {
    const x = rnd() * S, y = rnd() * S;
    if (at(tile, x, y) < 128) continue;
    if (at(wm, x, y) >= 128) {
      if (nW < wantW) push(toX(x), toY(y), zf + E, 0, 0, 1, 0), nW++;
    } else if (nT < wantT) {
      const r = 7;
      const rim = at(tile, x + r, y) < 128 || at(tile, x - r, y) < 128 || at(tile, x, y + r) < 128 || at(tile, x, y - r) < 128;
      // le liseré gagne sur la face dans une même case (légèrement devant)
      push(toX(x), toY(y), rim ? zf + 0.004 : zf, 0, 0, 1, rim ? 3 : 2), nT++;
    }
  }
  // Parois : contour extrudé
  const te = edges(tile), we = edges(wm);
  for (let i = 0; i < 12000; i++) {
    const j = ((rnd() * (te.length / 4)) | 0) * 4;
    push(toX(te[j]), toY(te[j + 1]), -zf + rnd() * T, te[j + 2], -te[j + 3], 0, 4);
  }
  for (let i = 0; i < 6000; i++) {
    const j = ((rnd() * (we.length / 4)) | 0) * 4;
    push(toX(we[j]), toY(we[j + 1]), zf + rnd() * E, we[j + 2], -we[j + 3], 0, 1);
  }
  // Poussière décorative : se fond dans l'icône pendant le rembobinage
  for (let i = 0; i < 2500; ) {
    const x = rnd() * S, y = rnd() * S;
    if (at(tile, x, y) < 128) continue;
    push(toX(x), toY(y), (rnd() - 0.5) * 0.2, 0, 0, 1, 5);
    i++;
  }
  return new Float32Array(data);
}

// ---------------------------------------------------------------------------------------------
// Atlas des caractères : une rangée de cases, R = caractère net, G = son halo
// ---------------------------------------------------------------------------------------------

function buildAtlas(cellW: number, cellH: number) {
  // Rampe : caractères imprimables triés par quantité d'encre réelle dans Geist Mono
  const probe = document.createElement("canvas");
  probe.width = probe.height = 48;
  const pg = probe.getContext("2d", { willReadFrequently: true })!;
  pg.font = `700 32px ${MONO}`;
  pg.textAlign = "center";
  pg.textBaseline = "middle";
  pg.fillStyle = "#fff";
  const ranked: { ch: string; ink: number }[] = [];
  for (let c = 33; c < 127; c++) {
    const ch = String.fromCharCode(c);
    pg.clearRect(0, 0, 48, 48);
    pg.fillText(ch, 24, 24);
    const d = pg.getImageData(0, 0, 48, 48).data;
    let ink = 0;
    for (let i = 3; i < d.length; i += 4) ink += d[i];
    ranked.push({ ch, ink });
  }
  ranked.sort((a, b) => a.ink - b.ink);
  const ramp = ranked.map((r) => r.ch);
  // Table densité → caractère : le premier dont l'encre atteint la densité voulue
  const maxInk = ranked[ranked.length - 1].ink || 1;
  const lut = new Float32Array(64);
  for (let j = 0, i = 0; j < 64; j++) {
    while (i < ranked.length - 1 && ranked[i].ink / maxInk < j / 63) i++;
    lut[j] = i;
  }
  const glyphs = [...ramp, ...STREAK, ...DUST, ...GLITCH];

  const stride = cellW + PAD * 2, height = cellH + PAD * 2;
  const cv = document.createElement("canvas");
  cv.width = stride * glyphs.length;
  cv.height = height;
  const g = cv.getContext("2d", { willReadFrequently: true })!;
  const fontPx = Math.min(cellH / 1.05, cellW / 0.6);
  // Gras : plus d'encre par case, donc un vert plus plein
  g.font = `700 ${fontPx}px ${MONO}`;
  g.textAlign = "center";
  g.textBaseline = "middle";
  g.fillStyle = "#fff";
  glyphs.forEach((ch, i) => g.fillText(ch, i * stride + PAD + cellW / 2, PAD + cellH * 0.54));
  const halo = document.createElement("canvas");
  halo.width = cv.width;
  halo.height = cv.height;
  const hg = halo.getContext("2d", { willReadFrequently: true })!;
  hg.filter = `blur(${Math.max(1, cellH * 0.11).toFixed(2)}px)`;
  hg.drawImage(cv, 0, 0);
  const a = g.getImageData(0, 0, cv.width, cv.height).data;
  const b = hg.getImageData(0, 0, cv.width, cv.height).data;
  const px = new Uint8Array(cv.width * cv.height * 4);
  for (let i = 0; i < cv.width * cv.height; i++) {
    px[i * 4] = a[i * 4 + 3];
    px[i * 4 + 1] = Math.min(255, b[i * 4 + 3] * 1.6);
    px[i * 4 + 3] = 255;
  }
  return {
    px,
    width: cv.width,
    height: cv.height,
    stride,
    lut,
    rampN: ramp.length,
    sets: [ramp.length, STREAK.length, ramp.length + STREAK.length, DUST.length] as [number, number, number, number],
    glitchSet: [ramp.length + STREAK.length + DUST.length, GLITCH.length] as [number, number],
  };
}

// ---------------------------------------------------------------------------------------------
// Shaders
// ---------------------------------------------------------------------------------------------

const VS_GRAINS = `#version 300 es
precision highp float;
layout(location = 0) in vec3 aHome;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in float aMat;
layout(location = 3) in vec4 aSeed;
layout(location = 4) in float aEnd;   // 0 = le grain, 1 = le bout de sa traînée

uniform float uT;
uniform float uLag;       // retard du bout de traînée (s)
uniform float uLine;      // 1 pendant la passe des traînées
uniform mat3 uRot;
uniform vec2 uCenter;     // centre de l'icône, en cases
uniform vec2 uCell;       // taille d'une case (px)
uniform vec2 uGrid;       // colonnes, lignes
uniform float uScale;     // px par unité du modèle
uniform vec3 uLight;
uniform float uSweep;
uniform float uFlash;
uniform float uRewind;
uniform float uDissolve;
uniform vec2 uTarget;     // logo de la barre du haut, en unités du modèle (repère caméra)
uniform float uTs;        // taille du petit logo / taille du W de l'icône (la poussière y reforme le W)
uniform float uLand;      // instant où le vrai logo prend le relais

out vec4 vCol;

const vec3 LIME = vec3(0.784, 0.941, 0.192);
const vec3 GREY = vec3(0.60, 0.62, 0.67);

float explode(float tau) { return (1.0 - exp(-4.2 * tau)) / (1.0 - exp(-4.2)); }
vec3 rotZ(vec3 v, float a) { float c = cos(a), s = sin(a); return vec3(c * v.x - s * v.y, s * v.x + c * v.y, v.z); }

// Position du grain à l'instant t (repère caméra) ; tau = 1 dispersé, 0 en place ; sd = temps depuis le départ au vent
vec3 posAt(float t, vec3 hw, out float tau, out float sd) {
  float start = uRewind + aSeed.x * 0.25;
  float dur = 1.9 + aSeed.y * 0.35;
  tau = t < start ? clamp(1.0 - (start - t) * 0.05, 0.0, 1.0) : clamp(1.0 - (t - start) / dur, 0.0, 1.0);
  float e = explode(tau);
  vec3 r = aSeed.yzw * 2.0 - 1.0;
  vec3 dir = normalize(vec3(hw.xy * 1.15, hw.z * 0.4) + r * vec3(1.1, 0.8, 1.3) + vec3(0.0, 0.0, 0.0001));
  float far = mix(0.7, 2.9, pow(aSeed.z, 0.7)) * (aMat > 4.5 ? 1.3 : 1.0);
  vec3 p = hw + rotZ(dir * far * e, (aSeed.w - 0.5) * 2.4 * e);
  // Fin : l'icône se défait et sa poussière file en un flux serré vers le petit logo de la barre du haut,
  // en commençant par le côté le plus proche ; chaque grain vise SA place dans le petit W (le nuage garde
  // la forme du W en rétrécissant et le reforme à la place exacte du logo). Arrivée de tous avant uLand.
  vec3 tgt = vec3(uTarget + (aHome.xy - vec2(0.0, -0.02)) * uTs, -0.3);
  float dist = length(tgt.xy - hw.xy);
  float td = uDissolve + clamp(dist / 4.0, 0.0, 1.0) * 0.2 + aSeed.x * 0.1;
  sd = t - td;
  if (sd > 0.0) {
    float k = clamp(sd / mix(0.45, 0.6, aSeed.y), 0.0, 1.0);
    float e2 = k * k * (3.0 - 2.0 * k);
    vec3 d = tgt - p;
    vec2 perp = normalize(vec2(-d.y, d.x) + 0.0001) * (aSeed.w - 0.5) * 0.35;
    vec3 mid = mix(p, tgt, 0.45) + vec3(perp, (aSeed.z - 0.5) * 0.2);
    vec3 a1 = mix(p, mid, e2), a2 = mix(mid, tgt, e2);
    p = mix(a1, a2, e2);
  }
  p.z = min(p.z, 4.5);
  return p;
}

vec2 toGrid(vec3 p) { return uCenter + p.xy * (6.0 / (6.0 - p.z)) * uScale / uCell; }

void main() {
  vec3 hw = uRot * aHome;
  float tau, sd;
  vec3 p = posAt(uT, hw, tau, sd);
  vec2 g = toGrid(p);
  float depthZ = p.z;
  float moving = smoothstep(0.0, 0.12, tau);
  bool hidden = false;

  vec3 n = uRot * aNormal;
  float dif = max(dot(n, uLight), 0.0);
  float spec = pow(max(dot(n, normalize(uLight + vec3(0.0, 0.0, 1.0))), 0.0), 28.0);
  float glint = exp(-pow((hw.x * 0.8 + hw.y * 0.55 - uSweep) * 5.0, 2.0));
  vec3 col; float lit; float fly;
  // Couleurs de l'icône : W au vert exact (le reflet l'éclaircit sans changer sa teinte), plaque NOIRE
  // (seul son contour fin est dessiné, en gris discret), parois sombres pour sentir l'épaisseur.
  if (aMat < 0.5)      { col = LIME; lit = 0.84 + 0.06 * dif + 0.1 * spec + glint * 0.3; fly = 0.95; }
  else if (aMat < 1.5) { col = LIME; lit = 0.34 + 0.55 * dif; fly = 0.8; }
  else if (aMat < 2.5) { col = GREY; lit = 0.0; fly = 0.14; }
  else if (aMat < 3.5) { col = GREY; lit = 0.3 + 0.18 * dif + glint * 0.12; fly = 0.3; }
  else if (aMat < 4.5) { col = GREY; lit = 0.05 + 0.22 * dif; fly = 0.22; }
  else                 { col = aSeed.w > 0.72 ? LIME : GREY * 1.25; lit = 0.0; fly = 0.42 + 0.3 * aSeed.x; }
  float b = mix(lit, fly * mix(1.0, 0.42, tau), moving);
  // Loin de l'icône, seul un grain sur sept est visible (poussière clairsemée) ; tous s'allument en approchant
  float sparse = fract(aSeed.y * 7.13 + aSeed.w * 3.71) < 0.14 ? 1.0 : 0.0;
  b *= mix(1.0, max(sparse, 1.0 - smoothstep(0.17, 0.4, tau)), moving);
  b *= smoothstep(0.0, 0.32, uT);                     // la poussière apparaît
  b *= 1.0 + uFlash * 0.55;                            // éclat quand l'icône se referme
  b *= clamp(1.0 + p.z * 0.16, 0.45, 1.25);            // profondeur : ce qui est loin est plus sombre
  if (aMat > 4.5) b *= smoothstep(0.0, 0.3, tau);      // la poussière décorative se fond dans l'icône
  float kind = (moving > 0.6 && b < 0.5) ? 0.33 : 1.0; // 1 = caractère selon la lumière, 0,33 = poussière
  if (sd > 0.0) {
    // La surface se défait : les grains du W filent jusqu'au logo et y restent allumés (le petit W reformé)
    // jusqu'à ce que le vrai logo prenne le relais ; quelques grains gris accompagnent le flux puis
    // s'éteignent en route ; le reste s'éteint vite après un petit éclat
    bool shape = aMat < 1.5;
    bool haze = !shape && sparse > 0.5 && fract(aSeed.z * 5.31 + aSeed.x * 2.17) < 0.5;
    float k = clamp(sd / mix(0.45, 0.6, aSeed.y), 0.0, 1.0);
    b *= 1.0 + 0.9 * exp(-sd * 14.0);
    if (shape) b *= 1.0 - smoothstep(uLand - 0.04, uLand + 0.08, uT);
    else if (haze) b *= 1.0 - smoothstep(0.35, 0.8, k);
    else b *= 1.0 - smoothstep(0.0, mix(0.15, 0.3, aSeed.z), sd);
    if (sd > 0.05 && b < 0.55) kind = 0.33;
    // Presque arrivés, les grains du W deviennent de la poussière scintillante (sinon l'amas de 2-3 cases
    // se lit comme des lettres) ; le vrai logo prend le relais juste après
    if (shape && k > 0.8) kind = 0.33;
  }
  // Faces tournées vers l'arrière : masquées quand l'icône est formée. La face noire de la plaque reste
  // dessinée (en noir) tant que l'icône est posée : elle cache ce qui est derrière, comme un vrai objet.
  bool blackFace = aMat > 1.5 && aMat < 2.5 && moving < 0.5 && sd <= 0.0;
  if ((b < 0.012 && !blackFace) || (aMat < 4.5 && moving < 0.5 && sd <= 0.0 && n.z < -0.08)) hidden = true;

  if (uLine > 0.5) {
    // Traînée : seulement pour les grains rapides (sinon des tirets apparaîtraient sur l'icône posée).
    // Les deux bouts décident pareil (valeurs de la tête) : jamais de segment à moitié coupé.
    float tau2, sd2;
    vec3 q = posAt(uT - uLag, hw, tau2, sd2);
    vec2 g2 = toGrid(q);
    if (distance(g, g2) < 1.5 || (moving < 0.3 && sd <= 0.0)) hidden = true;
    kind = 0.66;
    b *= mix(0.6, 0.1, aEnd);
    if (aEnd > 0.5) { g = g2; depthZ = q.z; }
  }

  gl_Position = hidden ? vec4(2.0, 2.0, 2.0, 1.0) : vec4(g / uGrid * 2.0 - 1.0, clamp(-depthZ * 0.08, -0.99, 0.99), 1.0);
  gl_PointSize = 1.0;
  // Luminosité bornée pour garder la teinte (sinon le vert sature canal par canal et vire au jaune)
  float peak = max(col.r, max(col.g, col.b));
  vCol = vec4(col * min(b, 1.0 / peak), kind);
}`;

const FS_GRAINS = `#version 300 es
precision highp float;
in vec4 vCol;
out vec4 o;
void main() { o = vCol; }`;

const VS_FULL = `#version 300 es
out vec2 vUv;
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  vUv = p;
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

// Flou gaussien séparable (9 points en 5 lectures)
const FS_BLUR = `#version 300 es
precision highp float;
uniform sampler2D uSrc;
uniform vec2 uStep;
in vec2 vUv;
out vec4 o;
void main() {
  vec3 c = texture(uSrc, vUv).rgb * 0.2270270270;
  c += texture(uSrc, vUv + uStep * 1.3846153846).rgb * 0.3162162162;
  c += texture(uSrc, vUv - uStep * 1.3846153846).rgb * 0.3162162162;
  c += texture(uSrc, vUv + uStep * 3.2307692308).rgb * 0.0702702703;
  c += texture(uSrc, vUv - uStep * 3.2307692308).rgb * 0.0702702703;
  o = vec4(c, 1.0);
}`;

const FS_FINAL = `#version 300 es
precision highp float;
uniform sampler2D uGridTex;
uniform sampler2D uGlowA;
uniform sampler2D uGlowB;
uniform sampler2D uAtlas;
uniform vec2 uCell;
uniform vec2 uGrid;
uniform float uStride;
uniform float uPad;
uniform float uRampN;
uniform float uLut[64]; // densité voulue → caractère de la rampe (selon l'encre réelle de chaque caractère)
uniform vec4 uSets;     // début et nombre des traînées, début et nombre de la poussière
uniform float uT;
uniform float uTrack;   // bandes de « rembobinage » (0..1)
uniform float uBloom;
uniform float uFade;
uniform float uGlitch;     // intensité du bug (0..1)
uniform vec4 uIcon;        // centre et demi-taille de l'icône, en cases
uniform vec2 uGlitchSet;   // début et nombre des caractères du bug dans l'atlas
out vec4 o;

const vec3 LIME = vec3(0.833, 1.0, 0.204);   // #c8f031 ramené à sa teinte (canal le plus fort = 1)
const vec3 WHITE = vec3(0.93, 0.95, 0.98);

float hash(vec2 p) { p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }

void main() {
  vec2 px = floor(gl_FragCoord.xy);
  float row = floor(px.y / uCell.y);
  // Quelques bandes de lignes glissent de côté pendant la phase la plus rapide (bande VHS qui rembobine)
  float band = floor(row / 2.0);
  float tick = floor(uT * 24.0);
  float on = step(0.86, hash(vec2(band, tick)));
  px.x = floor(px.x + on * uTrack * (hash(vec2(band * 1.7, tick + 3.0)) - 0.5) * uCell.x * 10.0);
  // Bug : des paquets de lignes sautent franchement de côté
  float gt = floor(uT * 26.0);
  if (uGlitch > 0.0) {
    float gband = floor(row / 3.0);
    float jump = step(1.0 - 0.4 * uGlitch, hash(vec2(gband * 2.31, gt + 17.0)));
    px.x = floor(px.x + jump * (hash(vec2(gband * 0.71, gt * 1.3 + 5.0)) - 0.5) * uCell.x * 30.0 * uGlitch);
  }

  vec2 cell = floor(px / uCell);
  vec2 inC = px - cell * uCell;
  vec3 core = vec3(0.0), halo = vec3(0.0), fill = vec3(0.0);
  float a = 0.0;
  if (cell.x >= 0.0 && cell.y >= 0.0 && cell.x < uGrid.x && cell.y < uGrid.y) {
    vec4 g = texelFetch(uGridTex, ivec2(cell), 0);
    float m = max(g.r, max(g.g, g.b));
    // Bug sur TOUT l'écran (demande de l'utilisateur) : cases corrompues (caractères au hasard, vert ou
    // blanc), parasites partout (un peu plus denses près de l'icône), lignes entières et blocs de
    // parasites, petits blocs éteints
    bool drop = false, wild = false;
    vec3 wildCol = LIME;
    if (uGlitch > 0.0) {
      drop = hash(floor(cell / vec2(7.0, 2.0)) * 1.93 + gt * 0.37) < 0.07 * uGlitch;
      float r1 = hash(cell * 1.17 + gt * 0.913);
      vec2 dn = abs(cell - uIcon.xy) / max(uIcon.zw, vec2(1.0));
      float near = 1.0 - smoothstep(1.0, 2.6, max(dn.x, dn.y));
      bool rowJunk = hash(vec2(row * 0.37, gt * 2.1 + 9.0)) < 0.05 * uGlitch && hash(cell * 3.1 + gt) < 0.6;
      bool blockJunk = hash(floor(cell / vec2(12.0, 3.0)) * 2.7 + gt * 0.53) < 0.05 * uGlitch && hash(cell * 1.9 + gt) < 0.7;
      wild = m > 0.015 ? r1 < 0.5 * uGlitch : (r1 < (0.05 + 0.07 * near) * uGlitch || rowJunk || blockJunk);
      wildCol = (hash(cell * 0.53 + gt * 1.71) < 0.35 ? WHITE : LIME) * (0.55 + 0.45 * hash(cell + gt));
    }
    if (!drop && (m > 0.015 || wild)) {
      // Le caractère change de temps en temps, comme un terminal vivant
      float h = hash(cell + floor(uT * 8.0 + hash(cell * 0.37) * 4.0) * 0.731);
      float idx;
      if (wild) idx = uGlitchSet.x + floor(hash(cell * 2.7 + gt) * uGlitchSet.y);
      else if (g.a > 0.8) {
        float d = clamp(0.04 + 0.9 * pow(m, 1.3), 0.0, 1.0);
        idx = clamp(uLut[int(d * 63.0)] + floor((h - 0.5) * 7.0), 0.0, uRampN - 1.0);
      }
      else if (g.a > 0.5) idx = uSets.x + floor(h * uSets.y);
      else idx = uSets.z + floor(h * uSets.w);
      ivec2 ap = ivec2(int(idx * uStride + uPad + inC.x), int(uPad + uCell.y - 1.0 - inC.y));
      vec2 gl = texelFetch(uAtlas, ap, 0).rg;
      if (wild) {
        core = wildCol;
      } else {
        // un peu d'irrégularité d'une case à l'autre, comme sur un vrai écran de caractères
        float inten = min(1.0, m * 1.15) * (0.92 + 0.14 * hash(cell * 1.31 + 7.0));
        core = (g.rgb / m) * min(inten, 1.0);
        // Les cases vertes ont un fond légèrement teinté : le W se lit comme une surface verte pleine
        float sat = (m - min(g.r, min(g.g, g.b))) / m;
        if (g.a > 0.8) fill = core * smoothstep(0.45, 0.7, sat) * 0.24;
      }
      a = gl.r;
      halo = core * gl.g * 0.32;
    }
  }
  vec2 uv = (px + 0.5) / (uGrid * uCell);
  vec3 glow = (texture(uGlowA, uv).rgb * 0.36 + texture(uGlowB, uv).rgb * 0.5) * uBloom;
  vec3 under = glow + halo + fill;
  vec3 rgb = min(core * a + under * (1.0 - a), vec3(1.0));
  o = vec4(rgb, a * 0.94) * uFade;
}`;

// ---------------------------------------------------------------------------------------------
// Moteur
// ---------------------------------------------------------------------------------------------

type Target = { tex: WebGLTexture; fbo: WebGLFramebuffer };

export interface IntroCallbacks {
  /** Appelé à chaque image avec le temps de l'intro (pour le mot « rewind » et le fond). */
  onFrame?: (t: number) => void;
  /** L'intro est finie (ou a échoué) : retirer la couche. */
  onDone?: () => void;
}

export class AsciiIntro {
  private gl: WebGL2RenderingContext;
  private progGrains: WebGLProgram;
  private progBlur: WebGLProgram;
  private progFinal: WebGLProgram;
  private vao: WebGLVertexArrayObject;
  private emptyVao: WebGLVertexArrayObject;
  private buffers: WebGLBuffer[] = [];
  private count: number;
  private atlas: WebGLTexture | null = null;
  private atlasInfo: ReturnType<typeof buildAtlas> | null = null;
  private grid: Target | null = null;
  private tmp: Target | null = null;
  private glowA: Target | null = null;
  private glowB: Target | null = null;
  private depth: WebGLRenderbuffer | null = null;
  private cols = 0;
  private rows = 0;
  private cellW = 0;
  private cellH = 0;
  private dpr = 1;
  private raf = 0;
  private t0 = -1;
  private ended = false;
  private loc = new Map<string, WebGLUniformLocation | null>();
  /** Où la poussière va à la fin : le logo de la barre du haut (pixels CSS, mis à jour par la page). */
  dest = { x: 24, y: 21, w: 20 };

  private constructor(
    private canvas: HTMLCanvasElement,
    gl: WebGL2RenderingContext,
    private cb: IntroCallbacks,
  ) {
    this.gl = gl;
    this.progGrains = this.program(VS_GRAINS, FS_GRAINS);
    this.progBlur = this.program(VS_FULL, FS_BLUR);
    this.progFinal = this.program(VS_FULL, FS_FINAL);

    const geo = buildGeometry();
    this.count = geo.length / 11;
    this.vao = gl.createVertexArray()!;
    gl.bindVertexArray(this.vao);
    const buf = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, geo, gl.STATIC_DRAW);
    const stride = 11 * 4;
    const attr = (loc: number, size: number, offset: number) => {
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, stride, offset * 4);
      gl.vertexAttribDivisor(loc, 1);
    };
    attr(0, 3, 0);
    attr(1, 3, 3);
    attr(2, 1, 6);
    attr(3, 4, 7);
    const ends = gl.createBuffer()!;
    gl.bindBuffer(gl.ARRAY_BUFFER, ends);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(4);
    gl.vertexAttribPointer(4, 1, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(4, 0);
    this.buffers.push(buf, ends);
    this.emptyVao = gl.createVertexArray()!;
    gl.bindVertexArray(null);
    this.resize();
  }

  /** Crée le moteur, ou renvoie null si la carte graphique ne le permet pas. */
  static create(canvas: HTMLCanvasElement, cb: IntroCallbacks, opts: { capture?: boolean } = {}): AsciiIntro | null {
    const gl = canvas.getContext("webgl2", {
      alpha: true,
      premultipliedAlpha: true,
      antialias: false,
      depth: false,
      stencil: false,
      preserveDrawingBuffer: !!opts.capture,
    });
    if (!gl) return null;
    const intro = new AsciiIntro(canvas, gl, cb);
    canvas.addEventListener("webglcontextlost", (e) => {
      e.preventDefault();
      intro.finish();
    });
    return intro;
  }

  /** Recalcule la grille et l'atlas pour la taille actuelle de la fenêtre. */
  resize() {
    const gl = this.gl;
    this.dpr = window.devicePixelRatio || 1;
    const W = Math.max(2, Math.round(this.canvas.clientWidth * this.dpr));
    const H = Math.max(2, Math.round(this.canvas.clientHeight * this.dpr));
    this.canvas.width = W;
    this.canvas.height = H;
    const cellH = clamp(Math.round(H / ROWS), 10, 15);
    const cellW = Math.max(4, Math.round(cellH * 0.556));
    const cols = Math.ceil(W / cellW), rows = Math.ceil(H / cellH);
    if (cellH !== this.cellH || cellW !== this.cellW || !this.atlas) {
      this.atlasInfo = buildAtlas(cellW, cellH);
      if (this.atlas) gl.deleteTexture(this.atlas);
      this.atlas = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, this.atlas);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, this.atlasInfo.width, this.atlasInfo.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, this.atlasInfo.px);
      this.texParams(gl.NEAREST);
    }
    this.cellW = cellW;
    this.cellH = cellH;
    if (cols !== this.cols || rows !== this.rows || !this.grid) {
      this.cols = cols;
      this.rows = rows;
      for (const t of [this.grid, this.tmp, this.glowA, this.glowB]) if (t) (gl.deleteTexture(t.tex), gl.deleteFramebuffer(t.fbo));
      if (this.depth) gl.deleteRenderbuffer(this.depth);
      this.depth = gl.createRenderbuffer()!;
      gl.bindRenderbuffer(gl.RENDERBUFFER, this.depth);
      gl.renderbufferStorage(gl.RENDERBUFFER, gl.DEPTH_COMPONENT16, cols, rows);
      this.grid = this.target(cols, rows, true);
      this.tmp = this.target(cols, rows, false);
      this.glowA = this.target(cols, rows, false);
      this.glowB = this.target(cols, rows, false);
    }
  }

  /** Lance l'horloge (quand la fenêtre est réellement visible). */
  start() {
    if (this.ended || this.raf) return;
    // Horloge lissée : si une image arrive en retard (appli qui se charge dessous), l'animation ralentit
    // un instant au lieu de sauter, sans jamais prendre plus de 0,35 s de retard (total < 5 s).
    let last = -1, t = 0;
    const loop = (now: number) => {
      if (this.ended) return;
      if (this.t0 < 0) this.t0 = last = now;
      const real = (now - this.t0) / 1000;
      t = Math.max(t + Math.min((now - last) / 1000, 0.05), real - 0.35);
      last = now;
      try {
        this.render(t);
        this.cb.onFrame?.(t);
      } catch {
        this.finish();
        return;
      }
      if (t >= TL.end) {
        this.finish();
        return;
      }
      this.raf = requestAnimationFrame(loop);
    };
    this.raf = requestAnimationFrame(loop);
  }

  /** Dessine une image précise (captures de vérification). */
  renderAt(t: number) {
    this.render(t);
  }

  /** Arrête l'animation et prévient la page. */
  finish() {
    if (this.ended) return;
    this.ended = true;
    cancelAnimationFrame(this.raf);
    this.cb.onDone?.();
  }

  /** Libère la carte graphique. */
  destroy() {
    this.ended = true;
    cancelAnimationFrame(this.raf);
    const gl = this.gl;
    for (const t of [this.grid, this.tmp, this.glowA, this.glowB]) if (t) (gl.deleteTexture(t.tex), gl.deleteFramebuffer(t.fbo));
    if (this.depth) gl.deleteRenderbuffer(this.depth);
    if (this.atlas) gl.deleteTexture(this.atlas);
    for (const b of this.buffers) gl.deleteBuffer(b);
    gl.deleteVertexArray(this.vao);
    gl.deleteVertexArray(this.emptyVao);
    for (const p of [this.progGrains, this.progBlur, this.progFinal]) gl.deleteProgram(p);
    gl.getExtension("WEBGL_lose_context")?.loseContext();
  }

  // --- Rendu d'une image --------------------------------------------------------------------------

  private render(t: number) {
    const gl = this.gl;
    const W = this.canvas.width, H = this.canvas.height;
    const lay = introLayout(W / this.dpr, H / this.dpr);
    const scale = (lay.iconPx * this.dpr) / 1.938; // px (écran) par unité du modèle
    const center = [(lay.cx * this.dpr) / this.cellW, (H - lay.cy * this.dpr) / this.cellH];
    const half = [(lay.iconPx * this.dpr) / 2 / this.cellW, (lay.iconPx * this.dpr) / 2 / this.cellH];
    // Bug : de temps en temps, l'icône entière saute de quelques cases
    const glitch = glitchAt(t);
    const gk = Math.floor(t * 26);
    if (glitch > 0.3 && hash1(gk * 3.7) < 0.22) center[0] += Math.round((hash1(gk * 7.1) - 0.5) * 6 * glitch);

    // Rotation : rembobinée avec les grains (rapide à la fin), petit rebond, puis légère dérive au vent
    const g = clamp((t - TL.rewind) / (TL.assembled - TL.rewind), 0, 1);
    const e = explode(t < TL.rewind ? 1 : 1 - g);
    let yaw = -1.15 * e, pitch = 0.42 * e;
    const roll = 0.2 * e;
    const s = t - TL.assembled;
    if (s > 0) {
      const k = Math.exp(-s * 4.2);
      yaw += 0.1 * k * Math.sin(s * 9);
      pitch -= 0.05 * k * Math.sin(s * 8);
    }
    // Une fois posée, l'icône tourne doucement sur elle-même en 3D (gauche ↔ droite) sans quitter sa place
    const sway = smooth(TL.assembled + 0.1, TL.assembled + 0.7, t);
    yaw += 0.28 * Math.sin(((t - TL.assembled - 0.1) / 2.6) * Math.PI * 2) * sway;
    pitch += 0.06 * Math.sin(((t - TL.assembled) / 3.1) * Math.PI * 2 + 0.8) * sway;
    yaw += 0.12 * smooth(TL.dissolve, TL.end, t);
    const rot = rotation(yaw, pitch, roll);
    const sw = clamp((t - TL.sweep) / 0.8, 0, 1);
    const sweep = t < TL.sweep || sw >= 1 ? -9 : -1.7 + 3.6 * smooth(0, 1, sw);
    const flash = Math.exp(-(((t - TL.assembled) / 0.07) ** 2));
    const track = smooth(TL.assembled - 0.9, TL.assembled - 0.5, t) * (1 - smooth(TL.assembled - 0.15, TL.assembled + 0.05, t));
    const fade = 1 - smooth(TL.end - 0.3, TL.end, t);
    const trails = (t > TL.rewind - 0.05 && t < TL.assembled + 0.08) || t > TL.dissolve;

    // 1. Grains → grille de caractères
    gl.bindFramebuffer(gl.FRAMEBUFFER, this.grid!.fbo);
    gl.viewport(0, 0, this.cols, this.rows);
    gl.clearColor(0, 0, 0, 0);
    gl.clearDepth(1);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    gl.enable(gl.DEPTH_TEST);
    gl.depthFunc(gl.LESS);
    gl.disable(gl.BLEND);
    const p = this.progGrains;
    gl.useProgram(p);
    gl.uniform1f(this.u(p, "uT"), t);
    gl.uniform1f(this.u(p, "uLag"), t < TL.dissolve ? 0.06 : 0.04);
    gl.uniformMatrix3fv(this.u(p, "uRot"), false, rot);
    gl.uniform2f(this.u(p, "uCenter"), center[0], center[1]);
    gl.uniform2f(this.u(p, "uCell"), this.cellW, this.cellH);
    gl.uniform2f(this.u(p, "uGrid"), this.cols, this.rows);
    gl.uniform1f(this.u(p, "uScale"), scale);
    gl.uniform3f(this.u(p, "uLight"), -0.4815, 0.642, 0.5965);
    gl.uniform1f(this.u(p, "uSweep"), sweep);
    gl.uniform1f(this.u(p, "uFlash"), flash);
    gl.uniform1f(this.u(p, "uRewind"), TL.rewind);
    gl.uniform1f(this.u(p, "uDissolve"), TL.dissolve);
    // Logo de la barre en unités du modèle, corrigé de la perspective (cible posée à z = -0,3)
    const persp = 6 / 6.3;
    gl.uniform2f(this.u(p, "uTarget"), ((this.dest.x - lay.cx) * this.dpr) / scale / persp, ((lay.cy - this.dest.y) * this.dpr) / scale / persp);
    // Le W de l'icône fait 1,44 unité de large (tracé posé à 0,882 dans la plaque de 1,938) ; le petit logo
    // fait dest.w pixels de large : chaque grain vise sa place dans ce petit W.
    gl.uniform1f(this.u(p, "uTs"), (this.dest.w * this.dpr) / scale / persp / 1.44);
    gl.uniform1f(this.u(p, "uLand"), TL.land);
    gl.bindVertexArray(this.vao);
    gl.uniform1f(this.u(p, "uLine"), 0);
    gl.drawArraysInstanced(gl.POINTS, 0, 1, this.count);
    if (trails) {
      gl.uniform1f(this.u(p, "uLine"), 1);
      gl.drawArraysInstanced(gl.LINES, 0, 2, this.count);
    }
    gl.disable(gl.DEPTH_TEST);

    // 2. Halos : flou serré puis large de la grille
    gl.bindVertexArray(this.emptyVao);
    const sa = 6 * this.dpr, sb = 20 * this.dpr; // écart type (px)
    this.blur(this.grid!, this.tmp!, sa / this.cellW / 2 / this.cols, 0);
    this.blur(this.tmp!, this.glowA!, 0, sa / this.cellH / 2 / this.rows);
    this.blur(this.glowA!, this.tmp!, sb / this.cellW / 2 / this.cols, 0);
    this.blur(this.tmp!, this.glowB!, 0, sb / this.cellH / 2 / this.rows);

    // 3. Caractères + halos à l'écran
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, W, H);
    const f = this.progFinal;
    gl.useProgram(f);
    this.bindTex(0, this.grid!.tex, f, "uGridTex");
    this.bindTex(1, this.glowA!.tex, f, "uGlowA");
    this.bindTex(2, this.glowB!.tex, f, "uGlowB");
    this.bindTex(3, this.atlas!, f, "uAtlas");
    const ai = this.atlasInfo!;
    gl.uniform2f(this.u(f, "uCell"), this.cellW, this.cellH);
    gl.uniform2f(this.u(f, "uGrid"), this.cols, this.rows);
    gl.uniform1f(this.u(f, "uStride"), ai.stride);
    gl.uniform1f(this.u(f, "uPad"), PAD);
    gl.uniform1f(this.u(f, "uRampN"), ai.rampN);
    gl.uniform1fv(this.u(f, "uLut"), ai.lut);
    gl.uniform4f(this.u(f, "uSets"), ...ai.sets);
    gl.uniform1f(this.u(f, "uT"), t);
    gl.uniform1f(this.u(f, "uTrack"), track);
    gl.uniform1f(this.u(f, "uBloom"), 1 + flash * 0.9);
    gl.uniform1f(this.u(f, "uFade"), fade);
    gl.uniform1f(this.u(f, "uGlitch"), glitch);
    gl.uniform4f(this.u(f, "uIcon"), center[0], center[1], half[0], half[1]);
    gl.uniform2f(this.u(f, "uGlitchSet"), ...ai.glitchSet);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindVertexArray(null);
  }

  private blur(src: Target, dst: Target, sx: number, sy: number) {
    const gl = this.gl;
    gl.bindFramebuffer(gl.FRAMEBUFFER, dst.fbo);
    gl.viewport(0, 0, this.cols, this.rows);
    gl.useProgram(this.progBlur);
    this.bindTex(0, src.tex, this.progBlur, "uSrc");
    gl.uniform2f(this.u(this.progBlur, "uStep"), sx, sy);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  // --- Outils WebGL ---------------------------------------------------------------------------------

  private program(vs: string, fs: string) {
    const gl = this.gl;
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) || "shader");
      return s;
    };
    const p = gl.createProgram()!;
    const a = sh(gl.VERTEX_SHADER, vs), b = sh(gl.FRAGMENT_SHADER, fs);
    gl.attachShader(p, a);
    gl.attachShader(p, b);
    gl.linkProgram(p);
    gl.deleteShader(a);
    gl.deleteShader(b);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) || "programme");
    return p;
  }

  private u(p: WebGLProgram, name: string) {
    const key = `${this.progKey(p)}:${name}`;
    if (!this.loc.has(key)) this.loc.set(key, this.gl.getUniformLocation(p, name));
    return this.loc.get(key)!;
  }
  private progKey(p: WebGLProgram) {
    return p === this.progGrains ? "g" : p === this.progBlur ? "b" : "f";
  }

  private bindTex(unit: number, tex: WebGLTexture, p: WebGLProgram, name: string) {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.uniform1i(this.u(p, name), unit);
  }

  private texParams(filter: number) {
    const gl = this.gl;
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }

  private target(w: number, h: number, withDepth: boolean): Target {
    const gl = this.gl;
    const tex = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    this.texParams(gl.LINEAR);
    const fbo = gl.createFramebuffer()!;
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    if (withDepth) gl.framebufferRenderbuffer(gl.FRAMEBUFFER, gl.DEPTH_ATTACHMENT, gl.RENDERBUFFER, this.depth);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    return { tex, fbo };
  }
}

/** Rotation lacet (Y) · tangage (X) · roulis (Z), en colonnes pour WebGL. */
function rotation(yaw: number, pitch: number, roll: number) {
  const cy = Math.cos(yaw), sy = Math.sin(yaw);
  const cx = Math.cos(pitch), sx = Math.sin(pitch);
  const cz = Math.cos(roll), sz = Math.sin(roll);
  // Ry · Rx
  const a = [
    [cy, sy * sx, sy * cx],
    [0, cx, -sx],
    [-sy, cy * sx, cy * cx],
  ];
  // (Ry · Rx) · Rz
  const rz = [
    [cz, -sz, 0],
    [sz, cz, 0],
    [0, 0, 1],
  ];
  const m = [0, 1, 2].map((r) => [0, 1, 2].map((c) => a[r][0] * rz[0][c] + a[r][1] * rz[1][c] + a[r][2] * rz[2][c]));
  return new Float32Array([m[0][0], m[1][0], m[2][0], m[0][1], m[1][1], m[2][1], m[0][2], m[1][2], m[2][2]]);
}
