// Barre de défilement « verre » : une pastille fine aux extrémités entièrement arrondies, qui flotte
// à droite du contenu (elle ne prend pas de place), s'élargit au survol et se glisse comme un curseur.
//
// Utilisation : le conteneur qui défile (`overflow-y: auto`) reçoit `use:glassScroll` et son PARENT doit
// avoir `position: relative`. Les barres natives sont masquées dans toute l'appli (voir styles.css).
// Le fondu en haut et en bas du texte (variables --fade-top / --fade-bottom) indique qu'il y a une suite.

const GAP = 16; // distance entre la pastille et les bords haut / bas (suit l'arrondi du cadre)
const MIN_THUMB = 40;

export function glassScroll(node: HTMLElement) {
  const wrap = node.parentElement;
  if (!wrap) return;
  const track = document.createElement("div");
  track.className = "gtrack";
  const thumb = document.createElement("div");
  thumb.className = "gthumb";
  track.appendChild(thumb);
  wrap.appendChild(track);

  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let dragging = false;
  let startY = 0;
  let startScroll = 0;

  const geometry = () => {
    const { scrollTop, scrollHeight, clientHeight } = node;
    const room = Math.max(0, clientHeight - GAP * 2);
    const size = Math.min(room, Math.max(MIN_THUMB, room * (clientHeight / scrollHeight)));
    const max = Math.max(1, scrollHeight - clientHeight);
    return { scrollTop, scrollHeight, clientHeight, room, size, max, top: GAP + (room - size) * (scrollTop / max) };
  };

  const update = () => {
    const g = geometry();
    const scrollable = g.scrollHeight > g.clientHeight + 1;
    wrap!.classList.toggle("can-scroll", scrollable);
    node.style.setProperty("--fade-top", `${Math.min(g.scrollTop, 22)}px`);
    node.style.setProperty("--fade-bottom", `${scrollable ? Math.min(g.max - g.scrollTop, 22) : 0}px`);
    thumb.style.height = `${g.size}px`;
    thumb.style.transform = `translateY(${g.top}px)`;
  };

  const wake = () => {
    wrap!.classList.add("scrolling");
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => !dragging && wrap!.classList.remove("scrolling"), 1100);
  };

  const onScroll = () => {
    update();
    wake();
  };
  const onDown = (e: PointerEvent) => {
    e.preventDefault();
    const g = geometry();
    const rect = thumb.getBoundingClientRect();
    if (e.clientY >= rect.top && e.clientY <= rect.bottom) {
      dragging = true;
      startY = e.clientY;
      startScroll = node.scrollTop;
      track.setPointerCapture(e.pointerId);
      wrap!.classList.add("dragging");
    } else {
      // clic dans la gouttière : une page vers le haut ou vers le bas
      node.scrollBy({ top: (e.clientY < rect.top ? -1 : 1) * g.clientHeight * 0.85, behavior: "smooth" });
    }
  };
  const onMove = (e: PointerEvent) => {
    if (!dragging) return;
    const g = geometry();
    const travel = Math.max(1, g.room - g.size);
    node.scrollTop = startScroll + ((e.clientY - startY) * g.max) / travel;
  };
  const onUp = (e: PointerEvent) => {
    if (!dragging) return;
    dragging = false;
    try {
      track.releasePointerCapture(e.pointerId);
    } catch {}
    wrap!.classList.remove("dragging");
    wake();
  };

  node.addEventListener("scroll", onScroll, { passive: true });
  track.addEventListener("pointerdown", onDown);
  track.addEventListener("pointermove", onMove);
  track.addEventListener("pointerup", onUp);
  track.addEventListener("pointercancel", onUp);
  wrap.addEventListener("pointerenter", wake);
  const ro = new ResizeObserver(update);
  ro.observe(node);
  for (const child of Array.from(node.children)) ro.observe(child);
  update();

  return {
    destroy() {
      clearTimeout(hideTimer);
      ro.disconnect();
      node.removeEventListener("scroll", onScroll);
      wrap.removeEventListener("pointerenter", wake);
      track.remove();
    },
  };
}
