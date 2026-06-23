<script lang="ts">
  import Icon from "../design/Icon.svelte";
  import type {
    WorkflowDefinition,
    WorkflowEdge,
    WorkflowNode,
    WorkflowNodeDefinitionLight,
    WorkflowPosition
  } from "../types";

  type Props = {
    value: WorkflowDefinition;
    nodeDefinitions?: WorkflowNodeDefinitionLight[];
    disabled?: boolean;
    onChange?: (definition: WorkflowDefinition) => void;
  };

  type EditorTab = "visual" | "json";
  type NodeCategory = "trigger" | "transformer" | "executor";
  type ExecutorSubcategory =
    | "All"
    | "Productivity"
    | "Communication"
    | "Sales"
    | "AI"
    | "Cloud"
    | "Developer"
    | "Analytics"
    | "Other";

  type NodeCategoryItem = {
    key: NodeCategory;
    label: string;
    description: string;
  };

  type WorkflowServiceIdentity = {
    key: string;
    displayName: string;
  };

  type NodeServiceGroup = WorkflowServiceIdentity & {
    subcategory: ExecutorSubcategory;
    definitions: WorkflowNodeDefinitionLight[];
  };

  type NodeServiceSection = {
    label: ExecutorSubcategory | null;
    groups: NodeServiceGroup[];
  };

  type NodeDragState = {
    nodeId: string;
    pointerId: number;
    startClientX: number;
    startClientY: number;
    startX: number;
    startY: number;
    hasMoved: boolean;
  };

  type ConnectionDraft = {
    sourceId: string;
    pointerId: number;
    x: number;
    y: number;
  };

  const EMPTY_DEFINITION: WorkflowDefinition = {
    nodes: [],
    edges: []
  };
  const NODE_DRAG_THRESHOLD_PX = 4;
  const NODE_WIDTH = 168;
  const NODE_HEIGHT = 62;
  const NODE_CATEGORIES: NodeCategoryItem[] = [
    { key: "trigger", label: "Trigger", description: "how workflow starts" },
    { key: "transformer", label: "Transformer", description: "map and reshape data" },
    { key: "executor", label: "Executor", description: "call tools and services" }
  ];
  const EXECUTOR_SUBCATEGORIES: ExecutorSubcategory[] = [
    "All",
    "Productivity",
    "Communication",
    "Sales",
    "AI",
    "Cloud",
    "Developer",
    "Analytics",
    "Other"
  ];
  const FALLBACK_NODE_DEFINITIONS: WorkflowNodeDefinitionLight[] = [
    {
      type: "webhook",
      category: "trigger",
      name: "Webhook",
      description: "Trigger a workflow from an HTTP request.",
      trusted: false,
      isBuiltin: true
    }
  ];
  const SERVICE_IDENTITY_PATTERNS: Array<{
    matcher: RegExp;
    identity: WorkflowServiceIdentity;
  }> = [
    { matcher: /(^|_)azure_open_ai(_|$)/, identity: { key: "azure_openai", displayName: "Azure OpenAI" } },
    { matcher: /(^|_)open_ai(_|$)/, identity: { key: "openai", displayName: "OpenAI" } },
    { matcher: /(^|_)anthropic(_|$)/, identity: { key: "anthropic", displayName: "Anthropic" } },
    { matcher: /(^|_)google_gemini(_|$)/, identity: { key: "google_gemini", displayName: "Google Gemini" } },
    { matcher: /(^|_)deep_seek(_|$)/, identity: { key: "deepseek", displayName: "DeepSeek" } },
    { matcher: /(^|_)hugging_face(_|$)|(^|_)huggingface(_|$)/, identity: { key: "huggingface", displayName: "Hugging Face" } },
    { matcher: /(^|_)duck_duck_go(_|$)|(^|_)duckduckgo(_|$)/, identity: { key: "duckduckgo", displayName: "DuckDuckGo" } },
    { matcher: /(^|_)mongo_db(_|$)|(^|_)mongodb(_|$)/, identity: { key: "mongodb", displayName: "MongoDB" } },
    { matcher: /(^|_)you_tube(_|$)|(^|_)youtube(_|$)/, identity: { key: "youtube", displayName: "YouTube" } }
  ];
  const SERVICE_DISPLAY_NAME_ALIASES: Record<string, string> = {
    anthropic: "Anthropic",
    azure_openai: "Azure OpenAI",
    bamboo_hr: "BambooHR",
    bamboohr: "BambooHR",
    deepseek: "DeepSeek",
    duckduckgo: "DuckDuckGo",
    elevenlabs: "ElevenLabs",
    github: "GitHub",
    gitlab: "GitLab",
    gmail: "Gmail",
    google_gemini: "Google Gemini",
    google_calendar: "Google Calendar",
    google_docs: "Google Docs",
    google_drive: "Google Drive",
    google_forms: "Google Forms",
    google_sheets: "Google Sheets",
    google_tasks: "Google Tasks",
    googlecalendar: "Google Calendar",
    googledocs: "Google Docs",
    googledrive: "Google Drive",
    googleforms: "Google Forms",
    googlesheets: "Google Sheets",
    googletasks: "Google Tasks",
    huggingface: "Hugging Face",
    langsmith: "LangSmith",
    lc: "LC",
    linkedin: "LinkedIn",
    mondaycom: "monday.com",
    mongodb: "MongoDB",
    openai: "OpenAI",
    postgresql: "PostgreSQL",
    quickbooks: "QuickBooks",
    rocketchat: "Rocket.Chat",
    youtube: "YouTube"
  };
  const SERVICE_TO_SUBCATEGORY: Record<string, ExecutorSubcategory> = {
    jira: "Productivity",
    linear: "Productivity",
    asana: "Productivity",
    trello: "Productivity",
    notion: "Productivity",
    confluence: "Productivity",
    mondaycom: "Productivity",
    baserow: "Productivity",
    airtable: "Productivity",
    googlesheets: "Productivity",
    googledocs: "Productivity",
    googledrive: "Productivity",
    googlecalendar: "Productivity",
    googleforms: "Productivity",
    googletasks: "Productivity",
    dropbox: "Productivity",
    box: "Productivity",
    calendly: "Productivity",
    calcom: "Productivity",
    zoom: "Productivity",
    bamboo: "Productivity",
    bamboo_hr: "Productivity",
    bamboohr: "Productivity",
    slack: "Communication",
    discord: "Communication",
    telegram: "Communication",
    matrix: "Communication",
    rocketchat: "Communication",
    dingtalk: "Communication",
    lark: "Communication",
    line: "Communication",
    kook: "Communication",
    qq: "Communication",
    gmail: "Communication",
    email: "Communication",
    smtp: "Communication",
    sendgrid: "Communication",
    twilio: "Communication",
    elevenlabs: "Communication",
    hubspot: "Sales",
    salesforce: "Sales",
    pipedrive: "Sales",
    stripe: "Sales",
    shopify: "Sales",
    paypal: "Sales",
    quickbooks: "Sales",
    lc: "AI",
    openai: "AI",
    azure_openai: "AI",
    anthropic: "AI",
    google_gemini: "AI",
    deepseek: "AI",
    mistral: "AI",
    perplexity: "AI",
    ai: "AI",
    llm: "AI",
    huggingface: "AI",
    guardrails: "AI",
    knowledge: "AI",
    pinecone: "AI",
    qdrant: "AI",
    aws: "Cloud",
    s3: "Cloud",
    google: "Cloud",
    cloudflare: "Cloud",
    ssh: "Cloud",
    redis: "Cloud",
    mongodb: "Cloud",
    mysql: "Cloud",
    postgresql: "Cloud",
    postgres: "Cloud",
    kafka: "Cloud",
    github: "Developer",
    gitlab: "Developer",
    bitbucket: "Developer",
    vercel: "Developer",
    netlify: "Developer",
    npm: "Developer",
    sentry: "Developer",
    datadog: "Developer",
    grafana: "Developer",
    figma: "Developer",
    posthog: "Analytics",
    segment: "Analytics",
    wikipedia: "Analytics",
    youtube: "Analytics",
    linkedin: "Analytics"
  };

  let props: Props = $props();
  let activeTab = $state<EditorTab>("visual");
  let selectedNodeId = $state<string | null>(null);
  let nodePickerOpen = $state(false);
  let nodePickerQuery = $state("");
  let selectedNodeCategory = $state<NodeCategory>("trigger");
  let selectedExecutorSubcategory = $state<ExecutorSubcategory>("All");
  let expandedServiceKeys = $state<Set<string>>(new Set());
  let jsonText = $state("");
  let jsonError = $state<string | null>(null);
  let configText = $state("");
  let configError = $state<string | null>(null);
  let configNodeId = $state<string | null>(null);
  let lastValueText = $state("");
  let nodeDrag = $state<NodeDragState | null>(null);
  let connectionDraft = $state<ConnectionDraft | null>(null);
  let boardElement = $state<HTMLDivElement | null>(null);

  const nodeDefinitions = $derived(
    props.nodeDefinitions && props.nodeDefinitions.length > 0
      ? props.nodeDefinitions
      : FALLBACK_NODE_DEFINITIONS
  );
  const definition = $derived(normalizeWorkflowDefinition(props.value));
  const nodePickerSearch = $derived(nodePickerQuery.trim().toLowerCase());
  const categoryNodeDefinitions = $derived(
    nodeDefinitions.filter((item) => nodeCategory(item) === selectedNodeCategory)
  );
  const visibleNodeDefinitions = $derived(
    categoryNodeDefinitions
      .filter((item) =>
        selectedNodeCategory !== "executor" ||
        selectedExecutorSubcategory === "All" ||
        serviceSubcategory(serviceIdentity(item.type).key) === selectedExecutorSubcategory
      )
      .filter((item) => {
        const service = serviceIdentity(item.type);
        if (!nodePickerSearch) return true;
        return [
          item.type,
          item.name,
          item.description ?? "",
          item.category,
          service.displayName,
          serviceSubcategory(service.key)
        ]
          .join(" ")
          .toLowerCase()
          .includes(nodePickerSearch);
      })
      .sort((left, right) => nodeDisplayName(left).localeCompare(nodeDisplayName(right)))
  );
  const visibleServiceGroups = $derived(groupNodeDefinitionsByService(visibleNodeDefinitions));
  const visibleServiceSections = $derived(serviceSections(visibleServiceGroups));
  const selectedNode = $derived(
    selectedNodeId
      ? definition.nodes.find((node) => node.id === selectedNodeId) ?? null
      : null
  );

  $effect(() => {
    const nextText = JSON.stringify(definition, null, 2);
    if (nextText !== lastValueText) {
      jsonText = nextText;
      lastValueText = nextText;
      jsonError = null;
    }
    if (selectedNode && !definition.nodes.some((node) => node.id === selectedNodeId)) {
      selectedNodeId = selectedNode.id;
    }
    if (!selectedNode) {
      selectedNodeId = null;
    }
  });

  $effect(() => {
    const node = selectedNode;
    if (!node) {
      configNodeId = null;
      configText = "";
      configError = null;
      return;
    }
    if (configNodeId !== node.id) {
      configNodeId = node.id;
      configText = JSON.stringify(node.config ?? {}, null, 2);
      configError = null;
    }
  });

  function emit(nextDefinition: WorkflowDefinition) {
    const normalized = normalizeWorkflowDefinition(nextDefinition);
    lastValueText = JSON.stringify(normalized, null, 2);
    jsonText = lastValueText;
    jsonError = null;
    props.onChange?.(normalized);
  }

  function openNodePicker() {
    nodePickerOpen = true;
    nodePickerQuery = "";
    selectedNodeCategory =
      NODE_CATEGORIES.find((category) => categoryCount(category.key) > 0)?.key ?? "trigger";
    selectedExecutorSubcategory = "All";
  }

  function closeNodePicker() {
    nodePickerOpen = false;
    nodePickerQuery = "";
    selectedExecutorSubcategory = "All";
    expandedServiceKeys = new Set();
  }

  function selectNodeCategory(category: NodeCategory) {
    selectedNodeCategory = category;
    selectedExecutorSubcategory = "All";
    expandedServiceKeys = new Set();
  }

  function addNode(type: string) {
    const nodeDefinition = nodeDefinitions.find((item) => item.type === type);
    const id = uniqueNodeId(type, definition.nodes);
    const position = {
      x: 160 + definition.nodes.length * 220,
      y: 120 + (definition.nodes.length % 2) * 120
    };
    const node: WorkflowNode = {
      id,
      type,
      name: nodeDefinition?.name ?? type,
      config: defaultNodeConfig(type),
      trusted: nodeDefinition?.trusted,
      position
    };
    selectedNodeId = id;
    emit({
      ...definition,
      nodes: [...definition.nodes, node]
    });
    closeNodePicker();
  }

  function toggleServiceGroup(serviceKey: string) {
    const next = new Set(expandedServiceKeys);
    if (next.has(serviceKey)) {
      next.delete(serviceKey);
    } else {
      next.add(serviceKey);
    }
    expandedServiceKeys = next;
  }

  function serviceGroupExpanded(serviceKey: string): boolean {
    return Boolean(nodePickerSearch) || expandedServiceKeys.has(serviceKey);
  }

  function updateSelectedNode(updates: Partial<WorkflowNode>) {
    if (!selectedNode) return;
    const previousId = selectedNode.id;
    const nextId = sanitizeNodeId(updates.id ?? previousId);
    const nextNodes = definition.nodes.map((node) =>
      node.id === previousId
        ? {
            ...node,
            ...updates,
            id: nextId
          }
        : node
    );
    const nextEdges = definition.edges.map((edge) => ({
      ...edge,
      source: edge.source === previousId ? nextId : edge.source,
      target: edge.target === previousId ? nextId : edge.target
    }));
    selectedNodeId = nextId;
    emit({
      nodes: nextNodes,
      edges: nextEdges
    });
  }

  function updateNodePosition(nodeId: string, position: WorkflowPosition) {
    emit({
      ...definition,
      nodes: definition.nodes.map((node) =>
        node.id === nodeId
          ? {
              ...node,
              position
            }
          : node
      )
    });
  }

  function removeSelectedNode() {
    if (!selectedNode) return;
    const nodeId = selectedNode.id;
    selectedNodeId = null;
    emit({
      nodes: definition.nodes.filter((node) => node.id !== nodeId),
      edges: definition.edges.filter((edge) => edge.source !== nodeId && edge.target !== nodeId)
    });
  }

  function addEdge(source: string, target: string) {
    if (!source || !target || source === target) return;
    if (
      definition.edges.some(
        (edge) => edge.source === source && edge.target === target
      )
    ) {
      return;
    }
    emit({
      ...definition,
      edges: [...definition.edges, { source, target }]
    });
  }

  function removeEdge(edgeToRemove: WorkflowEdge) {
    emit({
      ...definition,
      edges: definition.edges.filter(
        (edge) => edge.source !== edgeToRemove.source || edge.target !== edgeToRemove.target
      )
    });
  }

  function applyJsonText() {
    try {
      const parsed = JSON.parse(jsonText) as unknown;
      if (!isWorkflowDefinition(parsed)) {
        jsonError = "JSON must include nodes and edges arrays.";
        return;
      }
      emit(parsed);
      jsonError = null;
    } catch (err) {
      jsonError = err instanceof Error ? err.message : String(err);
    }
  }

  function applySelectedConfig() {
    if (!selectedNode) return;
    try {
      const parsed = JSON.parse(configText) as unknown;
      if (!isPlainObject(parsed)) {
        configError = "Config must be a JSON object.";
        return;
      }
      updateSelectedNode({ config: parsed });
      configError = null;
    } catch (err) {
      configError = err instanceof Error ? err.message : String(err);
    }
  }

  function startNodeDrag(event: PointerEvent, nodeId: string) {
    if (props.disabled || event.button !== 0) return;
    const node = definition.nodes.find((item) => item.id === nodeId);
    if (!node) return;
    event.stopPropagation();
    const position = node.position ?? { x: 0, y: 0 };
    selectedNodeId = nodeId;
    nodeDrag = {
      nodeId,
      pointerId: event.pointerId,
      startClientX: event.clientX,
      startClientY: event.clientY,
      startX: position.x,
      startY: position.y,
      hasMoved: false
    };
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  }

  function moveNodeDrag(event: PointerEvent, nodeId: string) {
    if (!nodeDrag || nodeDrag.nodeId !== nodeId || nodeDrag.pointerId !== event.pointerId) return;
    const dx = event.clientX - nodeDrag.startClientX;
    const dy = event.clientY - nodeDrag.startClientY;
    const hasMoved =
      nodeDrag.hasMoved ||
      Math.abs(dx) > NODE_DRAG_THRESHOLD_PX ||
      Math.abs(dy) > NODE_DRAG_THRESHOLD_PX;
    nodeDrag = {
      ...nodeDrag,
      hasMoved
    };
    if (!hasMoved) return;
    event.preventDefault();
    updateNodePosition(nodeId, {
      x: Math.max(0, Math.round(nodeDrag.startX + dx)),
      y: Math.max(0, Math.round(nodeDrag.startY + dy))
    });
  }

  function endNodeDrag(event: PointerEvent, nodeId: string) {
    if (!nodeDrag || nodeDrag.nodeId !== nodeId || nodeDrag.pointerId !== event.pointerId) return;
    (event.currentTarget as HTMLElement).releasePointerCapture?.(event.pointerId);
    nodeDrag = null;
  }

  function nodeDragging(nodeId: string): boolean {
    return nodeDrag?.nodeId === nodeId && nodeDrag.hasMoved;
  }

  function boardPoint(event: PointerEvent): WorkflowPosition {
    if (!boardElement) return { x: 0, y: 0 };
    const rect = boardElement.getBoundingClientRect();
    return {
      x: Math.round(event.clientX - rect.left + boardElement.scrollLeft),
      y: Math.round(event.clientY - rect.top + boardElement.scrollTop)
    };
  }

  function startConnection(event: PointerEvent, sourceId: string) {
    if (props.disabled || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const point = boardPoint(event);
    connectionDraft = {
      sourceId,
      pointerId: event.pointerId,
      x: point.x,
      y: point.y
    };
    selectedNodeId = sourceId;
  }

  function moveConnection(event: PointerEvent) {
    if (!connectionDraft || connectionDraft.pointerId !== event.pointerId) return;
    const point = boardPoint(event);
    connectionDraft = {
      ...connectionDraft,
      x: point.x,
      y: point.y
    };
  }

  function finishConnection(event: PointerEvent, targetId: string) {
    if (!connectionDraft || connectionDraft.pointerId !== event.pointerId) return;
    event.preventDefault();
    event.stopPropagation();
    addEdge(connectionDraft.sourceId, targetId);
    selectedNodeId = targetId;
    connectionDraft = null;
  }

  function cancelConnection(event: PointerEvent) {
    if (!connectionDraft || connectionDraft.pointerId !== event.pointerId) return;
    connectionDraft = null;
  }

  function clearCanvasSelection(event: PointerEvent) {
    if (event.target === event.currentTarget) selectedNodeId = null;
  }

  function selectNodeWithKeyboard(event: KeyboardEvent, nodeId: string) {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    selectedNodeId = nodeId;
  }

  function nodeLabel(nodeId: string): string {
    const node = definition.nodes.find((item) => item.id === nodeId);
    return node?.name || nodeId;
  }

  function nodeConnections(nodeId: string): WorkflowEdge[] {
    return definition.edges.filter((edge) => edge.source === nodeId || edge.target === nodeId);
  }

  function edgePath(edge: WorkflowEdge): string | null {
    const source = definition.nodes.find((node) => node.id === edge.source);
    const target = definition.nodes.find((node) => node.id === edge.target);
    if (!source || !target) return null;
    return connectionPath(
      (source.position?.x ?? 0) + NODE_WIDTH,
      (source.position?.y ?? 0) + NODE_HEIGHT / 2,
      target.position?.x ?? 0,
      (target.position?.y ?? 0) + NODE_HEIGHT / 2
    );
  }

  function connectionPreviewPath(): string | null {
    const draft = connectionDraft;
    if (!draft) return null;
    const source = definition.nodes.find((node) => node.id === draft.sourceId);
    if (!source) return null;
    return connectionPath(
      (source.position?.x ?? 0) + NODE_WIDTH,
      (source.position?.y ?? 0) + NODE_HEIGHT / 2,
      draft.x,
      draft.y
    );
  }

  function connectionPath(startX: number, startY: number, endX: number, endY: number): string {
    const controlOffset = Math.max(80, Math.abs(endX - startX) / 2);
    return `M ${startX} ${startY} C ${startX + controlOffset} ${startY}, ${endX - controlOffset} ${endY}, ${endX} ${endY}`;
  }

  function nodeDisplayName(nodeDefinition: WorkflowNodeDefinitionLight): string {
    return nodeDefinition.name || nodeDefinition.type;
  }

  function nodeCategory(nodeDefinition: WorkflowNodeDefinitionLight): NodeCategory {
    const category = nodeDefinition.category.toLowerCase();
    if (category.includes("trigger")) return "trigger";
    if (category.includes("transform")) return "transformer";
    return "executor";
  }

  function categoryCount(category: NodeCategory): number {
    return nodeDefinitions.filter((item) => nodeCategory(item) === category).length;
  }

  function groupNodeDefinitionsByService(definitions: WorkflowNodeDefinitionLight[]): NodeServiceGroup[] {
    const groups = new Map<string, NodeServiceGroup>();
    for (const definition of definitions) {
      const service = serviceIdentity(definition.type);
      const existing = groups.get(service.key);
      if (existing) {
        existing.definitions.push(definition);
        continue;
      }
      groups.set(service.key, {
        ...service,
        subcategory: serviceSubcategory(service.key),
        definitions: [definition]
      });
    }
    return Array.from(groups.values())
      .map((group) => ({
        ...group,
        definitions: group.definitions.sort((left, right) =>
          nodeDisplayName(left).localeCompare(nodeDisplayName(right))
        )
      }))
      .sort((left, right) => left.displayName.localeCompare(right.displayName));
  }

  function serviceSections(groups: NodeServiceGroup[]): NodeServiceSection[] {
    if (selectedNodeCategory !== "executor" || selectedExecutorSubcategory !== "All") {
      return [{ label: null, groups }];
    }
    return EXECUTOR_SUBCATEGORIES.filter((subcategory) => subcategory !== "All")
      .map((subcategory) => ({
        label: subcategory,
        groups: groups.filter((group) => group.subcategory === subcategory)
      }))
      .filter((section) => section.groups.length > 0);
  }

  function serviceIdentity(type: string): WorkflowServiceIdentity {
    const normalized = type.trim().toLowerCase();
    const pattern = SERVICE_IDENTITY_PATTERNS.find((item) => item.matcher.test(normalized));
    if (pattern) return pattern.identity;
    const key = servicePrefix(normalized);
    return {
      key,
      displayName: serviceDisplayName(key)
    };
  }

  function servicePrefix(type: string): string {
    const normalized = type.trim().toLowerCase();
    if (!normalized) return "service";
    const firstUnderscore = normalized.indexOf("_");
    return firstUnderscore > 0 ? normalized.slice(0, firstUnderscore) : normalized;
  }

  function serviceDisplayName(serviceKey: string): string {
    const alias = SERVICE_DISPLAY_NAME_ALIASES[serviceKey];
    if (alias) return alias;
    return serviceKey
      .split(/[_\s-]+/)
      .filter(Boolean)
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join(" ");
  }

  function serviceSubcategory(serviceKey: string): ExecutorSubcategory {
    return SERVICE_TO_SUBCATEGORY[serviceKey] ?? "Other";
  }

  function normalizeWorkflowDefinition(value: WorkflowDefinition | null | undefined): WorkflowDefinition {
    if (!value || !Array.isArray(value.nodes) || !Array.isArray(value.edges)) {
      return EMPTY_DEFINITION;
    }
    return {
      nodes: value.nodes.map((node, index) => ({
        id: sanitizeNodeId(node.id || `node_${index + 1}`),
        type: String(node.type || "noop"),
        ...(node.name ? { name: String(node.name) } : {}),
        config: isPlainObject(node.config) ? node.config : {},
        ...(typeof node.trusted === "boolean" ? { trusted: node.trusted } : {}),
        ...(isPosition(node.position) ? { position: node.position } : {})
      })),
      edges: value.edges
        .filter((edge) => edge && typeof edge.source === "string" && typeof edge.target === "string")
        .map((edge) => ({
          source: edge.source,
          target: edge.target,
          ...(typeof edge.conditionScript === "string"
            ? { conditionScript: edge.conditionScript }
            : {})
        }))
    };
  }

  function isWorkflowDefinition(value: unknown): value is WorkflowDefinition {
    return Boolean(
      value &&
        typeof value === "object" &&
        Array.isArray((value as WorkflowDefinition).nodes) &&
        Array.isArray((value as WorkflowDefinition).edges)
    );
  }

  function isPlainObject(value: unknown): value is Record<string, unknown> {
    return Boolean(value) && typeof value === "object" && !Array.isArray(value);
  }

  function isPosition(value: unknown): value is { x: number; y: number } {
    if (!isPlainObject(value)) return false;
    return typeof value.x === "number" && typeof value.y === "number";
  }

  function defaultNodeConfig(type: string): Record<string, unknown> {
    if (type === "webhook") {
      return {
        path: uniqueNodeId("puffer_webhook", definition.nodes),
        methods: ["POST"],
        authentication: "none"
      };
    }
    return {};
  }

  function uniqueNodeId(type: string, nodes: WorkflowNode[]): string {
    const base = sanitizeNodeId(type || "node");
    let candidate = base;
    let index = nodes.length + 1;
    const ids = new Set(nodes.map((node) => node.id));
    while (ids.has(candidate)) {
      candidate = `${base}_${index}`;
      index += 1;
    }
    return candidate;
  }

  function sanitizeNodeId(value: string): string {
    return value.trim().replace(/[^A-Za-z0-9_-]/g, "_") || "node";
  }
</script>

<div class="pf-workflow-editor-shell">
  <div class="pf-workflow-editor-tabs" role="tablist" aria-label="Workflow editor mode">
    <button
      type="button"
      class="pf-workflow-editor-tab"
      role="tab"
      aria-selected={activeTab === "visual"}
      onclick={() => (activeTab = "visual")}
    >
      <Icon name="layers" size={13} />Visual
    </button>
    <button
      type="button"
      class="pf-workflow-editor-tab"
      role="tab"
      aria-selected={activeTab === "json"}
      onclick={() => (activeTab = "json")}
    >
      <Icon name="file" size={13} />JSON
    </button>
  </div>

  {#if activeTab === "visual"}
    <div class="pf-workflow-visual-editor" aria-label="Visual workflow editor">
      <div class="pf-workflow-canvas">
        <div class="pf-workflow-canvas-toolbar">
          <span class="pf-workflow-toolbar-hint">{definition.nodes.length} nodes</span>
          <button
            type="button"
            class="sc-btn"
            data-variant="ghost"
            data-size="sm"
            disabled={props.disabled}
            onclick={openNodePicker}
          >
            <Icon name="plus" size={12} />Add Node
          </button>
        </div>
        {#if nodePickerOpen}
          <div class="pf-workflow-node-picker-backdrop" role="presentation">
            <div
              class="pf-workflow-node-picker"
              role="dialog"
              aria-modal="true"
              aria-label="Add Node"
            >
              <aside class="pf-workflow-node-picker-side">
                <div class="pf-workflow-node-picker-title">Add Node</div>
                <label class="pf-workflow-node-picker-search">
                  <span>Search</span>
                  <input
                    aria-label="Search node types"
                    placeholder="Search..."
                    value={nodePickerQuery}
                    oninput={(event) => (nodePickerQuery = event.currentTarget.value)}
                  />
                </label>
                <div class="pf-workflow-node-picker-categories" aria-label="Node categories">
                  {#each NODE_CATEGORIES as category (category.key)}
                    <button
                      type="button"
                      class="pf-workflow-node-picker-category"
                      data-selected={selectedNodeCategory === category.key}
                      aria-pressed={selectedNodeCategory === category.key}
                      onclick={() => selectNodeCategory(category.key)}
                    >
                      <strong>{category.label}</strong>
                      <span>{category.description}</span>
                      <small>{categoryCount(category.key)}</small>
                    </button>
                  {/each}
                </div>
              </aside>
              <section class="pf-workflow-node-picker-main">
                <div class="pf-workflow-node-picker-head">
                  <strong>{NODE_CATEGORIES.find((category) => category.key === selectedNodeCategory)?.label}</strong>
                  <button
                    type="button"
                    class="sc-icon-btn"
                    aria-label="Close add node dialog"
                    onclick={closeNodePicker}
                  >
                    <Icon name="x" size={13} />
                  </button>
                </div>
                {#if selectedNodeCategory === "executor"}
                  <div class="pf-workflow-node-picker-subcategories" aria-label="Executor categories">
                    {#each EXECUTOR_SUBCATEGORIES as subcategory (subcategory)}
                      <button
                        type="button"
                        data-selected={selectedExecutorSubcategory === subcategory}
                        aria-pressed={selectedExecutorSubcategory === subcategory}
                        onclick={() => (selectedExecutorSubcategory = subcategory)}
                      >
                        {subcategory}
                      </button>
                    {/each}
                  </div>
                {/if}
                <div
                  class="pf-workflow-node-picker-list"
                  data-mode={selectedNodeCategory === "trigger" ? "definition" : "service"}
                >
                  {#if visibleNodeDefinitions.length === 0}
                    <div class="pf-workflow-empty">
                      {nodePickerQuery.trim() ? `No nodes matching "${nodePickerQuery.trim()}".` : "No nodes in this category."}
                    </div>
                  {:else if selectedNodeCategory === "trigger"}
                    {#each visibleNodeDefinitions as item (item.type)}
                      <button
                        type="button"
                        class="pf-workflow-node-picker-item"
                        onclick={() => addNode(item.type)}
                      >
                        <span>
                          <strong>{nodeDisplayName(item)}</strong>
                          <small>{item.type}</small>
                        </span>
                        {#if item.description}
                          <em>{item.description}</em>
                        {/if}
                      </button>
                    {/each}
                  {:else}
                    {#each visibleServiceSections as section (`${section.label ?? "all"}-${section.groups.length}`)}
                      {#if section.label}
                        <div class="pf-workflow-node-picker-section">
                          <span>{section.label}</span>
                          <hr />
                        </div>
                      {/if}
                      {#each section.groups as group (group.key)}
                        <div
                          class="pf-workflow-node-service"
                          data-expanded={serviceGroupExpanded(group.key)}
                        >
                          <button
                            type="button"
                            class="pf-workflow-node-service-head"
                            aria-expanded={serviceGroupExpanded(group.key)}
                            onclick={() => toggleServiceGroup(group.key)}
                          >
                            <span class="pf-workflow-node-service-mark">{group.displayName.slice(0, 1)}</span>
                            <strong>{group.displayName}</strong>
                            <small>{group.definitions.length} {group.definitions.length === 1 ? "function" : "functions"}</small>
                            <span aria-hidden="true">{serviceGroupExpanded(group.key) ? "⌃" : "⌄"}</span>
                          </button>
                          {#if serviceGroupExpanded(group.key)}
                            <div class="pf-workflow-node-function-grid">
                              {#each group.definitions as item (item.type)}
                                <button
                                  type="button"
                                  class="pf-workflow-node-function"
                                  title={item.description ?? item.type}
                                  onclick={() => addNode(item.type)}
                                >
                                  <strong>{nodeDisplayName(item)}</strong>
                                  {#if item.description}
                                    <small>{item.description}</small>
                                  {/if}
                                </button>
                              {/each}
                            </div>
                          {/if}
                        </div>
                      {/each}
                    {/each}
                  {/if}
                </div>
              </section>
            </div>
          </div>
        {/if}
        <div
          class="pf-workflow-node-board"
          role="application"
          aria-label="Workflow canvas"
          bind:this={boardElement}
          onpointermove={moveConnection}
          onpointerup={cancelConnection}
        >
          <div
            class="pf-workflow-board-surface"
            role="presentation"
            onpointerdown={clearCanvasSelection}
          >
            <svg class="pf-workflow-edge-svg" aria-hidden="true">
              {#each definition.edges as edge (`${edge.source}-${edge.target}`)}
                {#if edgePath(edge)}
                  <path class="pf-workflow-edge-path" d={edgePath(edge) ?? ""} />
                  <circle
                    class="pf-workflow-edge-dot"
                    cx={definition.nodes.find((node) => node.id === edge.target)?.position?.x ?? 0}
                    cy={(definition.nodes.find((node) => node.id === edge.target)?.position?.y ?? 0) + NODE_HEIGHT / 2}
                    r="3"
                  />
                {/if}
              {/each}
              {#if connectionPreviewPath()}
                <path class="pf-workflow-edge-preview" d={connectionPreviewPath() ?? ""} />
              {/if}
            </svg>
          {#if definition.nodes.length === 0}
            <div class="pf-workflow-empty">No nodes.</div>
          {:else}
            {#each definition.nodes as node (node.id)}
              <div
                class="pf-workflow-node-card"
                role="button"
                tabindex="0"
                aria-label={`Select node ${node.name || node.id}`}
                aria-pressed={selectedNode?.id === node.id}
                data-selected={selectedNode?.id === node.id}
                data-dragging={nodeDragging(node.id)}
                style={`--node-x: ${node.position?.x ?? 0}px; --node-y: ${node.position?.y ?? 0}px;`}
                onpointerdown={(event) => startNodeDrag(event, node.id)}
                onpointermove={(event) => moveNodeDrag(event, node.id)}
                onpointerup={(event) => endNodeDrag(event, node.id)}
                onpointercancel={(event) => endNodeDrag(event, node.id)}
                onkeydown={(event) => selectNodeWithKeyboard(event, node.id)}
              >
                <button
                  type="button"
                  class="pf-workflow-node-port pf-workflow-node-port-in"
                  aria-label={`Finish connection into ${node.name || node.id}`}
                  disabled={props.disabled || !connectionDraft || connectionDraft.sourceId === node.id}
                  onpointerup={(event) => finishConnection(event, node.id)}
                ></button>
                <span>{node.name || node.id}</span>
                <small>{node.type}</small>
                <button
                  type="button"
                  class="pf-workflow-node-port pf-workflow-node-port-out"
                  aria-label={`Start connection from ${node.name || node.id}`}
                  disabled={props.disabled}
                  onpointerdown={(event) => startConnection(event, node.id)}
                ></button>
              </div>
            {/each}
          {/if}
          </div>
        </div>
      </div>

      {#if selectedNode}
      <div class="pf-workflow-inspector">
        {#if selectedNode}
          <div class="pf-workflow-node-editor" aria-label="Selected node editor">
            <strong>Node</strong>
            <label>
              <span>ID</span>
              <input
                aria-label="Node id"
                value={selectedNode.id}
                disabled={props.disabled}
                oninput={(event) => updateSelectedNode({ id: event.currentTarget.value })}
              />
            </label>
            <label>
              <span>Name</span>
              <input
                aria-label="Node name"
                value={selectedNode.name ?? ""}
                disabled={props.disabled}
                oninput={(event) => updateSelectedNode({ name: event.currentTarget.value })}
              />
            </label>
            <label>
              <span>Type</span>
              <input aria-label="Node type value" value={selectedNode.type} disabled />
            </label>
            <label class="pf-workflow-config-editor">
              <span>Config JSON</span>
              <textarea
                aria-label="Node config JSON"
                value={configText}
                disabled={props.disabled}
                spellcheck="false"
                oninput={(event) => (configText = event.currentTarget.value)}
              ></textarea>
            </label>
            {#if configError}
              <div class="pf-workflow-inline-error" role="alert">{configError}</div>
            {/if}
            <div class="pf-workflow-editor-actions">
              <button
                type="button"
                class="sc-btn"
                data-variant="ghost"
                data-size="sm"
                disabled={props.disabled}
                onclick={applySelectedConfig}
              >
                <Icon name="check" size={12} />Apply config
              </button>
              <button
                type="button"
                class="sc-btn"
                data-variant="ghost"
                data-size="sm"
                disabled={props.disabled}
                onclick={removeSelectedNode}
              >
                <Icon name="trash" size={12} />Remove
              </button>
            </div>
            <div class="pf-workflow-edge-editor">
              <strong>Connections</strong>
              <div class="pf-workflow-edge-list">
                {#if nodeConnections(selectedNode.id).length === 0}
                  <span>No connections for this node.</span>
                {:else}
                  {#each nodeConnections(selectedNode.id) as edge (`${edge.source}-${edge.target}`)}
                    <div class="pf-workflow-edge-row">
                      <span>{nodeLabel(edge.source)} -> {nodeLabel(edge.target)}</span>
                      <button
                        type="button"
                        class="sc-icon-btn"
                        aria-label={`Remove edge ${edge.source} to ${edge.target}`}
                        disabled={props.disabled}
                        onclick={() => removeEdge(edge)}
                      >
                        <Icon name="x" size={12} />
                      </button>
                    </div>
                  {/each}
                {/if}
              </div>
            </div>
          </div>
        {/if}
      </div>
      {/if}
    </div>
  {:else}
    <div class="pf-workflow-json-editor" aria-label="Workflow JSON editor">
      <textarea
        aria-label="Workflow definition JSON"
        value={jsonText}
        disabled={props.disabled}
        spellcheck="false"
        oninput={(event) => (jsonText = event.currentTarget.value)}
      ></textarea>
      {#if jsonError}
        <div class="pf-workflow-inline-error" role="alert">{jsonError}</div>
      {/if}
      <div class="pf-workflow-editor-actions">
        <button
          type="button"
          class="sc-btn"
          data-variant="ghost"
          data-size="sm"
          disabled={props.disabled}
          onclick={applyJsonText}
        >
          <Icon name="check" size={12} />Apply JSON
        </button>
      </div>
    </div>
  {/if}
</div>
