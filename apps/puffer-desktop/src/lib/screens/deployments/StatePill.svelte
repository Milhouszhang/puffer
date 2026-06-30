<script lang="ts">
  import type { DeployState } from "../../data/mockDeployments";
  type Props = { state: DeployState };
  let { state }: Props = $props();

  type Info = { label: string; color: string };
  const map: Record<DeployState, Info> = {
    healthy:   { label: "Healthy",   color: "oklch(0.55 0.16 145)" },
    deploying: { label: "Deploying", color: "oklch(0.55 0.14 240)" },
    degraded:  { label: "Degraded",  color: "oklch(0.58 0.17 55)"  },
    drift:     { label: "Drift",     color: "oklch(0.58 0.15 30)"  },
    failed:    { label: "Failed",    color: "oklch(0.55 0.18 25)"  }
  };
  let info = $derived(map[state]);
</script>

<span class="pf-dep-state" data-state={state} style="color: {info.color};">
  <span class="pf-dep-state-dot {state === 'deploying' ? 'pulse' : ''}" style="background: {info.color};"></span>
  {info.label}
</span>
