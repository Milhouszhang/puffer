<script lang="ts">
  import Icon from "../../design/Icon.svelte";
  import { loadLarkChats } from "../../api/desktop";
  import type {
    MonitorRuleAddRequest,
    MonitorRuleMode,
    MonitorRuleOperator,
    MonitorRuleSchema,
    MonitorRuleSchemaValue,
    WorkflowBinding,
    WorkflowFilterRule
  } from "../../types";
  import {
    MESSAGE_TEXT_PATH,
    coerceRuleValue,
    defaultValueKeyForDetail,
    monitorRuleChipsForMode,
    monitorRuleDetails,
    operatorLabel,
    valueOptionsForDetail,
    valueRequiredForOperator,
    type MonitorRuleChip,
    type MonitorRuleDetail
  } from "./monitorRules";

  type Props = {
    binding: WorkflowBinding | null;
    schema?: MonitorRuleSchema | null;
    savingMode: MonitorRuleMode | null;
    deletingKey: string | null;
    onAddRule: (request: MonitorRuleAddRequest) => Promise<void> | void;
    onDeleteRule: (
      mode: MonitorRuleMode,
      rule: WorkflowFilterRule,
      key: string
    ) => Promise<void> | void;
  };

  let {
    binding,
    schema = null,
    savingMode,
    deletingKey,
    onAddRule,
    onDeleteRule
  }: Props = $props();

  let openMode = $state<MonitorRuleMode | null>(null);
  let selectedPath = $state(MESSAGE_TEXT_PATH);
  let selectedOperator = $state<MonitorRuleOperator>("contains");
  let selectedValue = $state("");
  let activeBindingSlug = "";

  // Combobox state for connector_chats value field
  let comboOpen = $state(false);
  let comboHighlightIndex = $state(-1);
  let comboInputEl = $state<HTMLInputElement | null>(null);

  // Dynamic options loading state for connector_chats fields
  let dynamicOptions = $state<MonitorRuleSchemaValue[]>([]);
  let dynamicOptionsLoading = $state(false);
  let dynamicOptionsError = $state(false);
  // Monotonic counter for fetch-race guard
  let dynamicOptionsFetchId = 0;

  let details = $derived(monitorRuleDetails(schema));
  let eventTextDetail = $derived(details.find((detail) => detail.target === "event_text"));
  let payloadDetails = $derived(details.filter((detail) => detail.target === "payload"));
  let selectedDetail = $derived(details.find((detail) => detail.path === selectedPath) ?? details[0] ?? null);
  let selectedOperators = $derived(selectedDetail?.operators ?? []);
  let selectedValueOptions = $derived(selectedDetail ? valueOptionsForDetail(selectedDetail) : []);
  let selectedNeedsValue = $derived(valueRequiredForOperator(selectedOperator));
  let includeChips = $derived(monitorRuleChipsForMode(binding, "include", schema));
  let excludeChips = $derived(monitorRuleChipsForMode(binding, "exclude", schema));
  let adding = $derived(openMode !== null && savingMode === openMode);

  // Fetch the connector's chat list, retrying while the browser tab is still
  // warming up (right after a connector starts, list_chats errors or returns
  // empty until the messenger feed has loaded). Stale fetches are ignored via
  // the fetchId guard.
  async function fetchConnectorChats(slug: string, fetchId: number) {
    for (let attempt = 0; attempt < 5; attempt++) {
      if (fetchId !== dynamicOptionsFetchId) return;
      try {
        const chats = await loadLarkChats(slug);
        if (fetchId !== dynamicOptionsFetchId) return;
        if (chats.length > 0) {
          dynamicOptions = chats.map((c) => ({ value: c.name, label: c.name }));
          if (selectedValue === "") selectedValue = String(dynamicOptions[0].value);
          dynamicOptionsLoading = false;
          return;
        }
        // loaded but empty — connector may still be warming up; retry
      } catch {
        // transient (tab warming up) — retry
        if (fetchId !== dynamicOptionsFetchId) return;
      }
      await new Promise((resolve) => setTimeout(resolve, 1500));
    }
    if (fetchId !== dynamicOptionsFetchId) return;
    // Exhausted: leave the dropdown empty so the free-text fallback shows.
    dynamicOptions = [];
    dynamicOptionsLoading = false;
  }

  // When the selected detail uses connector_chats, fetch options dynamically
  $effect(() => {
    const detail = selectedDetail;
    const slug = binding?.connection_slug;
    if (!detail || detail.optionsSource !== "connector_chats" || !slug || openMode === null) {
      return;
    }
    dynamicOptions = [];
    dynamicOptionsLoading = true;
    dynamicOptionsError = false;
    const fetchId = ++dynamicOptionsFetchId;
    void fetchConnectorChats(slug, fetchId);
  });

  // Resolve the effective value options for the current detail:
  // prefer dynamicOptions for connector_chats fields, fall back to static options
  let effectiveValueOptions = $derived(
    selectedDetail?.optionsSource === "connector_chats"
      ? dynamicOptions
      : selectedValueOptions
  );

  // Filtered combobox options based on what the user has typed
  let comboFilteredOptions = $derived.by(() => {
    if (selectedDetail?.optionsSource !== "connector_chats") return [];
    const needle = selectedValue.trim().toLowerCase();
    if (!needle) return dynamicOptions;
    return dynamicOptions.filter((opt) =>
      String(opt.label).toLowerCase().includes(needle)
    );
  });

  function openCombo() {
    comboOpen = true;
    comboHighlightIndex = -1;
  }

  function closeCombo() {
    comboOpen = false;
    comboHighlightIndex = -1;
  }

  function pickComboOption(value: string) {
    selectedValue = value;
    closeCombo();
  }

  function onComboInput(event: Event) {
    selectedValue = (event.currentTarget as HTMLInputElement).value;
    comboOpen = true;
    comboHighlightIndex = -1;
  }

  function onComboKeydown(event: KeyboardEvent) {
    if (!comboOpen) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        openCombo();
        event.preventDefault();
      }
      return;
    }
    if (event.key === "ArrowDown") {
      event.preventDefault();
      comboHighlightIndex = Math.min(comboHighlightIndex + 1, comboFilteredOptions.length - 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      comboHighlightIndex = Math.max(comboHighlightIndex - 1, 0);
    } else if (event.key === "Enter") {
      const opt = comboFilteredOptions[comboHighlightIndex];
      if (opt) {
        event.preventDefault();
        pickComboOption(String(opt.value));
      }
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeCombo();
      comboInputEl?.blur();
    }
  }

  function onComboBlur(event: FocusEvent) {
    // Only close if focus moves outside the combobox wrapper
    const related = event.relatedTarget as Node | null;
    const wrapper = (event.currentTarget as HTMLElement).closest(".pf-monitor-rule-combobox");
    if (wrapper && related && wrapper.contains(related)) return;
    closeCombo();
  }

  $effect(() => {
    const nextSlug = binding?.slug ?? "";
    if (activeBindingSlug === nextSlug) return;
    activeBindingSlug = nextSlug;
    closeBuilder();
  });

  $effect(() => {
    if (details.some((detail) => detail.path === selectedPath)) return;
    resetBuilderControls();
  });

  function startBuilder(mode: MonitorRuleMode) {
    openMode = mode;
    resetBuilderControls();
  }

  function closeBuilder() {
    openMode = null;
    resetBuilderControls();
  }

  function resetBuilderControls() {
    const first = details[0];
    selectedPath = first?.path ?? MESSAGE_TEXT_PATH;
    selectedOperator = first?.operators[0] ?? "contains";
    selectedValue = first ? defaultValueKeyForDetail(first) : "";
    dynamicOptions = [];
    dynamicOptionsLoading = false;
    dynamicOptionsError = false;
    closeCombo();
  }

  function detailForPath(path: string): MonitorRuleDetail {
    return details.find((detail) => detail.path === path) ?? details[0];
  }

  function onDetailChange(event: Event) {
    const detail = detailForPath((event.currentTarget as HTMLSelectElement).value);
    selectedPath = detail.path;
    selectedOperator = detail.operators[0] ?? "contains";
    selectedValue = defaultValueKeyForDetail(detail);
    // Reset dynamic options so the $effect re-triggers for the new detail.
    // Eagerly show loading state for connector_chats fields to avoid flashing the text input.
    dynamicOptions = [];
    dynamicOptionsLoading = detail.optionsSource === "connector_chats";
    dynamicOptionsError = false;
  }

  function onConditionChange(event: Event) {
    selectedOperator = (event.currentTarget as HTMLSelectElement).value as MonitorRuleOperator;
    if (selectedDetail) selectedValue = defaultValueKeyForDetail(selectedDetail);
  }

  function onValueInput(event: Event) {
    selectedValue = (event.currentTarget as HTMLInputElement | HTMLSelectElement).value;
  }

  async function submitCondition(event: SubmitEvent) {
    event.preventDefault();
    if (!binding || !openMode || !selectedDetail) return;
    const needsValue = valueRequiredForOperator(selectedOperator);
    const rawValue = selectedValue.trim();
    if (needsValue && rawValue.length === 0) return;

    const request: MonitorRuleAddRequest = selectedDetail.target === "event_text"
      ? {
          connection_slug: binding.connection_slug,
          mode: openMode,
          kind: "keyword",
          keywords: [rawValue],
          operator: selectedOperator,
          case_insensitive: true
        }
      : {
          connection_slug: binding.connection_slug,
          mode: openMode,
          kind: "field",
          field: selectedDetail.path,
          operator: selectedOperator,
          value: needsValue ? coerceRuleValue(selectedDetail, rawValue) : null
        };
    await onAddRule(request);
    closeBuilder();
  }

  async function deleteChip(chip: MonitorRuleChip) {
    await onDeleteRule(chip.mode, chip.rule, chip.key);
  }
