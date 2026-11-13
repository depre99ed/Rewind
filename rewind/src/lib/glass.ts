// Verre liquide : ce qui passe derrière l'élément est vraiment déformé (réfraction sur les bords),
// comme une lentille de verre aux bords arrondis.
//
// Technique (Chromium / WebView2) : une carte de déplacement est calculée pour la forme exacte de
// l'élément (rectangle aux coins arrondis). Le centre est neutre ; dans la bande du bord, chaque
// pixel va chercher l'image un peu plus vers l'intérieur, ce qui grossit et courbe le contenu comme
// le bord épais d'une lentille. La carte est donnée à un filtre SVG (feImage + feDisplacementMap),
// utilisé dans `backdrop-filter`. Une légère dispersion (rouge / bleu décalés) imite la lumière
// qui traverse un verre courbe. Le liseré de lumière est fait en CSS (classe `.glass`).
//
// Le filtre n'est branché qu'une fois créé dans la page (sinon Chromium affiche l'élément
// transparent un instant). La carte n'est recalculée que si la taille change.

type Opts = {
  /** Largeur de la bande qui réfracte, en px */
  bezel?: number;
  /** Force du déplacement, en px */
  strength?: number;
  /** Flou du verre en px (0 = verre clair) */
  blur?: number;
  /** Saturation du fond vu à travers le verre */
  saturate?: number;
  /** Dispersion des couleurs sur les bords (0 à 1) */
  chroma?: number;
};

const NS = "http://www.w3.org/2000/svg";
let host: SVGSVGElement | null = null;
let seq = 0;

function svgHost(): SVGSVGElement {
  if (host && host.isConnected) return host;
  host = document.createElementNS(NS, "svg");
  host.setAttribute("aria-hidden", "true");
  host.setAttribute("width", "0");
  host.setAttribute("height", "0");
  host.style.position = "absolute";
  host.style.pointerEvents = "none";
  host.appendChild(document.createElementNS(NS, "defs"));
  document.body.appendChild(host);
  return host;
}

/** Carte de déplacement d'un rectangle aux coins arrondis (canaux R = x, G = y, 128 = neutre). */
function lensMap(w: number, h: number, r: number, bezel: number): string {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const ctx = c.getContext("2d")!;
  const img = ctx.createImageData(w, h);
  const d = img.data;
  const hw = w / 2;
  const hh = h / 2;
  r = Math.min(r, hw, hh);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const px = x + 0.5 - hw;
      const py = y + 0.5 - hh;
      const qx = Math.abs(px) - (hw - r);
      const qy = Math.abs(py) - (hh - r);
      // Distance au bord (positive à l'intérieur) et direction vers l'extérieur
      let dist: number, nx: number, ny: number;
      if (qx > 0 && qy > 0) {
        const len = Math.hypot(qx, qy) || 1;
        dist = r - len;
        nx = qx / len;
        ny = qy / len;
      } else if (qx > qy) {
        dist = r - qx;
        nx = 1;
        ny = 0;
      } else {
        dist = r - qy;
        nx = 0;
        ny = 1;
      }
      nx *= Math.sign(px) || 1;
      ny *= Math.sign(py) || 1;
      let vx = 0;
      let vy = 0;
      if (dist >= 0 && dist < bezel) {
        // Profil de lentille convexe : nul au fond de la bande, maximal au bord
        const t = 1 - dist / bezel;
        const m = t * t * (3 - 2 * t);
        vx = -nx * m;
        vy = -ny * m;
      }
      const i = (y * w + x) * 4;
      d[i] = 128 + vx * 127;
      d[i + 1] = 128 + vy * 127;
      d[i + 2] = 128;
      d[i + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
  return c.toDataURL("image/png");
}

function buildFilter(id: string, w: number, h: number, map: string, strength: number, chroma: number): SVGFilterElement {
  const f = document.createElementNS(NS, "filter");
  f.setAttribute("id", id);
  f.setAttribute("x", "0");
  f.setAttribute("y", "0");
  f.setAttribute("width", String(w));
  f.setAttribute("height", String(h));
  f.setAttribute("filterUnits", "userSpaceOnUse");
  f.setAttribute("color-interpolation-filters", "sRGB");
  const s = strength;
  const dR = s * (1 + chroma * 0.18);
  const dB = s * (1 - chroma * 0.18);
  f.innerHTML = `
    <feImage href="${map}" x="0" y="0" width="${w}" height="${h}" preserveAspectRatio="none" result="map"/>
    <feDisplacementMap in="SourceGraphic" in2="map" scale="${dR}" xChannelSelector="R" yChannelSelector="G" result="dr"/>
    <feDisplacementMap in="SourceGraphic" in2="map" scale="${s}" xChannelSelector="R" yChannelSelector="G" result="dg"/>
    <feDisplacementMap in="SourceGraphic" in2="map" scale="${dB}" xChannelSelector="R" yChannelSelector="G" result="db"/>
    <feColorMatrix in="dr" type="matrix" values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0" result="r"/>
    <feColorMatrix in="dg" type="matrix" values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0" result="g"/>
    <feColorMatrix in="db" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0" result="b"/>
    <feBlend in="r" in2="g" mode="screen" result="rg"/>
    <feBlend in="rg" in2="b" mode="screen"/>`;
  return f;
}

/** Action Svelte : `<div class="glass" use:liquid={{ blur: 12 }}>` */
export function liquid(node: HTMLElement, opts: Opts = {}) {
  let o: Required<Opts> = { bezel: 14, strength: 26, blur: 0, saturate: 1.6, chroma: 0.5, ...opts };
  let filter: SVGFilterElement | null = null;
  let last = "";
  const id = `lg-${++seq}`;

  const apply = () => {
    const w = Math.round(node.offsetWidth);
    const h = Math.round(node.offsetHeight);
    if (w < 4 || h < 4) return;
    const radius = parseFloat(getComputedStyle(node).borderTopLeftRadius) || 0;
    const key = `${w}x${h}r${radius}|${o.bezel}|${o.strength}|${o.chroma}`;
    if (key !== last) {
      last = key;
      const map = lensMap(w, h, radius, Math.min(o.bezel, w / 2, h / 2));
      const next = buildFilter(id, w, h, map, o.strength, o.chroma);
      const defs = svgHost().firstElementChild!;
      if (filter) filter.replaceWith(next);
      else defs.appendChild(next);
      filter = next;
    }
    const extra = `${o.blur ? ` blur(${o.blur}px)` : ""} saturate(${o.saturate})`;
    node.style.setProperty("backdrop-filter", `url(#${id})${extra}`);
  };

  // Rendu correct même sans filtre SVG (premier affichage) : simple verre dépoli
  node.style.setProperty("backdrop-filter", `${o.blur ? `blur(${o.blur}px) ` : ""}saturate(${o.saturate})`);
  const ro = new ResizeObserver(() => apply());
  ro.observe(node);
  requestAnimationFrame(apply);

  return {
    update(n: Opts = {}) {
      o = { ...o, ...n };
      last = "";
      apply();
    },
    destroy() {
      ro.disconnect();
      filter?.remove();
    },
  };
}

/** Reflet qui suit la souris (variables --mx / --my), pour le liseré de lumière du verre. */
export function glint(node: HTMLElement) {
  const move = (e: PointerEvent) => {
    const r = node.getBoundingClientRect();
    node.style.setProperty("--mx", `${e.clientX - r.left}px`);
    node.style.setProperty("--my", `${e.clientY - r.top}px`);
  };
  node.addEventListener("pointermove", move);
  return { destroy: () => node.removeEventListener("pointermove", move) };
}
