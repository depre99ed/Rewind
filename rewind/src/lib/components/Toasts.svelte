<script lang="ts">
  import { CheckCircle2, XCircle, Info } from "@lucide/svelte";
  import { app } from "$lib/state.svelte";
  import { fileUrl } from "$lib/api";
  import { liquid } from "$lib/glass";
</script>

<div class="toasts">
  {#each app.toasts as t (t.id)}
    <div class="toast glass strong {t.kind}" role="status" use:liquid={{ bezel: 14, strength: 26, blur: 22, saturate: 1.8 }}>
      {#if t.clip?.thumb}
        <button class="thumb" onclick={() => { app.openClip = t.clip!; app.toasts = app.toasts.filter((x) => x.id !== t.id); }} aria-label="Ouvrir">
          <img src={fileUrl(t.clip.thumb)} alt="" />
        </button>
      {:else}
        <span class="ico">
          {#if t.kind === "ok"}<CheckCircle2 size={18} />{:else if t.kind === "err"}<XCircle size={18} />{:else}<Info size={18} />{/if}
        </span>
      {/if}
      <div class="txt">
        <strong>{t.title}</strong>
        {#if t.text}<span>{t.text}</span>{/if}
      </div>
    </div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: 18px; bottom: 18px; z-index: 50; display: flex; flex-direction: column; gap: 8px; pointer-events: none; }
  .toast { pointer-events: auto; display: flex; align-items: center; gap: 12px; width: 340px; padding: 10px 14px 10px 10px; border-radius: 16px; animation: toast-in 420ms var(--spring) both; }
  .toast > :global(*) { position: relative; z-index: 2; }
  @keyframes toast-in { from { opacity: 0; transform: translateY(10px) scale(0.97); } to { opacity: 1; transform: none; } }
  .ico { width: 34px; height: 34px; display: grid; place-items: center; flex: none; border-radius: 10px; background: color-mix(in srgb, var(--text) 7%, transparent); }
  .ok .ico { color: var(--text); }
  :root[data-theme="light"] .ok .ico { color: color-mix(in srgb, var(--hi) 55%, #000); }
  .err .ico { color: var(--rec); }
  .thumb { width: 72px; aspect-ratio: 16 / 9; border-radius: 8px; overflow: hidden; flex: none; border: 1px solid var(--line); }
  .thumb img { width: 100%; height: 100%; object-fit: cover; }
  .txt { display: flex; flex-direction: column; min-width: 0; font-size: 13px; }
  .txt span { color: var(--text-2); font-size: 12.5px; overflow: hidden; text-overflow: ellipsis; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
</style>
