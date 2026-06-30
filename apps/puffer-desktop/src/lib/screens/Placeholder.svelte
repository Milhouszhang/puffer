<script lang="ts">
  import Puffer from "../design/Puffer.svelte";
  import Icon, { type IconName } from "../design/Icon.svelte";

  type Chip = { label: string; icon?: IconName };

  type Props = {
    eyebrow: string;
    title: string;
    subtitle?: string;
    sub?: string;
    chips?: Chip[];
  };

  let { eyebrow, title, subtitle, sub, chips = [] }: Props = $props();
</script>

<div class="pf-screen-top">
  <div class="pf-screen-top-left">
    <span class="pf-screen-top-eyebrow">{eyebrow}</span>
    <span class="pf-screen-top-title">{title}</span>
    {#if subtitle}
      <span class="pf-screen-top-sub">{subtitle}</span>
    {/if}
  </div>
</div>

<div class="pf-placeholder">
  <Puffer size={80} state="idle" />
  <h1>{title}</h1>
  {#if sub}
    <p class="sub">{sub}</p>
  {/if}
  {#if chips.length}
    <div class="chips">
      {#each chips as chip (chip.label)}
        <span class="chip">
          {#if chip.icon}
            <Icon name={chip.icon} size={11} />
          {/if}
          {chip.label}
        </span>
      {/each}
    </div>
  {/if}
</div>
