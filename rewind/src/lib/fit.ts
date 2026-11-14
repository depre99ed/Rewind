// Action Svelte : une image ou une vidéo verticale (9:16, TikTok) ne doit pas être rognée dans un cadre 16:9.
// Elle est affichée en entier (`contain`) sur un fond flou fait de la même image (voir `.portrait` dans styles.css).
export function fit(node: HTMLImageElement | HTMLVideoElement) {
  const video = node instanceof HTMLVideoElement;
  const apply = () => {
    const w = video ? node.videoWidth : node.naturalWidth;
    const h = video ? node.videoHeight : node.naturalHeight;
    if (!w || !h) return;
    const portrait = h > w * 1.05;
    node.classList.toggle("portrait", portrait);
    const parent = node.parentElement;
    if (parent) {
      parent.classList.toggle("has-portrait", portrait);
      // Le fond flou vient de la miniature (une image) ; une vidéo seule garde le noir.
      if (portrait && !video && node.currentSrc) parent.style.setProperty("--pbg", `url("${node.currentSrc}")`);
    }
  };
  const event = video ? "loadedmetadata" : "load";
  node.addEventListener(event, apply);
  apply(); // image déjà chargée (cache)
  return { destroy: () => node.removeEventListener(event, apply) };
}
