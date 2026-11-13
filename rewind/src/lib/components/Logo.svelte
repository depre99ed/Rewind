<script lang="ts">
  // Logo Rewind « A3 » : carré vert acide, double retour noir.
  let { size = 22 }: { size?: number } = $props();
</script>

<svg viewBox="0 0 64 64" width={size} height={size} aria-hidden="true">
  <rect width="64" height="64" rx="15" fill="#c8f031" />
  <path d="M30 20 15 32l15 12Z" fill="#0b0c0f" stroke="#0b0c0f" stroke-width="6" stroke-linejoin="round" />
  <g opacity="0.55"><path d="M49 20 34 32l15 12Z" fill="#0b0c0f" stroke="#0b0c0f" stroke-width="6" stroke-linejoin="round" /></g>
</svg>
