<script lang="ts">
  import "$lib/styles.css";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { fileUrl, gameColor } from "$lib/api";
  import Logo from "$lib/components/Logo.svelte";
  import { EN } from "$lib/i18n-en";

  type Toast = { state: string; title: string; subtitle: string; game: string; thumb: string; badges: string[]; seconds: number; keys?: [string, string][]; lang?: string };
  let t = $state<Toast | null>(null);
  let visible = $state(false);
  let key = $state(0);

  onMount(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const params = new URLSearchParams(location.search);
    if (params.get("demo") === "welcome") {
      t = {
        state: "welcome", title: "Counter-Strike 2", subtitle: "Rewind enregistre · tes raccourcis", game: "Counter-Strike 2", thumb: "", badges: [], seconds: 0,
        keys: [["Alt+F10", "Clip 30 s"], ["Alt+Shift+F10", "Clip 2 min"], ["Alt+F11", "Clip sur mesure"], ["Alt+F7", "Session complète"]],
      };
      visible = true; key++;
      return;
    }
    if (params.get("demo")) {
      // Aperçu dans un navigateur : rejoue les états
      const demo: Toast = { state: "saving", title: "30 dernières secondes", subtitle: "Counter-Strike 2", game: "Counter-Strike 2", thumb: "", badges: [], seconds: 30 };
      t = demo; visible = true; key++;
      setTimeout(() => { t = { state: "saved", title: "CS2 ACE · 2× One Tap", subtitle: "Counter-Strike 2 · 34 s", game: "Counter-Strike 2", thumb: "/mock/t1.jpg", badges: ["ACE", "One Tap", params.get("lang") === "en" ? "5 streak" : "Série de 5"], seconds: 34, lang: params.get("lang") ?? undefined }; key++; }, params.get("demo") === "saved" ? 0 : 1600);
      return;
    }
    const un1 = listen<Toast>("toast", (e) => { t = e.payload; visible = true; key++; });
    const un2 = listen("toast-hide", () => (visible = false));
    return () => { un1.then((f) => f()); un2.then((f) => f()); };
  });

  const color = $derived(t?.state === "failed" ? "#ff5d5d" : gameColor(t?.game ?? ""));
  // Fenêtre à part : la langue arrive avec chaque notification (réglage de l'appli)
  const L = (fr: string) => (t?.lang === "en" ? (EN[fr] ?? fr) : fr);
  const label = $derived(
    L(t?.state === "saving" ? "Enregistrement…" : t?.state === "failed" ? "Échec" : t?.state === "shot" ? "Capture enregistrée" : t?.state === "marked" ? "Rewind" : "Clip enregistré"),
  );
</script>

