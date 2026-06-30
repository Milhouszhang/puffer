<script lang="ts">
  import "../../design/settings.css";

  import { saveRemoteSettings, saveSecret } from "../../api/desktop";
  import Icon from "../../design/Icon.svelte";
  import type {
    AgentEnvSettings,
    AgentEnvSandboxDefaults,
    SaveRemoteSettingsInput,
    SecretSummary,
    SettingsSnapshot,
    SshHostSettings
  } from "../../types";

  type Props = {
    snapshot: SettingsSnapshot | null;
    daemonReachable: boolean;
    onSaved: (snapshot: SettingsSnapshot) => void;
    onRefresh: () => void;
  };

  type SshDraft = SshHostSettings;

  let props: Props = $props();

  const defaultAgentEnv: AgentEnvSettings = {
    enabled: false,
    apiUrl: "https://api.agentenv.io",
    runnerHost: null,
    workspace: null,
    credentialSecretId: null,
    hasCredential: false,
    authMethod: "api_key",
    defaults: {
      sandboxType: "small",
      image: "python:3.11-slim",
      region: null,
      cpuMillis: null,
      memoryMb: null,
      gpuCount: 0,
      gpuType: null,
      maxLifetimeSeconds: null
    }
  };

  let lastSnapshotKey = $state("");
  let saving = $state(false);
  let error = $state<string | null>(null);
  let saved = $state<string | null>(null);
  let defaultTarget = $state<string | null>(null);
  let sshHosts = $state<SshDraft[]>([]);
  let agentenv = $state<AgentEnvSettings>({ ...defaultAgentEnv, defaults: { ...defaultAgentEnv.defaults } });
  let agentenvCredentialDraft = $state("");

  let disabled = $derived(!props.daemonReachable || saving);

  $effect(() => {
    const key = JSON.stringify(props.snapshot?.remote ?? null);
    if (key === lastSnapshotKey) return;
    lastSnapshotKey = key;
    const remote = props.snapshot?.remote;
    defaultTarget = remote?.defaultTarget ?? null;
    sshHosts = remote?.sshHosts ?? [];
    const nextAgentEnv = remote?.agentenv ?? defaultAgentEnv;
    agentenv = {
      ...nextAgentEnv,
      defaults: { ...nextAgentEnv.defaults }
    };
    agentenvCredentialDraft = "";
  });

  function normalize(value: string | null | undefined): string | null {
    const trimmed = (value ?? "").trim();
    return trimmed ? trimmed : null;
  }

  function numberOrNull(value: string): number | null {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
  }

  function validateSshPort(port: number | null | undefined): number | null {
    if (port == null) return null;
    if (port === 1) throw new Error("SSH port 1 is invalid. Leave the port blank or use 22.");
    if (port <= 0) throw new Error("SSH port must be greater than 0.");
    return port;
  }

  function validateAgentEnvWorkspace(workspace: string | null): string | null {
    if (!workspace) return null;
    const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
    if (!uuidPattern.test(workspace)) {
      throw new Error("AgentEnv workspace must be the full workspace UUID, not the shortened table display value.");
    }
    return workspace;
  }

  function secretLabel(): string {
    const workspace = normalize(agentenv.workspace);
    return workspace ? `AgentEnv ${workspace} credential` : "AgentEnv credential";
  }

  function matchingSecretId(items: SecretSummary[]): string | null {
    const label = secretLabel();
    const origin = normalize(agentenv.apiUrl);
    const matches = items
      .filter((secret) => secret.label === label && (secret.origin ?? "") === origin)
      .sort((a, b) => b.updatedAtMs - a.updatedAtMs);
    return matches[0]?.id ?? null;
  }

  async function persistAgentEnvSecretIfNeeded(): Promise<string | null> {
    const value = agentenvCredentialDraft.trim();
    if (!value) return agentenv.credentialSecretId;
    const snapshot = await saveSecret({
      label: secretLabel(),
      value,
      description: "AgentEnv existing account credential",
      username: normalize(agentenv.workspace),
      origin: normalize(agentenv.apiUrl)
    });
    const id = matchingSecretId(snapshot.secrets.items);
    if (!id) throw new Error("Saved AgentEnv credential, but the secret id was not returned.");
    return id;
  }

  function updateAgentEnvDefaults(patch: Partial<AgentEnvSandboxDefaults>) {
    agentenv = {
      ...agentenv,
      defaults: {
        ...agentenv.defaults,
        ...patch
      }
    };
  }

  function addSshHost() {
    const id = `ssh-${Date.now().toString(36)}`;
    sshHosts = [
      ...sshHosts,
      {
        id,
        label: "SSH host",
        target: "",
        port: null,
        cwd: null
      }
    ];
  }

  function updateSshHost(id: string, patch: Partial<SshDraft>) {
    sshHosts = sshHosts.map((host) => (host.id === id ? { ...host, ...patch } : host));
  }

  function removeSshHost(id: string) {
    sshHosts = sshHosts.filter((host) => host.id !== id);
    if (defaultTarget === `ssh:${id}`) defaultTarget = null;
  }

  async function saveSettings() {
    if (disabled) return;
    saving = true;
    error = null;
    saved = null;
    try {
      const credentialSecretId = await persistAgentEnvSecretIfNeeded();
      const savedSshHosts: SaveRemoteSettingsInput["sshHosts"] = [];
      for (const host of sshHosts) {
        const id = host.id.trim();
        const target = host.target.trim();
        if (!id && !target) continue;
        if (!id || !target) throw new Error("Each SSH host needs both an id and target.");
        savedSshHosts.push({
          id,
          label: host.label.trim() || id,
          target,
          port: validateSshPort(host.port),
          cwd: normalize(host.cwd)
        });
      }
      const input: SaveRemoteSettingsInput = {
        defaultTarget,
        sshHosts: savedSshHosts,
        agentenv: {
          enabled: agentenv.enabled,
          apiUrl: normalize(agentenv.apiUrl) ?? "https://api.agentenv.io",
          runnerHost: normalize(agentenv.runnerHost),
          workspace: validateAgentEnvWorkspace(normalize(agentenv.workspace)),
          credentialSecretId,
          authMethod: agentenv.authMethod === "access_token" ? "access_token" : "api_key",
          defaults: {
            sandboxType: normalize(agentenv.defaults.sandboxType) ?? "small",
            image: normalize(agentenv.defaults.image) ?? "python:3.11-slim",
            region: normalize(agentenv.defaults.region),
            cpuMillis: agentenv.defaults.cpuMillis,
            memoryMb: agentenv.defaults.memoryMb,
            gpuCount: agentenv.defaults.gpuCount,
            gpuType: normalize(agentenv.defaults.gpuType),
            maxLifetimeSeconds: agentenv.defaults.maxLifetimeSeconds
          }
        }
      };
      const snapshot = await saveRemoteSettings(input);
      saved = "Saved remote execution settings.";
      props.onSaved(snapshot);
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    } finally {
      saving = false;
    }
  }