</script>

<div class="pf-monitor-rule-editor">
  {#if binding}
    <section class="pf-monitor-rule-group" role="group" aria-label="Only create tasks when">
      <div class="pf-monitor-rule-group-head">
        <div>
          <strong>Only create tasks when</strong>
          <span>{includeChips.length === 0 ? "No required conditions" : `${includeChips.length} condition${includeChips.length === 1 ? "" : "s"}`}</span>
        </div>
        <button
          type="button"
          class="pf-monitor-rule-add-button"
          disabled={savingMode !== null || deletingKey !== null}
          onclick={() => startBuilder("include")}
        >
          <Icon name="plus" size={13} />Add task condition
        </button>
      </div>

      <div class="pf-monitor-rule-chip-list">
        {#each includeChips as chip (chip.key)}
          <span class="pf-monitor-rule-chip" data-mode={chip.mode} data-tone={chip.tone}>
            <span class="pf-monitor-rule-chip-text">
              <strong>{chip.detailLabel}</strong>
              <span>{chip.operatorLabel}</span>
              {#if chip.valueLabel}
                <strong>{chip.valueLabel}</strong>
              {/if}
            </span>
            <button
              type="button"
              aria-label={`Remove task condition ${chip.title}`}
              disabled={deletingKey !== null || savingMode !== null}
              onclick={() => void deleteChip(chip)}
            >
              <Icon name="x" size={10} />
            </button>
          </span>
        {/each}
      </div>

      {#if openMode === "include"}
        <form class="pf-monitor-rule-builder" onsubmit={(event) => void submitCondition(event)}>
          <label>
            <span>Message detail</span>
            <select aria-label="Message detail" value={selectedPath} onchange={onDetailChange}>
              <option value={MESSAGE_TEXT_PATH}>{eventTextDetail?.label ?? "Message text"}</option>
              {#each payloadDetails as detail (detail.path)}
                <option value={detail.path}>{detail.label}</option>
              {/each}
            </select>
          </label>
          <label>
            <span>Condition</span>
            <select aria-label="Condition" value={selectedOperator} onchange={onConditionChange}>
              {#each selectedOperators as operator (operator)}
                <option value={operator}>{operatorLabel(operator)}</option>
              {/each}
            </select>
          </label>
          {#if selectedNeedsValue}
            {#if selectedDetail?.optionsSource === "connector_chats"}
              <label>
                <span>Value</span>
                {#if dynamicOptionsLoading}
                  <input aria-label="Value" value="" placeholder="Loading groups…" disabled />
                {:else}
                  <div class="pf-monitor-rule-combobox" onblur={onComboBlur}>
                    <input
                      bind:this={comboInputEl}
                      aria-label="Value"
                      aria-autocomplete="list"
                      aria-expanded={comboOpen}
                      aria-haspopup="listbox"
                      autocomplete="off"
                      spellcheck="false"
                      value={selectedValue}
                      placeholder={dynamicOptions.length > 0 ? "Search or type a group name" : "Type a group name"}
                      oninput={onComboInput}
                      onfocus={openCombo}
                      onkeydown={onComboKeydown}
                    />
                    {#if comboOpen && comboFilteredOptions.length > 0}
                      <ul class="pf-monitor-rule-combobox-list" role="listbox" aria-label="Group names">
                        {#each comboFilteredOptions as option, i (String(option.value))}
                          <li
                            role="option"
                            aria-selected={selectedValue === String(option.value)}
                            class:highlighted={i === comboHighlightIndex}
                            onmousedown={(e) => { e.preventDefault(); pickComboOption(String(option.value)); }}
                          >
                            {option.label}
                          </li>
                        {/each}
                      </ul>
                    {/if}
                  </div>
                {/if}
              </label>
              {#if dynamicOptionsError || (!dynamicOptionsLoading && dynamicOptions.length === 0)}
                <p class="pf-monitor-rule-builder-hint">Connector not ready — type a group name</p>
              {/if}
            {:else}
              <label>
                <span>Value</span>
                {#if selectedValueOptions.length > 0}
                  <select aria-label="Value" value={selectedValue} onchange={onValueInput}>
                    {#each selectedValueOptions as option (String(option.value))}
                      <option value={String(option.value)}>{option.label}</option>
                    {/each}
                  </select>
                {:else}
                  <input
                    aria-label="Value"
                    value={selectedValue}
                    placeholder="Value"
                    oninput={onValueInput}
                  />
                {/if}
              </label>
            {/if}
          {/if}
          <div class="pf-monitor-rule-builder-actions">
            <button type="button" class="pf-secondary-button" onclick={closeBuilder}>Cancel</button>
            <button type="submit" class="pf-primary-button" disabled={adding}>
              {adding ? "Adding..." : "Add condition"}
            </button>
          </div>
        </form>
      {/if}
    </section>

    <section class="pf-monitor-rule-group" role="group" aria-label="Skip tasks when">
      <div class="pf-monitor-rule-group-head">
        <div>
          <strong>Skip tasks when</strong>
          <span>{excludeChips.length === 0 ? "No skip conditions" : `${excludeChips.length} condition${excludeChips.length === 1 ? "" : "s"}`}</span>
        </div>
        <button
          type="button"
          class="pf-monitor-rule-add-button"
          disabled={savingMode !== null || deletingKey !== null}
          onclick={() => startBuilder("exclude")}
        >
          <Icon name="plus" size={13} />Add skip condition
        </button>
      </div>

      <div class="pf-monitor-rule-chip-list">
        {#each excludeChips as chip (chip.key)}
          <span class="pf-monitor-rule-chip" data-mode={chip.mode} data-tone={chip.tone}>
            <span class="pf-monitor-rule-chip-text">
              <strong>{chip.detailLabel}</strong>
              <span>{chip.operatorLabel}</span>
              {#if chip.valueLabel}
                <strong>{chip.valueLabel}</strong>
              {/if}
            </span>
            <button
              type="button"
              aria-label={`Remove skip condition ${chip.title}`}
              disabled={deletingKey !== null || savingMode !== null}
              onclick={() => void deleteChip(chip)}
            >
              <Icon name="x" size={10} />
            </button>
          </span>
        {/each}
      </div>

      {#if openMode === "exclude"}
        <form class="pf-monitor-rule-builder" onsubmit={(event) => void submitCondition(event)}>
          <label>
            <span>Message detail</span>
            <select aria-label="Message detail" value={selectedPath} onchange={onDetailChange}>
              <option value={MESSAGE_TEXT_PATH}>{eventTextDetail?.label ?? "Message text"}</option>
              {#each payloadDetails as detail (detail.path)}
                <option value={detail.path}>{detail.label}</option>
              {/each}
            </select>
          </label>
          <label>
            <span>Condition</span>
            <select aria-label="Condition" value={selectedOperator} onchange={onConditionChange}>
              {#each selectedOperators as operator (operator)}
                <option value={operator}>{operatorLabel(operator)}</option>
              {/each}
            </select>
          </label>
          {#if selectedNeedsValue}
            {#if selectedDetail?.optionsSource === "connector_chats"}
              <label>
                <span>Value</span>
                {#if dynamicOptionsLoading}
                  <input aria-label="Value" value="" placeholder="Loading groups…" disabled />
                {:else}
                  <div class="pf-monitor-rule-combobox" onblur={onComboBlur}>
                    <input
                      bind:this={comboInputEl}
                      aria-label="Value"
                      aria-autocomplete="list"
                      aria-expanded={comboOpen}
                      aria-haspopup="listbox"
                      autocomplete="off"
                      spellcheck="false"
                      value={selectedValue}
                      placeholder={dynamicOptions.length > 0 ? "Search or type a group name" : "Type a group name"}
                      oninput={onComboInput}
                      onfocus={openCombo}
                      onkeydown={onComboKeydown}
                    />
                    {#if comboOpen && comboFilteredOptions.length > 0}
                      <ul class="pf-monitor-rule-combobox-list" role="listbox" aria-label="Group names">
                        {#each comboFilteredOptions as option, i (String(option.value))}
                          <li
                            role="option"
                            aria-selected={selectedValue === String(option.value)}
                            class:highlighted={i === comboHighlightIndex}
                            onmousedown={(e) => { e.preventDefault(); pickComboOption(String(option.value)); }}
                          >
                            {option.label}
                          </li>
                        {/each}
                      </ul>
                    {/if}
                  </div>
                {/if}
              </label>
              {#if dynamicOptionsError || (!dynamicOptionsLoading && dynamicOptions.length === 0)}
                <p class="pf-monitor-rule-builder-hint">Connector not ready — type a group name</p>
              {/if}
            {:else}
              <label>
                <span>Value</span>
                {#if selectedValueOptions.length > 0}
                  <select aria-label="Value" value={selectedValue} onchange={onValueInput}>
                    {#each selectedValueOptions as option (String(option.value))}
                      <option value={String(option.value)}>{option.label}</option>
                    {/each}
                  </select>
                {:else}
                  <input
                    aria-label="Value"
                    value={selectedValue}
                    placeholder="Value"
                    oninput={onValueInput}
                  />
                {/if}
              </label>
            {/if}
          {/if}
          <div class="pf-monitor-rule-builder-actions">
            <button type="button" class="pf-secondary-button" onclick={closeBuilder}>Cancel</button>
            <button type="submit" class="pf-primary-button" disabled={adding}>
              {adding ? "Adding..." : "Add condition"}
            </button>
          </div>
        </form>
      {/if}
    </section>
  {:else}
    <section class="pf-monitor-rule-group is-empty" role="group" aria-label="Only create tasks when">
      <div class="pf-monitor-rule-group-head">
        <div>
          <strong>Only create tasks when</strong>
          <span>No active monitor</span>
        </div>
      </div>
    </section>
    <section class="pf-monitor-rule-group is-empty" role="group" aria-label="Skip tasks when">
      <div class="pf-monitor-rule-group-head">
        <div>
          <strong>Skip tasks when</strong>
          <span>No active monitor</span>
        </div>
      </div>
    </section>
  {/if}
</div>
