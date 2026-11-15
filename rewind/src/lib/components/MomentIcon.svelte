<script lang="ts">
  // Pictos des moments CS2, dessinés sur une même grille 24×24 (trait 2, bouts ronds) pour qu'ils
  // se reconnaissent d'un coup d'œil : viseur = kill, tête touchée = headshot, une balle = one tap…
  // Les multi-kills sont écrits (2K, 3K, ACE) : plus clair qu'un dessin.
  let { kind, size = 16 }: { kind: string; size?: number } = $props();

  const TEXT: Record<string, string> = { "2k": "2K", "3k": "3K", "4k": "4K", ace: "ACE" };
</script>

{#if TEXT[kind]}
  <span class="mtext" style="font-size:{Math.round(size * 0.62)}px">{TEXT[kind]}</span>
{:else}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if kind === "kill"}
      <circle cx="12" cy="12" r="6.5" />
      <path d="M12 2.5v5M12 16.5v5M2.5 12h5M16.5 12h5" />
      <circle cx="12" cy="12" r="1.2" fill="currentColor" stroke="none" />
    {:else if kind === "headshot"}
      <circle cx="12" cy="8.5" r="5" />
      <path d="M4.5 21c1.1-3.9 4-5.8 7.5-5.8s6.4 1.9 7.5 5.8" />
      <circle cx="13.4" cy="7.4" r="1.7" fill="currentColor" stroke="none" />
    {:else if kind === "onetap"}
      <path d="M12 2.8c-1.9 1.5-2.9 3.7-2.9 6.3V18h5.8V9.1c0-2.6-1-4.8-2.9-6.3z" />
      <path d="M8.5 21.2h7" />
    {:else if kind === "knife"}
      <path d="M20.5 3.5c-4 .6-8.4 4.2-10.7 8.1l2.6 2.6c3.9-2.3 7.5-6.7 8.1-10.7z" />
      <path d="M7.4 11.4l5.2 5.2" />
      <path d="M9.6 15.4l-5.1 5.1" stroke-width="3" />
    {:else if kind === "zeus"}
      <path d="M13.2 2.5 5 13.6h6.2L10.4 21.5 19 10.2h-6.4z" fill="currentColor" fill-opacity="0.25" />
    {:else if kind === "collateral"}
      <circle cx="8" cy="10.5" r="3.6" />
      <circle cx="16" cy="10.5" r="3.6" />
      <path d="M1.8 10.5h20.4" />
      <path d="M4 20c.8-2.3 2.2-3.4 4-3.4s3.2 1.1 4 3.4M12 20c.8-2.3 2.2-3.4 4-3.4s3.2 1.1 4 3.4" />
    {:else if kind === "streak"}
      <path d="M12 21.5c3.9 0 6.8-2.6 6.8-6.5 0-3.2-2.1-5.1-3.5-7-.6 1.5-1.5 2.5-2.8 3.1.4-3.3-1-6.1-3.4-8 .2 2.7-1.3 4.5-2.9 6.2C4.8 11.4 5.2 12.6 5.2 15c0 3.9 2.9 6.5 6.8 6.5z" />
    {:else if kind === "death"}
      <path d="M12 3a7.3 7.3 0 0 0-7.3 7.3c0 2.5 1.2 4.3 2.9 5.4v3.1c0 .6.4 1 1 1h6.8c.6 0 1-.4 1-1v-3.1c1.7-1.1 2.9-2.9 2.9-5.4A7.3 7.3 0 0 0 12 3z" />
      <circle cx="9.2" cy="11" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="14.8" cy="11" r="1.5" fill="currentColor" stroke="none" />
      <path d="M10.5 19.8v-1.8M13.5 19.8v-1.8" />
    {:else if kind === "assist"}
      <path d="M12 6v12M6 12h12" />
    {:else if kind === "clutch"}
      <path d="M12 2.8l7.5 3v5.6c0 4.5-3.2 8.2-7.5 9.4-4.3-1.2-7.5-4.9-7.5-9.4V5.8z" />
      <path d="M8.8 12.2l2.2 2.2 4.3-4.4" />
    {:else if kind === "manual"}
      <path d="M7 3h10v18l-5-3.7L7 21z" />
    {:else}
      <circle cx="12" cy="12" r="4" />
    {/if}
  </svg>
{/if}

<style>
  .mtext { font-weight: 700; letter-spacing: 0.02em; line-height: 1; font-variant-numeric: tabular-nums; }
</style>
