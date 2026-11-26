<script lang="ts">
  // Animation d'ouverture (2026-11-26) : l'icône Rewind en caractères lumineux, puis « rewind » tapé
  // dessous. Jouée à la première ouverture de la fenêtre après le lancement (voir +page.svelte), ou
  // depuis Réglages › Apparence. Un clic ou une touche la passe. Elle ne bloque jamais l'appli : en cas
  // de souci (pas de WebGL 2…), elle se retire aussitôt, et un minuteur la retire quoi qu'il arrive.
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { AsciiIntro, TL, TYPE_TIMES, glitchAt, introLayout } from "$lib/intro/ascii-intro";
  import { api, logFront } from "$lib/api";

  let { ondone }: { ondone: () => void } = $props();

  const WORD = "rewind";
  const NOISE = "#%&@$*+=-:.";
  const GLITCH = "#%&@$*/\\|<>[]{}!?=+~^";
  const mock = typeof window !== "undefined" && !!(window as any).__REWIND_MOCK__;

  let canvas: HTMLCanvasElement;
  let w = $state(typeof window === "undefined" ? 1320 : innerWidth);
  let h = $state(typeof window === "undefined" ? 840 : innerHeight);
  const lay = $derived(introLayout(w, h));
  let backdrop = $state(1);
  let typed = $state(0);
  let letters = $state(WORD.split("").map((ch) => ({ ch, lime: false })));
  let cursor = $state(false);
  let shake = $state(0);
  let leaving = $state(false);
  let sound = true;
  let ticked = 0;
  let engine: AsciiIntro | null = null;
  let finished = false;
  let watchdog: ReturnType<typeof setTimeout> | undefined;

  const clamp = (v: number, a: number, b: number) => Math.min(b, Math.max(a, v));

  function finish() {
    if (finished) return;
    finished = true;
    clearTimeout(watchdog);
    engine?.destroy();
    engine = null;
    ondone();
  }

  /** Passer : fondu rapide de toute la couche, l'appli est déjà prête dessous. */
  function skip() {
    if (finished || leaving) return;
    leaving = true;
    engine?.finish();
    setTimeout(finish, 220);
  }

  const rnd = (n: number) => {
    const x = Math.sin(n * 91.7 + 17.3) * 43758.5453;
    return x - Math.floor(x);
  };

  // Mot « rewind » et fond noir, pilotés par l'horloge de l'animation
  function onFrame(t: number) {
    const r = clamp((t - TL.reveal) / TL.revealDur, 0, 1);
    backdrop = 1 - r * r * (3 - 2 * r);
    typed = TYPE_TIMES.filter((x) => t >= x).length;
    // Un petit « tick » par lettre (joué par Rust : pas de blocage du son au lancement)
    while (sound && ticked < typed) api.introKey(ticked++).catch(() => {});
    const g = glitchAt(t);
    const k = Math.floor(t * 26);
    letters = WORD.split("").map((ch, i) => {
      // Au départ en poussière, chaque lettre se brouille puis s'efface, de gauche à droite (comme le vent)
      const ts = TL.dissolve + 0.02 + i * 0.045;
      if (t >= ts + 0.16) return { ch: " ", lime: false };
      if (t >= ts) return { ch: NOISE[(Math.floor(t * 40) + i * 3) % NOISE.length], lime: false };
      // Bug : les lettres sont remplacées par des caractères au hasard, en blanc ou en vert
      if (g > 0 && rnd(k * 7 + i) < 0.55 * g) return { ch: GLITCH[Math.floor(rnd(k * 13 + i * 5) * GLITCH.length)], lime: rnd(k * 3 + i) < 0.4 };
      return { ch, lime: false };
    });
    // Le mot entier saute de côté par moments pendant le bug
    shake = g > 0.2 && rnd(k * 1.9) < 0.35 ? Math.round((rnd(k * 2.3) - 0.5) * 16 * g) : 0;
    const typingEnd = TYPE_TIMES[TYPE_TIMES.length - 1];
    const blinkOn = Math.floor((t - typingEnd) / 0.42) % 2 === 1;
    cursor = t >= TL.typeStart - 0.3 && t < TL.dissolve && (t < typingEnd + 0.1 ? true : g > 0 ? rnd(k * 5.3) < 0.5 : blinkOn);
  }

  /** Attend que la fenêtre soit vraiment à l'écran (Rewind peut démarrer caché dans la zone de notification). */
  async function waitShown(alive: () => boolean) {
    if (mock) return;
    const win = getCurrentWindow();
    const shown = async () => {
      if (document.hidden) return false;
      if (document.hasFocus()) return true;
      try {
        return await win.isVisible();
      } catch {
        return true;
      }
    };
    const since = performance.now();
    while (alive()) {
      if (await shown()) return;
      await new Promise<void>((resolve) => {
        const done = () => {
          clearTimeout(timer);
          document.removeEventListener("visibilitychange", done);
          window.removeEventListener("focus", done);
          resolve();
        };
        const wait = document.hidden ? 60_000 : performance.now() - since < 15_000 ? 300 : 1000;
        const timer = setTimeout(done, wait);
        document.addEventListener("visibilitychange", done);
        window.addEventListener("focus", done);
      });
    }
  }

  onMount(() => {
    let alive = true;
    const freeze = mock && new URLSearchParams(location.search).has("introfreeze");
    (async () => {
      try {
        await document.fonts.load('600 16px "Geist Mono Variable"').catch(() => {});
        if (!alive) return;
        engine = AsciiIntro.create(canvas, { onFrame, onDone: () => !leaving && finish() }, { capture: freeze });
        if (!engine) {
          logFront("warn", "animation d'ouverture passée : WebGL 2 indisponible");
          return finish();
        }
        if (freeze) {
          sound = false;
          // Captures de vérification (aperçu navigateur seulement) : une image précise à l'instant t
          (window as any).__introEnd = TL.end;
          (window as any).__introSeek = async (t: number) => {
            engine?.renderAt(t);
            onFrame(t);
            await tick();
            await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
          };
          return;
        }
        await waitShown(() => alive);
        // Laisse d'abord l'appli se dessiner dessous (fond noir), pour que l'intro parte sans à-coup
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        if (!alive || !engine) return;
        watchdog = setTimeout(finish, (TL.end + 1.5) * 1000);
        engine.start();
      } catch (e) {
        logFront("warn", `animation d'ouverture : ${e}`);
        finish();
      }
    })();
    const onResize = () => {
      w = innerWidth;
      h = innerHeight;
      engine?.resize();
    };
    const onKey = () => skip();
    window.addEventListener("resize", onResize);
    window.addEventListener("keydown", onKey);
    return () => {
      alive = false;
      clearTimeout(watchdog);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("keydown", onKey);
      engine?.destroy();
      engine = null;
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="intro" class:leaving style:pointer-events={backdrop < 0.6 ? "none" : "auto"} onpointerdown={skip} aria-hidden="true">
  <div class="black" style:opacity={backdrop}></div>
  <canvas bind:this={canvas}></canvas>
  <div class="line" style:top="{lay.wordY}px" style:font-size="{lay.wordSize}px">
    <span class="word" style:transform="translateX({shake}px)">
      {#each letters as l, i}<span class="l" class:on={i < typed} class:lime={l.lime}>{l.ch}</span>{/each}
      <i class="cursor" class:on={cursor} style:left="calc({typed} * (1ch + 0.32em))"></i>
    </span>
  </div>
</div>

<style>
  .intro { position: fixed; inset: 0; z-index: 9000; overflow: hidden; }
  .intro.leaving { opacity: 0; transition: opacity 220ms var(--ease); }
  .black { position: absolute; inset: 0; background: #000; }
  canvas { position: absolute; inset: 0; width: 100%; height: 100%; display: block; }
  /* « rewind » tapé comme dans un terminal : Geist Mono, blanc cassé de la marque, curseur vert */
  .line { position: absolute; left: 0; right: 0; display: flex; justify-content: center; transform: translateY(-50%); pointer-events: none; }
  .word { position: relative; font-family: var(--mono); font-weight: 500; letter-spacing: 0.32em; margin-right: -0.32em; color: #eef0f4; text-shadow: 0 0 14px rgba(238, 240, 244, 0.45); white-space: pre; line-height: 1; }
  .l { visibility: hidden; }
  .l.on { visibility: visible; }
  .l.lime { color: #c8f031; text-shadow: 0 0 12px rgba(200, 240, 49, 0.6); }
  .cursor { position: absolute; top: 0.02em; width: 0.56em; height: 0.98em; background: #c8f031; box-shadow: 0 0 12px rgba(200, 240, 49, 0.7); opacity: 0; }
  .cursor.on { opacity: 1; }
</style>