</script>

<h2>Remote Execution</h2>
<p class="lead">Connect an existing AgentEnv account or SSH hosts for future remote tool execution.</p>

{#if error}
  <div class="pf-settings-note warn">{error}</div>
{/if}
{#if saved}
  <div class="pf-settings-note">{saved}</div>
{/if}
{#if !props.daemonReachable}
  <div class="pf-settings-note">Preview mode - connect the local daemon to edit remote settings.</div>
{/if}

<section class="pf-card">
  <div class="pf-card-head">
    <div>
      <h3>AgentEnv account</h3>
      <p>Use an existing AgentEnv Cloud account. The credential is stored as a Puffer secret.</p>
    </div>
    <label class="pf-switch">
      <input
        type="checkbox"
        checked={agentenv.enabled}
        disabled={disabled}
        onchange={(e) => (agentenv = { ...agentenv, enabled: (e.currentTarget as HTMLInputElement).checked })}
      />
      Enabled
    </label>
  </div>

  <div class="pf-form-grid">
    <label>
      API URL
      <input
        class="sc-input"
        value={agentenv.apiUrl}
        disabled={disabled}
        placeholder="https://api.agentenv.io"
        oninput={(e) => (agentenv = { ...agentenv, apiUrl: (e.currentTarget as HTMLInputElement).value })}
      />
    </label>
    <label>
      Workspace ID
      <input
        class="sc-input"
        value={agentenv.workspace ?? ""}
        disabled={disabled}
        placeholder="wk_..."
        oninput={(e) => (agentenv = { ...agentenv, workspace: (e.currentTarget as HTMLInputElement).value })}
      />
    </label>
    <label>
      Runner host
      <input
        class="sc-input"
        value={agentenv.runnerHost ?? ""}
        disabled={disabled}
        placeholder="Optional, e.g. 93.115.25.198"
        oninput={(e) => (agentenv = { ...agentenv, runnerHost: (e.currentTarget as HTMLInputElement).value })}
      />
    </label>
    <label>
      Auth method
      <select
        class="sc-input"
        value={agentenv.authMethod}
        disabled={disabled}
        onchange={(e) =>
          (agentenv = {
            ...agentenv,
            authMethod: (e.currentTarget as HTMLSelectElement).value as "api_key" | "access_token"
          })}
      >
        <option value="api_key">API key</option>
        <option value="access_token">Access token</option>
      </select>
    </label>
    <label>
      Credential
      <input
        class="sc-input"
        type="password"
        value={agentenvCredentialDraft}
        disabled={disabled}
        placeholder={agentenv.hasCredential ? "Stored secret configured" : "Paste token to save"}
        oninput={(e) => (agentenvCredentialDraft = (e.currentTarget as HTMLInputElement).value)}
      />
    </label>
  </div>

  <div class="pf-form-grid">
    <label>
      Sandbox size
      <select
        class="sc-input"
        value={agentenv.defaults.sandboxType}
        disabled={disabled}
        onchange={(e) => updateAgentEnvDefaults({ sandboxType: (e.currentTarget as HTMLSelectElement).value })}
      >
        <option value="micro">micro</option>
        <option value="small">small</option>
        <option value="medium">medium</option>
        <option value="large">large</option>
        <option value="xl">xl</option>
      </select>
    </label>
    <label>
      Image
      <input
        class="sc-input"
        value={agentenv.defaults.image}
        disabled={disabled}
        placeholder="python:3.11-slim"
        oninput={(e) => updateAgentEnvDefaults({ image: (e.currentTarget as HTMLInputElement).value })}
      />
    </label>
    <label>
      Region
      <input
        class="sc-input"
        value={agentenv.defaults.region ?? ""}
        disabled={disabled}
        placeholder="Optional"
        oninput={(e) => updateAgentEnvDefaults({ region: (e.currentTarget as HTMLInputElement).value })}
      />
    </label>
    <label>
      Max lifetime seconds
      <input
        class="sc-input"
        inputmode="numeric"
        value={agentenv.defaults.maxLifetimeSeconds ?? ""}
        disabled={disabled}
        placeholder="Optional"
        oninput={(e) => updateAgentEnvDefaults({ maxLifetimeSeconds: numberOrNull((e.currentTarget as HTMLInputElement).value) })}
      />
    </label>
  </div>
</section>

<section class="pf-card">
  <div class="pf-card-head">
    <div>
      <h3>SSH hosts</h3>
      <p>Save host records now; runner bootstrap and tunnel lifecycle will use these next.</p>
    </div>
    <button type="button" class="sc-btn" data-variant="outline" data-size="sm" disabled={disabled} onclick={addSshHost}>
      <Icon name="plus" size={13} />Add SSH host
    </button>
  </div>

  {#each sshHosts as host (host.id)}
    <div class="pf-remote-host">
      <div class="pf-form-grid">
        <label>
          ID
          <input class="sc-input" value={host.id} disabled={disabled} oninput={(e) => updateSshHost(host.id, { id: (e.currentTarget as HTMLInputElement).value })} />
        </label>
        <label>
          Label
          <input class="sc-input" value={host.label} disabled={disabled} oninput={(e) => updateSshHost(host.id, { label: (e.currentTarget as HTMLInputElement).value })} />
        </label>
        <label>
          Target
          <input class="sc-input" value={host.target} disabled={disabled} placeholder="user@hostname" oninput={(e) => updateSshHost(host.id, { target: (e.currentTarget as HTMLInputElement).value })} />
        </label>
        <label>
          Port
          <input
            class="sc-input"
            inputmode="numeric"
            value={host.port ?? ""}
            disabled={disabled}
            placeholder="22"
            oninput={(e) => updateSshHost(host.id, { port: numberOrNull((e.currentTarget as HTMLInputElement).value) })}
          />
        </label>
        <label>
          CWD
          <input class="sc-input" value={host.cwd ?? ""} disabled={disabled} placeholder="/home/user/project" oninput={(e) => updateSshHost(host.id, { cwd: (e.currentTarget as HTMLInputElement).value })} />
        </label>
      </div>
      <div class="pf-remote-actions">
        <button type="button" class="sc-btn" data-variant="outline" data-size="sm" disabled={disabled} onclick={() => (defaultTarget = `ssh:${host.id}`)}>
          {defaultTarget === `ssh:${host.id}` ? "Default host" : "Set default"}
        </button>
        <button type="button" class="sc-btn" data-variant="ghost" data-size="sm" disabled={disabled} onclick={() => removeSshHost(host.id)}>
          <Icon name="trash" size={13} />Remove
        </button>
      </div>
    </div>
  {/each}

  {#if sshHosts.length === 0}
    <div class="pf-empty">No SSH hosts configured.</div>
  {/if}
</section>

<div class="pf-settings-actions">
  <button type="button" class="sc-btn" data-variant="outline" disabled={disabled} onclick={props.onRefresh}>
    <Icon name="refresh" size={13} />Refresh
  </button>
  <button type="button" class="sc-btn" data-variant="default" disabled={disabled} onclick={saveSettings}>
    <Icon name="check" size={13} />{saving ? "Saving..." : "Save remote settings"}
  </button>
</div>

<style>
  .pf-card {
    display: grid;
    gap: 14px;
    border: 1px solid var(--pf-border, var(--border));
    border-radius: 8px;
    padding: 16px;
    margin: 14px 0;
    background: var(--pf-surface, var(--background));
  }

  .pf-card-head,
  .pf-remote-actions,
  .pf-settings-actions {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }

  .pf-card-head h3 {
    margin: 0 0 4px;
  }

  .pf-card-head p {
    margin: 0;
    color: var(--pf-muted, var(--muted-foreground));
    font-size: 13px;
  }

  .pf-form-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
    gap: 12px;
  }

  .pf-form-grid label {
    display: grid;
    gap: 6px;
    font-size: 12px;
    color: var(--pf-muted, var(--muted-foreground));
  }

  .pf-switch {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }

  .pf-remote-host {
    display: grid;
    gap: 10px;
    border-top: 1px solid var(--pf-border, var(--border));
    padding-top: 12px;
  }
</style>
