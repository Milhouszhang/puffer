<script lang="ts">
  import { onMount } from "svelte";
  import { addMonitorRule, createMonitor, loadLarkChats } from "../../api/desktop";
  import type { LarkChat } from "../../types";

  type Props = {
    connectionSlug: string;
    connectorSlug: string;
    onDone: () => void;
  };

  let props: Props = $props();

  const MAX_RETRIES = 5;
  const RETRY_DELAY_MS = 1500;

  let chats = $state<LarkChat[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let submitting = $state(false);
  let searchQuery = $state("");
  let checkedIds = $state(new Set<string>());

  let filteredChats = $derived(
    searchQuery.trim()
      ? chats.filter((c) => c.name.toLowerCase().includes(searchQuery.trim().toLowerCase()))
      : chats
  );

  let hasChecked = $derived(checkedIds.size > 0);

  function toggleChat(chatId: string, checked: boolean) {
    const next = new Set(checkedIds);
    if (checked) {
      next.add(chatId);
    } else {
      next.delete(chatId);
    }
    checkedIds = next;
  }

  function conversationTypeTag(type: string): string | null {
    if (type === "person") return null;
    if (type === "bot") return "Bot";
    if (type === "external") return "External";
    if (type === "official") return "Official";
    return null;
  }

  async function fetchChatsWithRetry(): Promise<void> {
    loading = true;
    error = null;
    let lastError: string | null = null;
    for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
      if (attempt > 0) {
        await new Promise<void>((resolve) => setTimeout(resolve, RETRY_DELAY_MS));
      }
      try {
        const result = await loadLarkChats(props.connectionSlug);
        if (result.length > 0) {
          chats = result;
          loading = false;
          return;
        }
        // Empty = feed not ready yet; retry
        lastError = null;
      } catch (e) {
        lastError = (e as Error).message ?? String(e);
      }
    }
    // Exhausted retries
    loading = false;
    if (lastError) {
      error = `Failed to load conversations: ${lastError}`;
    } else {
      // Legitimately empty after retries
      chats = [];
    }
  }

  async function handleSubmit() {
    if (!hasChecked || submitting) return;
    submitting = true;
    error = null;
    try {
      await createMonitor(props.connectionSlug);
      for (const chatName of checkedIds) {
        await addMonitorRule({
          connection_slug: props.connectionSlug,
          mode: "include",
          kind: "field",
          field: "chat_name",
          operator: "equals",
          value: chatName
        });
      }
      props.onDone();
    } catch (e) {
      error = `Failed to save: ${(e as Error).message ?? String(e)}`;
      submitting = false;
    }
  }

  function handleSkip() {
    props.onDone();
  }

  onMount(() => {
    void fetchChatsWithRetry();
  });
</script>

