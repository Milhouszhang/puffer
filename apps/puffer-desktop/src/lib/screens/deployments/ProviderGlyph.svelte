<script lang="ts">
  type Props = { kind: string; size?: number };
  let { kind, size = 14 }: Props = $props();
</script>

{#if kind === "vercel"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor"><path d="M12 3l10 17H2z" /></svg>
{:else if kind === "aws"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M3 13c0 3 3 6 9 6s9-3 9-6" />
    <path d="M4 9.5c0 2 2.5 3.5 8 3.5s8-1.5 8-3.5S17.5 6 12 6 4 7.5 4 9.5z" />
    <path d="M4 9.5v-2C4 5.5 6.5 4 12 4s8 1.5 8 3.5v2" />
  </svg>
{:else if kind === "fly"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M3 18c4-6 7-9 9-9s4 2 9 9" />
    <circle cx="16" cy="8" r="1.5" />
  </svg>
{:else if kind === "railway"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="3" y="7" width="18" height="10" rx="2" />
    <path d="M3 11h18" />
    <circle cx="8" cy="14" r="1" />
    <circle cx="16" cy="14" r="1" />
  </svg>
{:else if kind === "cloudflare"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M6 17h11a4 4 0 0 0 0-8 6 6 0 0 0-11-2 4 4 0 0 0 0 10z" />
  </svg>
{:else if kind === "supabase"}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M12 3v11h8z" />
    <path d="M12 21V10H4z" />
  </svg>
{:else}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <path d="M17.5 19a4.5 4.5 0 1 0-1.3-8.8 6 6 0 0 0-11.7 1.6A3.5 3.5 0 0 0 5.5 19z" />
  </svg>
{/if}