{#if t && t.state === "welcome"}
  <!-- Bandeau d'accueil : jeu reconnu + raccourcis (6 s) -->
  <div class="toast welcome" class:in={visible} style="--c:{color};--life:5.8s">
    <div class="glow"></div>
    <div class="whead">
      <div class="wlogo"><Logo size={26} /></div>
      <div class="wtext">
        <div class="kicker"><i class="dot rec"></i>{L("Rewind est prêt")}</div>
        {#key key}<div class="title pop">{t.title}</div>{/key}
        <div class="sub"><span class="muted">{t.subtitle}</span></div>
      </div>
    </div>
    <div class="keys">
      {#each t.keys ?? [] as [k, what], i}
        <div class="krow" style="animation-delay:{120 + i * 70}ms">
          <span class="kchip">{#each k.split("+") as part, j}{#if j > 0}<b>+</b>{/if}<kbd>{part}</kbd>{/each}</span>
          <span class="kwhat">{what}</span>
        </div>
      {/each}
    </div>
    {#key key}<div class="timer"><i></i></div>{/key}
  </div>
{:else if t}
  <div class="toast" class:in={visible} class:saving={t.state === "saving"} style="--c:{color}">
    <div class="glow"></div>
    <div class="thumb">
      {#if t.thumb && t.state !== "saving"}
        {#key key}<img src={fileUrl(t.thumb)} alt="" class="pop" />{/key}
      {:else}
        <div class="rew" class:spin={t.state === "saving"}>
          <Logo size={34} />
        </div>
      {/if}
    </div>
    <div class="body">
      <div class="kicker">
        {#if t.state === "saved" || t.state === "shot"}
          <svg class="check" viewBox="0 0 24 24" width="13" height="13"><path d="M5 12.5l4.2 4.2L19 7" fill="none" stroke="var(--c)" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" /></svg>
        {:else}
          <i class="dot"></i>
        {/if}
        {label}
      </div>
      {#key key}
        <div class="title pop">{t.title}</div>
      {/key}
      <div class="sub">
        {#if t.badges.length}
          {#each t.badges as b}<span class="badge">{b}</span>{/each}
        {:else}
          <span class="muted">{t.subtitle}</span>
        {/if}
      </div>
    </div>
    {#key key}
      <div class="timer" class:indeterminate={t.state === "saving"}><i></i></div>
    {/key}
  </div>
{/if}

<style>
  :global(html), :global(body) { background: transparent !important; overflow: hidden; }
  .toast {
    position: fixed; inset: 6px; display: flex; align-items: center; gap: 14px; padding: 12px 16px 12px 12px;
    border-radius: 18px; overflow: hidden; color: #fff;
    background: linear-gradient(135deg, rgba(20, 22, 29, 0.96), rgba(11, 12, 16, 0.96));
    border: 1px solid color-mix(in srgb, var(--c) 45%, rgba(255, 255, 255, 0.08));
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.06);
    opacity: 0; transform: translateX(-28px) scale(0.96);
    transition: opacity 260ms cubic-bezier(0.2, 0.8, 0.2, 1), transform 420ms cubic-bezier(0.16, 1, 0.3, 1);
  }
  .toast.in { opacity: 1; transform: none; }
  .glow { position: absolute; left: -40px; top: -40px; width: 180px; height: 180px; border-radius: 50%; background: radial-gradient(circle, color-mix(in srgb, var(--c) 45%, transparent), transparent 70%); opacity: 0.55; pointer-events: none; }
  .toast::before { content: ""; position: absolute; left: 0; top: 14px; bottom: 14px; width: 3px; border-radius: 0 3px 3px 0; background: var(--c); box-shadow: 0 0 14px var(--c); }
  .thumb { position: relative; width: 122px; height: 69px; flex: none; border-radius: 11px; overflow: hidden; background: rgba(255, 255, 255, 0.05); border: 1px solid rgba(255, 255, 255, 0.08); display: grid; place-items: center; }
  .thumb img { width: 100%; height: 100%; object-fit: cover; }
  .rew.spin :global(svg) { animation: rew 1.1s cubic-bezier(0.6, 0, 0.4, 1) infinite; }
  @keyframes rew { 0% { transform: translateX(6px); opacity: 0.4; } 50% { transform: translateX(-6px); opacity: 1; } 100% { transform: translateX(6px); opacity: 0.4; } }
  .body { position: relative; min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 4px; }
  .kicker { display: flex; align-items: center; gap: 6px; font-size: 10.5px; font-weight: 700; letter-spacing: 0.12em; text-transform: uppercase; color: color-mix(in srgb, var(--c) 80%, #fff); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--c); animation: pulse 1s ease-in-out infinite; }
  .title { font-family: var(--display); font-weight: 800; font-size: 16px; letter-spacing: -0.02em; line-height: 1.15; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sub { display: flex; gap: 5px; font-size: 12px; color: rgba(255, 255, 255, 0.65); white-space: nowrap; overflow: hidden; }
  .badge { height: 19px; padding: 0 7px; border-radius: 6px; display: inline-flex; align-items: center; font-size: 10.5px; font-weight: 800; letter-spacing: 0.04em; text-transform: uppercase; background: color-mix(in srgb, var(--c) 22%, rgba(255, 255, 255, 0.04)); color: color-mix(in srgb, var(--c) 70%, #fff); }
  .timer { position: absolute; left: 0; right: 0; bottom: 0; height: 3px; background: rgba(255, 255, 255, 0.06); }
  .timer i { display: block; height: 100%; background: var(--c); transform-origin: left; animation: drain 3.4s linear both; }
  .timer.indeterminate i { width: 30%; animation: slide 1.2s ease-in-out infinite; }
  @keyframes drain { from { transform: scaleX(1); } to { transform: scaleX(0); } }
  @keyframes slide { from { transform: translateX(-110%); } to { transform: translateX(380%); } }
  .pop { animation: popin 380ms cubic-bezier(0.16, 1, 0.3, 1) both; }
  @keyframes popin { from { opacity: 0; transform: translateY(6px) scale(0.98); } to { opacity: 1; transform: none; } }
  .check { animation: popin 380ms both; }

  /* Bandeau d'accueil */
  .toast.welcome { flex-direction: column; align-items: stretch; gap: 12px; padding: 14px 16px 16px 16px; }
  .whead { position: relative; display: flex; align-items: center; gap: 12px; }
  .wlogo { width: 46px; height: 46px; border-radius: 13px; display: grid; place-items: center; background: rgba(255, 255, 255, 0.05); border: 1px solid rgba(255, 255, 255, 0.08); flex: none; }
  .wtext { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .dot.rec { background: #ff4d4f; box-shadow: 0 0 8px #ff4d4f; }
  .keys { position: relative; display: grid; grid-template-columns: max-content 1fr; gap: 5px 12px; align-items: center; padding-left: 58px; }
  .krow { display: contents; }
  .krow > * { animation: popin 420ms cubic-bezier(0.16, 1, 0.3, 1) both; animation-delay: inherit; }
  .kchip { display: inline-flex; align-items: center; gap: 2px; flex: none; justify-self: start; }
  .kchip b { font-size: 10px; color: rgba(255, 255, 255, 0.4); font-weight: 600; }
  kbd { font-family: var(--mono); font-size: 10.5px; font-weight: 700; padding: 3px 6px; border-radius: 6px; background: rgba(255, 255, 255, 0.09); border: 1px solid rgba(255, 255, 255, 0.14); border-bottom-width: 2px; color: #fff; line-height: 1; }
  .kwhat { font-size: 12px; color: rgba(255, 255, 255, 0.8); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .welcome .timer i { animation-duration: var(--life); }
</style>