<div class="pf-modal-scrim pf-lark-picker-scrim" role="presentation" onkeydown={() => {}}>
  <div
    class="pf-modal pf-lark-picker-modal"
    role="dialog"
    aria-label="Choose which Lark conversations to receive"
    aria-modal="true"
    tabindex="-1"
    onclick={(event) => event.stopPropagation()}
    onkeydown={(event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        handleSkip();
      }
    }}
  >
    <div class="pf-modal-head">
      <div class="pf-modal-title-group">
        <div class="pf-modal-title">Choose which conversations to receive</div>
        <div class="pf-modal-eyebrow">{props.connectorSlug}</div>
      </div>
      <button type="button" class="pf-modal-close" onclick={handleSkip} aria-label="Skip">
        ✕
      </button>
    </div>

    <div class="pf-modal-body pf-lark-picker-body">
      {#if loading}
        <div class="pf-connector-question-loading" role="status" aria-live="polite">
          <span class="pf-connector-loading-spinner" aria-hidden="true"></span>
          <div>
            <strong>Loading conversations…</strong>
            <span>Please wait while the Lark feed initializes.</span>
          </div>
        </div>
      {:else if error}
        <div class="pf-lark-picker-error" role="alert">
          <strong>Something went wrong</strong>
          <span>{error}</span>
        </div>
      {:else if chats.length === 0}
        <div class="pf-lark-picker-empty">
          <p>No conversations found.</p>
          <p class="pf-lark-picker-hint">Nothing selected = nothing received. You can choose later in the filter rules.</p>
        </div>
      {:else}
        <div class="pf-lark-picker-search">
          <input
            class="sc-input pf-lark-picker-search-input"
            type="search"
            placeholder="Search conversation names…"
            value={searchQuery}
            oninput={(e) => (searchQuery = (e.currentTarget as HTMLInputElement).value)}
            aria-label="Search conversations"
          />
        </div>

        {#if filteredChats.length === 0}
          <div class="pf-lark-picker-empty">
            <p>No matching conversations.</p>
          </div>
        {:else}
          <div class="pf-lark-picker-list" role="list">
            {#each filteredChats as chat (chat.chat_id)}
              {@const tag = conversationTypeTag(chat.conversation_type)}
              <label class="pf-lark-picker-row" role="listitem">
                <input
                  type="checkbox"
                  checked={checkedIds.has(chat.name)}
                  onchange={(e) => toggleChat(chat.name, (e.currentTarget as HTMLInputElement).checked)}
                />
                <span class="pf-lark-picker-row-name">{chat.name}</span>
                {#if tag}
                  <span class="pf-lark-picker-tag">{tag}</span>
                {/if}
                {#if chat.unread}
                  <span class="pf-lark-picker-unread" aria-label="Has unread messages"></span>
                {/if}
              </label>
            {/each}
          </div>
        {/if}

        {#if !hasChecked}
          <p class="pf-lark-picker-hint">Nothing selected = nothing received. You can choose later in the filter rules.</p>
        {/if}
      {/if}
    </div>

    <div class="pf-modal-foot pf-lark-picker-foot">
      {#if error && !loading}
        <div class="pf-lark-picker-foot-error" role="alert">{error}</div>
      {/if}
      <div class="pf-modal-foot-btns">
        <button
          type="button"
          class="sc-btn"
          data-variant="outline"
          data-size="sm"
          onclick={handleSkip}
          disabled={submitting}
        >
          Stay quiet for now
        </button>
        <button
          type="button"
          class="sc-btn"
          data-variant="default"
          data-size="sm"
          disabled={!hasChecked || submitting || loading}
          aria-busy={submitting}
          onclick={() => void handleSubmit()}
        >
          {submitting ? "Saving…" : "Start receiving selected"}
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  .pf-lark-picker-scrim {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.45);
  }

  .pf-lark-picker-modal {
    width: 520px;
    max-width: calc(100vw - 48px);
    max-height: calc(100vh - 80px);
    display: flex;
    flex-direction: column;
  }

  .pf-lark-picker-body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: hidden;
  }

  .pf-lark-picker-search {
    padding: 0 0 4px;
  }

  .pf-lark-picker-search-input {
    width: 100%;
  }

  .pf-lark-picker-list {
    overflow-y: auto;
    max-height: 340px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-right: 2px;
  }

  .pf-lark-picker-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
  }

  .pf-lark-picker-row:hover {
    background: var(--muted);
  }

  .pf-lark-picker-row-name {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pf-lark-picker-tag {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--muted);
    color: var(--muted-foreground);
    flex-shrink: 0;
  }

  .pf-lark-picker-unread {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--primary, #4f8ef7);
    flex-shrink: 0;
  }

  .pf-lark-picker-hint {
    font-size: 12px;
    color: var(--muted-foreground);
    margin: 4px 0 0;
  }

  .pf-lark-picker-empty {
    padding: 24px 0;
    text-align: center;
    color: var(--muted-foreground);
    font-size: 13px;
  }

  .pf-lark-picker-error {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--destructive, #e53e3e) 12%, transparent);
    font-size: 13px;
  }

  .pf-lark-picker-foot {
    flex-direction: column;
    gap: 8px;
  }

  .pf-lark-picker-foot-error {
    font-size: 12px;
    color: var(--destructive, #e53e3e);
  }
</style>
