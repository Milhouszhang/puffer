<script lang="ts">
  import "../design/chat.css";
  import Icon, { type IconName } from "../design/Icon.svelte";

  type AutomationItem = {
    id: string;
    title: string;
    description: string;
    status: string;
    source: string;
    updated: string;
    when: string;
    then: string;
    review: string;
    recent: string[];
    icon: IconName;
    prompt?: string;
    trigger?: AutomationTrigger | null;
    tools?: SelectedAutomationTool[];
    enabled?: boolean;
    owner?: string;
    history?: AutomationRun[];
  };

  type AutomationRun = {
    id: string;
    title: string;
    status: string;
    started: string;
    duration: string;
    summary: string;
  };

  type AutomationStarter = {
    id: string;
    title: string;
    description: string;
    icon: IconName;
    name: string;
    prompt: string;
    trigger: AutomationTrigger;
  };

  type AutomationTrigger = {
    icon: IconName;
    leading: string;
    target?: string;
    actorPrefix?: string;
    actor?: string;
  };

  type AutomationApp = {
    id: string;
    title: string;
    description: string;
    icon: IconName;
    capabilities: AutomationCapability[];
  };

  type AutomationCapability = {
    id: string;
    title: string;
    description: string;
    targetLabel?: string;
    targetOptions?: string[];
    defaultTarget?: string;
  };

  type VisibleAutomationApp = AutomationApp & {
    visibleCapabilities: AutomationCapability[];
  };

  type SelectedAutomationTool = {
    id: string;
    appId: string;
    appTitle: string;
    icon: IconName;
    title: string;
    targetLabel?: string;
    targetOptions: string[];
    target: string | null;
  };

  type AutomationDraft = {
    name: string;
    prompt: string;
    trigger: AutomationTrigger | null;
    tools: SelectedAutomationTool[];
  };

  const blankAutomationName = "Untitled automation";

  const automations: AutomationItem[] = [
    {
      id: "review-inbox",
      title: "Review inbox",
      description: "Review drafts before they go out.",
      status: "Needs review",
      source: "GitHub, Telegram",
      updated: "Today 09:12",
      when: "A pull request, issue, or message needs a response.",
      then: "Puffer gathers the relevant context and prepares a concise draft.",
      review: "You edit, approve, or reject the draft from the detail pane.",
      recent: ["4 drafts waiting", "2 new since this morning", "Last approved yesterday"],
      icon: "listTodo"
    },
    {
      id: "pr-review",
      title: "PR review assistant",
      description: "Summarize code changes and draft a review note.",
      status: "Ready",
      source: "GitHub",
      updated: "Starter",
      when: "A new pull request is opened or marked ready for review.",
      then: "Puffer reads the diff, checks test signals, and writes a short review draft.",
      review: "You decide whether to post, edit, or keep the note for later.",
      recent: ["Template ready", "Works best with linked repos", "Draft style: concise"],
      icon: "git"
    },
    {
      id: "calendar-rsvp",
      title: "Calendar RSVP",
      description: "Prepare RSVP suggestions with meeting context.",
      status: "Needs setup",
      source: "Calendar",
      updated: "Starter",
      when: "A new invite arrives or a meeting time changes.",
      then: "Puffer checks conflicts and drafts an accept, decline, or tentative response.",
      review: "You approve the RSVP after checking the guest list and conflicts.",
      recent: ["Choose calendars", "Set default response tone", "Keep final approval on"],
      icon: "clock"
    },
    {
      id: "release-watch",
      title: "Release watch",
      description: "Watch a release branch and surface changes that need attention.",
      status: "Paused",
      source: "GitHub Actions",
      updated: "Every 15 min",
      when: "A release check fails, recovers, or waits for a manual step.",
      then: "Puffer summarizes the change and suggests the next owner-facing update.",
      review: "You review the summary before sending it to the team.",
      recent: ["Last run yesterday", "No failures in latest run", "Paused by user"],
      icon: "rocket"
    },
    {
      id: "morning-digest",
      title: "Morning digest",
      description: "Collect overnight updates into a short start-of-day brief.",
      status: "Ready",
      source: "Slack, Calendar",
      updated: "Daily 09:00",
      when: "Your workday starts.",
      then: "Puffer groups overnight updates, upcoming meetings, and waiting reviews.",
      review: "You skim the digest and open anything that needs action.",
      recent: ["3 sources selected", "Digest length: short", "Weekdays only"],
      icon: "logs"
    }
  ];

  const automationTemplates: AutomationStarter[] = [
    {
      id: "pr-review",
      title: "Review PRs",
      description: "Prepare a concise review draft when code changes need attention.",
      icon: "git",
      name: "PR review draft",
      prompt: "When a pull request opens, summarize the changes and prepare a review note for me.",
      trigger: {
        icon: "git",
        leading: "PR opened in",
        target: "Select repos",
        actorPrefix: "by",
        actor: "Anyone"
      }
    },
    {
      id: "reply-drafts",
      title: "Reply drafts",
      description: "Turn incoming messages into replies you can edit before sending.",
      icon: "edit",
      name: "Reply draft",
      prompt: "When a message needs a response, gather context and prepare a reply draft.",
      trigger: {
        icon: "edit",
        leading: "Message arrives from",
        target: "Trusted contacts"
      }
    },
    {
      id: "calendar-rsvp",
      title: "Calendar RSVP",
      description: "Check meeting conflicts and prepare an RSVP suggestion.",
      icon: "clock",
      name: "Calendar RSVP",
      prompt: "When a calendar invite arrives, check conflicts and prepare an RSVP suggestion.",
      trigger: {
        icon: "clock",
        leading: "Invite arrives on",
        target: "Calendar",
        actorPrefix: "for",
        actor: "Any meeting"
      }
    },
    {
      id: "morning-digest",
      title: "Morning digest",
      description: "Collect overnight updates into a short start-of-day brief.",
      icon: "logs",
      name: "Morning digest",
      prompt: "Every weekday morning, summarize overnight updates and anything waiting for me.",
      trigger: {
        icon: "clock",
        leading: "Weekdays at",
        target: "09:00",
        actorPrefix: "from",
        actor: "Selected sources"
      }
    }
  ];

  const baseUserAutomations: AutomationItem[] = [];
  const everyDayTrigger: AutomationTrigger = {
    icon: "clock",
    leading: "Every day at",
    target: "09:00"
  };
  const customScheduleTrigger: AutomationTrigger = {
    icon: "clock",
    leading: "Custom schedule",
    target: "Cron"
  };
  const prOpenedTrigger: AutomationTrigger = {
    icon: "git",
    leading: "PR opened in",
    target: "Select repos",
    actorPrefix: "by",
    actor: "Anyone"
  };
  const draftOpenedTrigger: AutomationTrigger = {
    icon: "git",
    leading: "Draft opened in",
    target: "Select repos"
  };
  const commentAddedTrigger: AutomationTrigger = {
    icon: "git",
    leading: "Comment added in",
    target: "Select repos"
  };
  const labelChangeTrigger: AutomationTrigger = {
    icon: "git",
    leading: "Label changes in",
    target: "Select repos"
  };
  const commonApps: AutomationApp[] = [
    {
      id: "github",
      title: "GitHub",
      description: "Pull requests, issues, and repository events.",
      icon: "git",
      capabilities: [
        {
          id: "watch-pull-requests",
          title: "Watch Pull Requests",
          description: "Read PR titles, diffs, status, and review activity."
        },
        {
          id: "comment-on-pull-request",
          title: "Comment on Pull Request",
          description: "Prepare or post a pull request comment.",
          targetLabel: "with",
          targetOptions: ["Allow PR Approval", "Comment only", "Request changes"],
          defaultTarget: "Allow PR Approval"
        },
        {
          id: "update-commit-status",
          title: "Update Commit Status",
          description: "Set a commit status after the automation reviews a result.",
          targetLabel: "as",
          targetOptions: ["Pending", "Success", "Failure"],
          defaultTarget: "Pending"
        }
      ]
    },
    {
      id: "slack",
      title: "Slack",
      description: "Messages, channels, and team updates.",
      icon: "logs",
      capabilities: [
        {
          id: "read-slack-channels",
          title: "Read Slack Channels",
          description: "Use selected channels as context."
        },
        {
          id: "send-to-slack",
          title: "Send to Slack",
          description: "Draft or send a message to a selected channel.",
          targetLabel: "to",
          targetOptions: ["#teams", "#engineering", "#release"],
          defaultTarget: "#teams"
        },
        {
          id: "reply-in-thread",
          title: "Reply in Slack Thread",
          description: "Draft a thread reply where the update came from.",
          targetLabel: "to",
          targetOptions: ["Original thread", "#teams", "#support"],
          defaultTarget: "Original thread"
        }
      ]
    },
    {
      id: "gmail",
      title: "Gmail",
      description: "Email threads, labels, and draft replies.",
      icon: "edit",
      capabilities: [
        {
          id: "read-gmail-threads",
          title: "Read Gmail Threads",
          description: "Use email threads as context."
        },
        {
          id: "create-gmail-draft",
          title: "Create Gmail Draft",
          description: "Create a draft reply for review.",
          targetLabel: "in",
          targetOptions: ["Primary inbox", "Support inbox", "Sales inbox"],
          defaultTarget: "Primary inbox"
        },
        {
          id: "apply-gmail-label",
          title: "Apply Gmail Label",
          description: "Label a thread after the automation reviews it.",
          targetLabel: "as",
          targetOptions: ["Needs review", "Waiting", "Done"],
          defaultTarget: "Needs review"
        }
      ]
    },
    {
      id: "google-calendar",
      title: "Google Calendar",
      description: "Events, invites, and availability.",
      icon: "clock",
      capabilities: [
        {
          id: "read-calendar-events",
          title: "Read Calendar Events",
          description: "Use upcoming events and invite details as context."
        },
        {
          id: "check-availability",
          title: "Check Availability",
          description: "Compare free time before drafting a response."
        },
        {
          id: "draft-rsvp",
          title: "Draft RSVP",
          description: "Prepare an RSVP for review.",
          targetLabel: "as",
          targetOptions: ["Tentative", "Accept", "Decline"],
          defaultTarget: "Tentative"
        }
      ]
    },
    {
      id: "linear",
      title: "Linear",
      description: "Issues, projects, and triage queues.",
      icon: "listTodo",
      capabilities: [
        {
          id: "read-linear-issues",
          title: "Read Linear Issues",
          description: "Use issues and comments as context."
        },
        {
          id: "create-linear-issue",
          title: "Create Linear Issue",
          description: "Create an issue from an approved draft.",
          targetLabel: "in",
          targetOptions: ["Triage", "Product", "Engineering"],
          defaultTarget: "Triage"
        },
        {
          id: "comment-on-linear",
          title: "Comment on Linear Issue",
          description: "Draft a comment on an existing issue.",
          targetLabel: "with",
          targetOptions: ["Internal note", "Public update"],
          defaultTarget: "Internal note"
        }
      ]
    },
    {
      id: "notion",
      title: "Notion",
      description: "Pages, docs, and team knowledge.",
      icon: "file",
      capabilities: [
        {
          id: "search-notion",
          title: "Search Notion",
          description: "Use selected pages and docs as context."
        },
        {
          id: "create-notion-page",
          title: "Create Notion Page",
          description: "Draft a new page for review.",
          targetLabel: "in",
          targetOptions: ["Team wiki", "Project notes", "Runbooks"],
          defaultTarget: "Team wiki"
        },
        {
          id: "update-notion-page",
          title: "Update Notion Page",
          description: "Prepare an update to an existing page.",
          targetLabel: "in",
          targetOptions: ["Team wiki", "Project notes", "Runbooks"],
          defaultTarget: "Team wiki"
        }
      ]
    }
  ];

  type AutomationLibraryTab = "your" | "templates";
  type AutomationDetailTab = "settings" | "history";

  let screenMode = $state<"home" | "new" | "detail">("home");
  let activeAutomationLibraryTab = $state<AutomationLibraryTab>("your");
  let activeAutomationDetailTab = $state<AutomationDetailTab>("settings");
  let savedAutomations = $state<AutomationItem[]>([]);
  let savedAutomationSequence = $state(0);
  let savedRunSequence = $state(0);
  let userAutomations = $derived([...savedAutomations, ...baseUserAutomations]);
  let selectedAutomationId = $state<string | null>(null);
  let selectedAutomation = $derived(userAutomations.find((item) => item.id === selectedAutomationId) ?? null);
  let homePrompt = $state("");
  let automationName = $state(blankAutomationName);
  let automationPrompt = $state("");
  let automationTrigger = $state<AutomationTrigger | null>(null);
  let selectedTools = $state<SelectedAutomationTool[]>([]);
  let automationEnabled = $state(true);
  let triggerMenuOpen = $state(false);
  let toolMenuOpen = $state(false);
  let automationActionMenuOpen = $state(false);
  let editingToolId = $state<string | null>(null);
  let toolSearchQuery = $state("");
  let visibleToolApps = $derived(visibleAppsForSearch(toolSearchQuery));

  function applyStarter(starter: AutomationStarter) {
    automationName = starter.name;
    automationPrompt = starter.prompt;
    automationTrigger = copyTrigger(starter.trigger);
    selectedTools = [];
    automationEnabled = true;
    selectedAutomationId = null;
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
  }

  function openBlankAutomation(prompt = "") {
    automationName = blankAutomationName;
    automationPrompt = prompt.trim();
    automationTrigger = null;
    selectedTools = [];
    automationEnabled = true;
    selectedAutomationId = null;
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
    screenMode = "new";
  }

  function appById(id: string): AutomationApp | null {
    return commonApps.find((app) => app.id === id) ?? null;
  }

  function selectedToolFrom(app: AutomationApp, capability: AutomationCapability): SelectedAutomationTool {
    return {
      id: `${app.id}:${capability.id}`,
      appId: app.id,
      appTitle: app.title,
      icon: app.icon,
      title: capability.title,
      targetLabel: capability.targetLabel,
      targetOptions: capability.targetOptions ?? [],
      target: capability.defaultTarget ?? capability.targetOptions?.[0] ?? null
    };
  }

  function toolById(appId: string, capabilityId: string): SelectedAutomationTool | null {
    const app = appById(appId);
    const capability = app?.capabilities.find((candidate) => candidate.id === capabilityId);
    if (!app || !capability) return null;
    return selectedToolFrom(app, capability);
  }

  function toolsById(ids: Array<[string, string]>): SelectedAutomationTool[] {
    return ids
      .map(([appId, capabilityId]) => toolById(appId, capabilityId))
      .filter((tool): tool is SelectedAutomationTool => tool !== null);
  }

  function copyTrigger(trigger: AutomationTrigger | null): AutomationTrigger | null {
    return trigger ? { ...trigger } : null;
  }

  function copySelectedTools(tools: SelectedAutomationTool[]): SelectedAutomationTool[] {
    return tools.map((tool) => ({
      ...tool,
      targetOptions: [...tool.targetOptions]
    }));
  }

  function capabilityMatchesSearch(capability: AutomationCapability, query: string): boolean {
    return [capability.title, capability.description, capability.targetLabel, ...(capability.targetOptions ?? [])].some(
      (value) => value?.toLowerCase().includes(query)
    );
  }

  function visibleAppsForSearch(query: string): VisibleAutomationApp[] {
    const normalizedQuery = query.trim().toLowerCase();
    return commonApps
      .map((app) => {
        const appMatches =
          !normalizedQuery ||
          [app.title, app.description].some((value) => value.toLowerCase().includes(normalizedQuery));
        const visibleCapabilities = appMatches
          ? app.capabilities
          : app.capabilities.filter((capability) => capabilityMatchesSearch(capability, normalizedQuery));
        return {
          ...app,
          visibleCapabilities
        };
      })
      .filter((app) => app.visibleCapabilities.length > 0);
  }

  function draftFromPrompt(prompt: string): AutomationDraft {
    const trimmedPrompt = prompt.trim();
    const lowerPrompt = trimmedPrompt.toLowerCase();
    if (/\bpr\b|pull request/.test(lowerPrompt)) {
      return {
        name: "PR review draft",
        prompt: trimmedPrompt,
        trigger: prOpenedTrigger,
        tools: toolsById([["github", "comment-on-pull-request"]])
      };
    }
    if (/calendar|invite|rsvp|meeting/.test(lowerPrompt)) {
      return {
        name: "Calendar RSVP",
        prompt: trimmedPrompt,
        trigger: automationTemplates.find((template) => template.id === "calendar-rsvp")?.trigger ?? null,
        tools: toolsById([["google-calendar", "draft-rsvp"]])
      };
    }
    if (/gmail|email|mail/.test(lowerPrompt)) {
      return {
        name: "Email reply draft",
        prompt: trimmedPrompt,
        trigger: {
          icon: "edit",
          leading: "Email arrives in",
          target: "Gmail"
        },
        tools: toolsById([["gmail", "create-gmail-draft"]])
      };
    }
    if (/slack|message|reply/.test(lowerPrompt)) {
      return {
        name: "Reply draft",
        prompt: trimmedPrompt,
        trigger: automationTemplates.find((template) => template.id === "reply-drafts")?.trigger ?? null,
        tools: toolsById([["slack", "send-to-slack"]])
      };
    }
    if (/daily|weekday|morning|digest|every/.test(lowerPrompt)) {
      return {
        name: "Morning digest",
        prompt: trimmedPrompt,
        trigger: everyDayTrigger,
        tools: toolsById([
          ["slack", "read-slack-channels"],
          ["google-calendar", "read-calendar-events"]
        ])
      };
    }
    return {
      name: blankAutomationName,
      prompt: trimmedPrompt,
      trigger: null,
      tools: []
    };
  }

  function openPromptAutomation(prompt: string) {
    const trimmedPrompt = prompt.trim();
    if (!trimmedPrompt) {
      openBlankAutomation();
      return;
    }
    const draft = draftFromPrompt(trimmedPrompt);
    automationName = draft.name;
    automationPrompt = draft.prompt;
    automationTrigger = copyTrigger(draft.trigger);
    selectedTools = copySelectedTools(draft.tools);
    automationEnabled = true;
    selectedAutomationId = null;
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
    screenMode = "new";
  }

  function openTemplateAutomation(starter: AutomationStarter) {
    applyStarter(starter);
    screenMode = "new";
  }

  function openExistingAutomation(item: AutomationItem) {
    selectedAutomationId = item.id;
    automationName = item.title;
    automationPrompt = item.prompt ?? item.description;
    automationTrigger = copyTrigger(item.trigger ?? {
      icon: item.icon,
      leading: item.when
    });
    selectedTools = copySelectedTools(item.tools ?? []);
    automationEnabled = item.enabled ?? item.status !== "Paused";
    activeAutomationDetailTab = "settings";
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
    screenMode = "detail";
  }

  function selectTrigger(trigger: AutomationTrigger) {
    automationTrigger = trigger;
    triggerMenuOpen = false;
  }

  function removeTrigger() {
    automationTrigger = null;
    triggerMenuOpen = false;
  }

  function openTriggerEditor() {
    triggerMenuOpen = true;
  }

  function openToolPickerForAdd() {
    editingToolId = null;
    toolSearchQuery = "";
    toolMenuOpen = !toolMenuOpen;
  }

  function openToolPickerForEdit(toolId: string) {
    editingToolId = toolId;
    toolSearchQuery = "";
    toolMenuOpen = true;
  }

  function cancelCreate() {
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
    selectedAutomationId = null;
    screenMode = "home";
  }

  function triggerSummary(trigger: AutomationTrigger | null): string {
    if (!trigger) return "No trigger selected.";
    return [trigger.leading, trigger.target, trigger.actorPrefix, trigger.actor].filter(Boolean).join(" ");
  }

  function saveAutomation() {
    const title = automationName.trim() || blankAutomationName;
    const description = automationPrompt.trim() || "Ready to configure.";
    const nextSequence = savedAutomationSequence + 1;
    savedAutomationSequence = nextSequence;
    savedAutomations = [
      {
        id: `local-${nextSequence}`,
        title,
        description,
        status: "Active",
        source: "Puffer",
        updated: "Just now",
        when: triggerSummary(automationTrigger),
        then: description,
        review: "You can review results before any action is sent.",
        recent: ["Saved locally"],
        icon: automationTrigger?.icon ?? "bolt",
        prompt: description,
        trigger: copyTrigger(automationTrigger),
        tools: copySelectedTools(selectedTools),
        enabled: true,
        owner: "You",
        history: []
      },
      ...savedAutomations
    ];
    activeAutomationLibraryTab = "your";
    homePrompt = "";
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
    selectedAutomationId = null;
    screenMode = "home";
  }

  function saveAutomationDetail() {
    if (!selectedAutomationId) return;
    const title = automationName.trim() || blankAutomationName;
    const description = automationPrompt.trim() || "Ready to configure.";
    savedAutomations = savedAutomations.map((item) =>
      item.id === selectedAutomationId
        ? {
            ...item,
            title,
            description,
            status: automationEnabled ? "Active" : "Paused",
            updated: "Just now",
            when: triggerSummary(automationTrigger),
            then: description,
            icon: automationTrigger?.icon ?? item.icon,
            prompt: description,
            trigger: copyTrigger(automationTrigger),
            tools: copySelectedTools(selectedTools),
            enabled: automationEnabled,
            owner: item.owner ?? "You"
          }
        : item
    );
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
  }

  function runTestAutomation() {
    if (!selectedAutomationId) return;
    const nextRunSequence = savedRunSequence + 1;
    savedRunSequence = nextRunSequence;
    const run: AutomationRun = {
      id: `test-${nextRunSequence}`,
      title: "Test run",
      status: "Waiting for review",
      started: "Just now",
      duration: "-",
      summary: "Puffer is checking the current configuration."
    };
    savedAutomations = savedAutomations.map((item) =>
      item.id === selectedAutomationId
        ? {
            ...item,
            updated: "Just now",
            recent: ["Test run started", ...item.recent.filter((entry) => entry !== "Test run started")],
            history: [run, ...(item.history ?? [])]
          }
        : item
    );
    activeAutomationDetailTab = "history";
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
  }

  function returnToAutomationHome() {
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
    selectedAutomationId = null;
    screenMode = "home";
  }

  function selectAutomationDetailTab(tab: AutomationDetailTab) {
    activeAutomationDetailTab = tab;
    triggerMenuOpen = false;
    toolMenuOpen = false;
    automationActionMenuOpen = false;
    editingToolId = null;
    toolSearchQuery = "";
  }

  function toggleAutomationActionMenu() {
    automationActionMenuOpen = !automationActionMenuOpen;
    triggerMenuOpen = false;
    toolMenuOpen = false;
  }

  function deleteSelectedAutomation() {
    if (!selectedAutomationId) return;
    savedAutomations = savedAutomations.filter((item) => item.id !== selectedAutomationId);
    activeAutomationLibraryTab = "your";
    returnToAutomationHome();
  }

  function closeFloatingMenusFromOutside(event: MouseEvent) {
    if (!toolMenuOpen && !triggerMenuOpen) return;
    const target = event.target;
    const insideToolPicker =
      target instanceof Element &&
      (target.closest(".pf-automation-tool-menu-wrap") || target.closest(".pf-automation-tool-config-row"));
    const insideTriggerPicker =
      target instanceof Element &&
      (target.closest(".pf-automation-trigger-menu-wrap") ||
        target.closest(".pf-automation-trigger-row") ||
        target.closest('[aria-label="Edit trigger"]'));

    if (toolMenuOpen && !insideToolPicker) {
      toolMenuOpen = false;
      editingToolId = null;
      toolSearchQuery = "";
    }
    if (triggerMenuOpen && !insideTriggerPicker) {
      triggerMenuOpen = false;
    }
  }

  function replaceSelectedTool(tool: SelectedAutomationTool) {
    if (editingToolId) {
      selectedTools = selectedTools
        .map((selected) => (selected.id === editingToolId ? tool : selected))
        .filter((selected, index, tools) => tools.findIndex((candidate) => candidate.id === selected.id) === index);
      editingToolId = null;
      toolSearchQuery = "";
      toolMenuOpen = false;
      return;
    }

    const alreadySelected = selectedTools.some((selected) => selected.id === tool.id);
    selectedTools = alreadySelected
      ? selectedTools.filter((selected) => selected.id !== tool.id)
      : [...selectedTools, tool];
  }

  function removeTool(toolId: string) {
    selectedTools = selectedTools.filter((selected) => selected.id !== toolId);
    if (editingToolId === toolId) {
      editingToolId = null;
      toolMenuOpen = false;
    }
  }

  function toolSelected(toolId: string): boolean {
    return selectedTools.some((tool) => tool.id === toolId);
  }

  function toggleToolCapability(app: AutomationApp, capability: AutomationCapability) {
    replaceSelectedTool(selectedToolFrom(app, capability));
  }

  function cycleToolTarget(toolId: string) {
    selectedTools = selectedTools.map((tool) => {
      if (tool.id !== toolId || tool.targetOptions.length === 0) return tool;
      const currentIndex = Math.max(0, tool.targetOptions.findIndex((option) => option === tool.target));
      const nextTarget = tool.targetOptions[(currentIndex + 1) % tool.targetOptions.length];
      return {
        ...tool,
        target: nextTarget
      };
    });
  }

  function selectedToolLabel(tool: SelectedAutomationTool): string {
    if (!tool.targetLabel || !tool.target) return tool.title;
    return `${tool.title} ${tool.targetLabel} ${tool.target}`;
  }

  function capabilityLabel(app: AutomationApp, capability: AutomationCapability): string {
    if (!capability.targetLabel || !capability.defaultTarget) return capability.title;
    return `${capability.title} ${capability.targetLabel} ${capability.defaultTarget}`;
  }

  function toolIdFor(app: AutomationApp, capability: AutomationCapability): string {
    return `${app.id}:${capability.id}`;
  }

  function stopButtonEvent(event: MouseEvent) {
    event.stopPropagation();
  }
</script>

<svelte:window onclick={closeFloatingMenusFromOutside} />

<div class="pf-screen-top">
  <div class="pf-screen-top-left">
    <span class="pf-screen-top-title">Automation</span>
    <span class="pf-screen-top-sub">Set up repeated work as editable drafts.</span>
  </div>
</div>

{#if screenMode === "new"}
  <section class="pf-automation-builder-page" aria-label="New automation page">
    <header class="pf-automation-builder-page-head">
      <div>
        <nav class="pf-automation-breadcrumb" aria-label="Automation path">
          <button type="button" aria-label="Back to automations" onclick={returnToAutomationHome}>Automations</button>
          <Icon name="chevR" size={12} />
          <span>Create New</span>
        </nav>
        <h1 class="pf-automation-sr-only">New automation</h1>
      </div>
      <div class="pf-automation-builder-page-actions">
        <button type="button" class="sc-btn" data-variant="outline" data-size="sm" onclick={cancelCreate}>Cancel</button>
        <button type="button" class="sc-btn" data-variant="default" data-size="sm" onclick={saveAutomation}>Save</button>
      </div>
    </header>

    <div class="pf-automation-builder-page-body">
      <main class="pf-automation-builder-main">
        <section class="pf-automation-builder-field">
          <input
            id="automation-name"
            class="pf-automation-name-input"
            aria-label="Name"
            bind:value={automationName}
          />
        </section>

        <section class="pf-automation-builder-config" aria-label="Automation rule">
          <h2>Triggers</h2>
          <div class="pf-automation-trigger-panel">
            {#if automationTrigger}
              <div class="pf-automation-config-row">
                <button type="button" class="pf-automation-trigger-row" onclick={openTriggerEditor}>
                  <Icon name={automationTrigger.icon} size={13} />
                  <span>{automationTrigger.leading}</span>
                  {#if automationTrigger.target}
                    <span class="pf-automation-token">{automationTrigger.target}</span>
                  {/if}
                  {#if automationTrigger.actorPrefix}
                    <span>{automationTrigger.actorPrefix}</span>
                  {/if}
                  {#if automationTrigger.actor}
                    <span class="pf-automation-token">{automationTrigger.actor}</span>
                  {/if}
                </button>
                <span class="pf-automation-row-actions">
                  <button type="button" class="pf-automation-row-action" aria-label="Edit trigger" onclick={openTriggerEditor}>
                    <Icon name="edit" size={12} />
                  </button>
                  <button type="button" class="pf-automation-row-action" aria-label="Remove trigger" onclick={removeTrigger}>
                    <Icon name="trash" size={12} />
                  </button>
                </span>
              </div>
            {/if}

            <div class="pf-automation-trigger-menu-wrap">
              <button
                type="button"
                class="pf-automation-add-row"
                onclick={() => (triggerMenuOpen = !triggerMenuOpen)}
              >
                <Icon name="plus" size={13} />
                Add Trigger
              </button>

              {#if triggerMenuOpen}
                <div class="pf-automation-trigger-menu" role="menu" aria-label="Add trigger">
                  <label>
                    <Icon name="search" size={12} />
                    <input type="search" placeholder="Search triggers..." />
                  </label>
                  <span>Scheduled</span>
                  <button type="button" role="menuitem" onclick={() => selectTrigger(everyDayTrigger)}>
                    <Icon name="clock" size={12} />
                    Every...
                    <Icon name="chevR" size={11} />
                  </button>
                  <button type="button" role="menuitem" onclick={() => selectTrigger(customScheduleTrigger)}><Icon name="clock" size={12} /> Custom (cron)</button>
                  <span>GitHub / GitLab</span>
                  <button type="button" role="menuitem" onclick={() => selectTrigger(draftOpenedTrigger)}><Icon name="git" size={12} /> Draft opened</button>
                  <button type="button" role="menuitem" onclick={() => selectTrigger(prOpenedTrigger)}>
                    <Icon name="git" size={12} />
                    Pull request...
                    <Icon name="chevR" size={11} />
                  </button>
                  <button type="button" role="menuitem" onclick={() => selectTrigger(commentAddedTrigger)}><Icon name="git" size={12} /> Comment added</button>
                  <button type="button" role="menuitem" onclick={() => selectTrigger(labelChangeTrigger)}><Icon name="git" size={12} /> Label change</button>
                </div>
              {/if}
            </div>
          </div>
        </section>

        <section class="pf-automation-builder-prompt">
          <h2>Instructions</h2>
          <div class="pf-automation-instructions-box">
            <textarea
              id="automation-prompt"
              aria-label="Instructions"
              rows="5"
              bind:value={automationPrompt}
              placeholder="Enter prompt text... (type @ for tools & MCPs, / for skills and commands)"
            ></textarea>
            <button type="button" class="pf-automation-model-row">
              Codex 5.3 High
              <Icon name="chevD" size={12} />
            </button>
          </div>
          <p class="pf-automation-warning">Some tools might not be configured yet</p>
        </section>

        <section class="pf-automation-builder-config">
          <h2>Tools</h2>
          <div class="pf-automation-stack-panel">
            <div class="pf-automation-config-row">
              <button type="button" class="pf-automation-tool-row" aria-label="Memories tool">
                <span class="pf-automation-tool-main"><Icon name="logs" size={13} /> Memories</span>
                <span class="pf-automation-tool-capabilities">
                  <span class="pf-automation-token">Read context</span>
                </span>
              </button>
            </div>
            {#each selectedTools as tool (tool.id)}
              <div class="pf-automation-config-row pf-automation-tool-config-row">
                <button
                  type="button"
                  class="pf-automation-tool-row"
                  aria-label={`${tool.title} tool`}
                  title={selectedToolLabel(tool)}
                  onclick={() => openToolPickerForEdit(tool.id)}
                >
                  <span class="pf-automation-tool-main"><Icon name={tool.icon} size={13} /> {tool.title}</span>
                </button>
                {#if tool.targetLabel && tool.target}
                  <span class="pf-automation-tool-target">
                    <span>{tool.targetLabel}</span>
                    <button
                      type="button"
                      class="pf-automation-target-chip"
                      aria-label={`${tool.title} target`}
                      onclick={(event) => {
                        stopButtonEvent(event);
                        cycleToolTarget(tool.id);
                      }}
                    >
                      {tool.target}
                      <Icon name="chevD" size={10} />
                    </button>
                  </span>
                {/if}
                <span class="pf-automation-row-actions">
                  <button type="button" class="pf-automation-row-action" aria-label={`Edit ${tool.title} tool`} onclick={() => openToolPickerForEdit(tool.id)}>
                    <Icon name="edit" size={12} />
                  </button>
                  <button type="button" class="pf-automation-row-action" aria-label={`Remove ${tool.title} tool`} onclick={() => removeTool(tool.id)}>
                    <Icon name="trash" size={12} />
                  </button>
                </span>
              </div>
            {/each}
            <div class="pf-automation-tool-menu-wrap">
              <button
                type="button"
                class="pf-automation-add-row"
                aria-expanded={toolMenuOpen}
                aria-haspopup="menu"
                onclick={openToolPickerForAdd}
              >
                <Icon name="plus" size={13} />
                Add Tool or MCP
              </button>
              {#if toolMenuOpen}
                <div class="pf-automation-app-menu" role="menu" aria-label="Common apps">
                  <label class="pf-automation-app-search">
                    <Icon name="search" size={12} />
                    <input type="search" placeholder="Search tools and APIs..." bind:value={toolSearchQuery} />
                  </label>
                  <span>Common apps</span>
                  {#each visibleToolApps as app (app.id)}
                    <div class="pf-automation-app-group" role="group" aria-label={`${app.title} API capabilities`}>
                      <div class="pf-automation-app-heading">
                        <Icon name={app.icon} size={13} />
                        <span>
                          <strong>{app.title}</strong>
                          <small>{app.description}</small>
                        </span>
                      </div>
                      <div class="pf-automation-app-capabilities">
                        {#each app.visibleCapabilities as capability}
                          <button
                            type="button"
                            role="menuitemcheckbox"
                            aria-checked={toolSelected(toolIdFor(app, capability))}
                            data-selected={toolSelected(toolIdFor(app, capability))}
                            title={capabilityLabel(app, capability)}
                            onclick={() => toggleToolCapability(app, capability)}
                          >
                            <Icon name={app.icon} size={13} />
                            <span>
                              <strong>{capability.title}</strong>
                              <small>{capability.description}</small>
                            </span>
                            {#if capability.targetLabel && capability.defaultTarget}
                              <span class="pf-automation-app-target-preview">
                                {capability.targetLabel} {capability.defaultTarget}
                              </span>
                            {/if}
                          </button>
                        {/each}
                      </div>
                    </div>
                  {:else}
                    <p class="pf-automation-app-empty">No matching apps.</p>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        </section>

        <section class="pf-automation-builder-config">
          <h2>Cloud Agent Environment</h2>
          <div class="pf-automation-environment-row">
            <div>
              <strong>Use Configured Environment</strong>
              <p>Applies any environment setup and secrets. Disable to start the agent faster.</p>
            </div>
            <button type="button">Manage</button>
            <label class="pf-automation-switch">
              <input type="checkbox" aria-label="Use Configured Environment" checked />
              <span></span>
            </label>
          </div>
        </section>

      </main>
    </div>
  </section>
{:else if screenMode === "detail"}
  <section class="pf-automation-detail-page" aria-label="Automation detail page">
    <header class="pf-automation-detail-page-head">
      <nav class="pf-automation-breadcrumb" aria-label="Automation path">
        <button type="button" aria-label="Back to automations" onclick={returnToAutomationHome}>Automations</button>
        <Icon name="chevR" size={12} />
        <span>{automationName.trim() || blankAutomationName}</span>
      </nav>
      <div class="pf-automation-detail-actions">
        <button type="button" class="sc-btn" data-variant="outline" data-size="sm" onclick={runTestAutomation}>
          <Icon name="test" size={13} />
          <span>Test Run</span>
        </button>
        <button type="button" class="sc-btn" data-variant="default" data-size="sm" onclick={saveAutomationDetail}>Save</button>
        <div class="pf-automation-action-menu-wrap">
          <button
            type="button"
            class="pf-automation-icon-action"
            aria-label="More automation actions"
            aria-haspopup="menu"
            aria-expanded={automationActionMenuOpen}
            onclick={toggleAutomationActionMenu}
          >
            <Icon name="moreH" size={15} />
          </button>
          {#if automationActionMenuOpen}
            <div class="pf-automation-action-menu" role="menu" aria-label="Automation actions">
              <button type="button" role="menuitem" onclick={deleteSelectedAutomation}>
                <Icon name="trash" size={13} />
                Delete
              </button>
            </div>
          {/if}
        </div>
      </div>
    </header>

    <div class="pf-automation-detail-body">
      <main class="pf-automation-detail-main">
        <section class="pf-automation-detail-identity" aria-label="Automation identity">
          <input
            class="pf-automation-detail-name"
            aria-label="Automation name"
            bind:value={automationName}
          />
          <div class="pf-automation-detail-status">
            <label class="pf-automation-switch">
              <input type="checkbox" aria-label="Active" bind:checked={automationEnabled} />
              <span></span>
            </label>
            <span>{automationEnabled ? "Active" : "Paused"} | {selectedAutomation?.owner ?? "You"}</span>
          </div>
        </section>

        <div class="pf-automation-tabs pf-automation-detail-tabs" role="tablist" aria-label="Automation detail">
          <button
            id="automation-settings-tab"
            type="button"
            role="tab"
            aria-selected={activeAutomationDetailTab === "settings"}
            aria-controls="automation-settings-panel"
            tabindex={activeAutomationDetailTab === "settings" ? 0 : -1}
            onclick={() => selectAutomationDetailTab("settings")}
          >
            <span>Settings</span>
          </button>
          <button
            id="automation-history-tab"
            type="button"
            role="tab"
            aria-selected={activeAutomationDetailTab === "history"}
            aria-controls="automation-history-panel"
            tabindex={activeAutomationDetailTab === "history" ? 0 : -1}
            onclick={() => selectAutomationDetailTab("history")}
          >
            <span>Run History</span>
          </button>
        </div>

        {#if activeAutomationDetailTab === "settings"}
          <div
            id="automation-settings-panel"
            class="pf-automation-detail-settings"
            role="tabpanel"
            aria-labelledby="automation-settings-tab"
          >
            <section class="pf-automation-builder-config" aria-label="Automation rule">
              <h2>Triggers</h2>
              <div class="pf-automation-trigger-panel">
                {#if automationTrigger}
                  <div class="pf-automation-config-row">
                    <button type="button" class="pf-automation-trigger-row" onclick={openTriggerEditor}>
                      <Icon name={automationTrigger.icon} size={13} />
                      <span>{automationTrigger.leading}</span>
                      {#if automationTrigger.target}
                        <span class="pf-automation-token">{automationTrigger.target}</span>
                      {/if}
                      {#if automationTrigger.actorPrefix}
                        <span>{automationTrigger.actorPrefix}</span>
                      {/if}
                      {#if automationTrigger.actor}
                        <span class="pf-automation-token">{automationTrigger.actor}</span>
                      {/if}
                    </button>
                    <span class="pf-automation-row-actions">
                      <button type="button" class="pf-automation-row-action" aria-label="Edit trigger" onclick={openTriggerEditor}>
                        <Icon name="edit" size={12} />
                      </button>
                      <button type="button" class="pf-automation-row-action" aria-label="Remove trigger" onclick={removeTrigger}>
                        <Icon name="trash" size={12} />
                      </button>
                    </span>
                  </div>
                {/if}

                <div class="pf-automation-trigger-menu-wrap">
                  <button
                    type="button"
                    class="pf-automation-add-row"
                    onclick={() => (triggerMenuOpen = !triggerMenuOpen)}
                  >
                    <Icon name="plus" size={13} />
                    Add Trigger
                  </button>

                  {#if triggerMenuOpen}
                    <div class="pf-automation-trigger-menu" role="menu" aria-label="Add trigger">
                      <label>
                        <Icon name="search" size={12} />
                        <input type="search" placeholder="Search triggers..." />
                      </label>
                      <span>Scheduled</span>
                      <button type="button" role="menuitem" onclick={() => selectTrigger(everyDayTrigger)}>
                        <Icon name="clock" size={12} />
                        Every...
                        <Icon name="chevR" size={11} />
                      </button>
                      <button type="button" role="menuitem" onclick={() => selectTrigger(customScheduleTrigger)}><Icon name="clock" size={12} /> Custom (cron)</button>
                      <span>GitHub / GitLab</span>
                      <button type="button" role="menuitem" onclick={() => selectTrigger(draftOpenedTrigger)}><Icon name="git" size={12} /> Draft opened</button>
                      <button type="button" role="menuitem" onclick={() => selectTrigger(prOpenedTrigger)}>
                        <Icon name="git" size={12} />
                        Pull request...
                        <Icon name="chevR" size={11} />
                      </button>
                      <button type="button" role="menuitem" onclick={() => selectTrigger(commentAddedTrigger)}><Icon name="git" size={12} /> Comment added</button>
                      <button type="button" role="menuitem" onclick={() => selectTrigger(labelChangeTrigger)}><Icon name="git" size={12} /> Label change</button>
                    </div>
                  {/if}
                </div>
              </div>
            </section>

            <section class="pf-automation-builder-prompt">
              <h2>Instructions</h2>
              <div class="pf-automation-instructions-box">
                <textarea
                  aria-label="Instructions"
                  rows="5"
                  bind:value={automationPrompt}
                  placeholder="Enter prompt text... (type @ for tools & MCPs, / for skills and commands)"
                ></textarea>
                <button type="button" class="pf-automation-model-row">
                  Codex 5.3 High
                  <Icon name="chevD" size={12} />
                </button>
              </div>
            </section>

            <section class="pf-automation-builder-config">
              <h2>Tools</h2>
              <div class="pf-automation-stack-panel">
                <div class="pf-automation-config-row">
                  <button type="button" class="pf-automation-tool-row" aria-label="Memories tool">
                    <span class="pf-automation-tool-main"><Icon name="logs" size={13} /> Memories</span>
                    <span class="pf-automation-tool-capabilities">
                      <span class="pf-automation-token">Read context</span>
                    </span>
                  </button>
                </div>
                {#each selectedTools as tool (tool.id)}
                  <div class="pf-automation-config-row pf-automation-tool-config-row">
                    <button
                      type="button"
                      class="pf-automation-tool-row"
                      aria-label={`${tool.title} tool`}
                      title={selectedToolLabel(tool)}
                      onclick={() => openToolPickerForEdit(tool.id)}
                    >
                      <span class="pf-automation-tool-main"><Icon name={tool.icon} size={13} /> {tool.title}</span>
                    </button>
                    {#if tool.targetLabel && tool.target}
                      <span class="pf-automation-tool-target">
                        <span>{tool.targetLabel}</span>
                        <button
                          type="button"
                          class="pf-automation-target-chip"
                          aria-label={`${tool.title} target`}
                          onclick={(event) => {
                            stopButtonEvent(event);
                            cycleToolTarget(tool.id);
                          }}
                        >
                          {tool.target}
                          <Icon name="chevD" size={10} />
                        </button>
                      </span>
                    {/if}
                    <span class="pf-automation-row-actions">
                      <button type="button" class="pf-automation-row-action" aria-label={`Edit ${tool.title} tool`} onclick={() => openToolPickerForEdit(tool.id)}>
                        <Icon name="edit" size={12} />
                      </button>
                      <button type="button" class="pf-automation-row-action" aria-label={`Remove ${tool.title} tool`} onclick={() => removeTool(tool.id)}>
                        <Icon name="trash" size={12} />
                      </button>
                    </span>
                  </div>
                {/each}
                <div class="pf-automation-tool-menu-wrap">
                  <button
                    type="button"
                    class="pf-automation-add-row"
                    aria-expanded={toolMenuOpen}
                    aria-haspopup="menu"
                    onclick={openToolPickerForAdd}
                  >
                    <Icon name="plus" size={13} />
                    Add Tool or MCP
                  </button>
                  {#if toolMenuOpen}
                    <div class="pf-automation-app-menu" role="menu" aria-label="Common apps">
                      <label class="pf-automation-app-search">
                        <Icon name="search" size={12} />
                        <input type="search" placeholder="Search tools and APIs..." bind:value={toolSearchQuery} />
                      </label>
                      <span>Common apps</span>
                      {#each visibleToolApps as app (app.id)}
                        <div class="pf-automation-app-group" role="group" aria-label={`${app.title} API capabilities`}>
                          <div class="pf-automation-app-heading">
                            <Icon name={app.icon} size={13} />
                            <span>
                              <strong>{app.title}</strong>
                              <small>{app.description}</small>
                            </span>
                          </div>
                          <div class="pf-automation-app-capabilities">
                            {#each app.visibleCapabilities as capability}
                              <button
                                type="button"
                                role="menuitemcheckbox"
                                aria-checked={toolSelected(toolIdFor(app, capability))}
                                data-selected={toolSelected(toolIdFor(app, capability))}
                                title={capabilityLabel(app, capability)}
                                onclick={() => toggleToolCapability(app, capability)}
                              >
                                <Icon name={app.icon} size={13} />
                                <span>
                                  <strong>{capability.title}</strong>
                                  <small>{capability.description}</small>
                                </span>
                                {#if capability.targetLabel && capability.defaultTarget}
                                  <span class="pf-automation-app-target-preview">
                                    {capability.targetLabel} {capability.defaultTarget}
                                  </span>
                                {/if}
                              </button>
                            {/each}
                          </div>
                        </div>
                      {:else}
                        <p class="pf-automation-app-empty">No matching apps.</p>
                      {/each}
                    </div>
                  {/if}
                </div>
              </div>
            </section>
          </div>
        {:else}
          <div
            id="automation-history-panel"
            class="pf-automation-history-panel"
            role="tabpanel"
            aria-labelledby="automation-history-tab"
          >
            {#if selectedAutomation && selectedAutomation.history && selectedAutomation.history.length > 0}
              <ul class="pf-automation-history-list" aria-label="Run history">
                {#each selectedAutomation.history as run (run.id)}
                  <li>
                    <span class="pf-automation-history-icon"><Icon name="test" size={13} /></span>
                    <span class="pf-automation-history-main">
                      <strong>{run.title}</strong>
                      <small>{run.summary}</small>
                    </span>
                    <span class="pf-automation-history-status">{run.status}</span>
                    <span class="pf-automation-history-meta">{run.started}</span>
                    <span class="pf-automation-history-meta">{run.duration}</span>
                  </li>
                {/each}
              </ul>
            {:else}
              <div class="pf-automation-history-empty">
                <span><Icon name="clock" size={14} /></span>
                <strong>No runs yet</strong>
              </div>
            {/if}
          </div>
        {/if}
      </main>
    </div>
  </section>
{:else}
  <section class="pf-automation-home" aria-label="Automation home">
    <section class="pf-automation-compose" aria-labelledby="automation-compose-title">
      <div class="pf-automation-compose-copy">
        <h1 id="automation-compose-title">Create an automation</h1>
        <p>Create an automation using natural language.</p>
      </div>

      <div class="pf-composer-wrap">
        <div class="pf-composer" role="group" aria-label="Message composer">
          <input
            class="pf-attachment-input"
            type="file"
            multiple
            tabindex="-1"
            data-testid="composer-file-input"
          />
          <textarea
            bind:value={homePrompt}
            placeholder="Tell Puffer what to automate, e.g. when a PR opens, prepare a review draft..."
          ></textarea>
          <div class="pf-composer-foot">
            <div class="pf-attachment-menu">
              <button
                type="button"
                class="pf-add-content-btn"
                aria-label="Add content"
                aria-haspopup="menu"
                aria-expanded="false"
                title="Add content"
              >
                <Icon name="plus" size={15} />
              </button>
            </div>
            <div class="picker">
              <button
                type="button"
                class="trigger"
                aria-haspopup="listbox"
                aria-expanded="false"
                title="OpenAI · gpt-5.5"
              >
                <Icon name="sparkles" size={11} color="var(--muted-foreground)" />
                <span class="model">gpt-5.5</span>
                <span class="provider">OpenAI</span>
                <Icon name="chevD" size={10} color="var(--muted-foreground)" />
              </button>
            </div>
            <label class="pf-toggle-chip" title="Fast mode">
              <input type="checkbox" />
              <Icon name="bolt" size={11} />
              <span>Fast</span>
            </label>
            <label class="pf-select-chip" title="Thinking level">
              <Icon name="cpu" size={11} />
              <select aria-label="Thinking level">
                <option value="">Default</option>
              </select>
            </label>
            <label class="pf-select-chip" title="Codex permissions">
              <Icon name="shield" size={11} />
              <select aria-label="Codex permissions">
                <option value="workspace-write">Workspace</option>
              </select>
            </label>
            <span class="spacer"></span>
            <span class="pf-composer-hint">⏎ to send · ⇧⏎ for newline</span>
            <button type="button" class="pf-send-btn" onclick={() => openPromptAutomation(homePrompt)} aria-label="Send">
              <Icon name="arrowUp" size={15} />
            </button>
          </div>
        </div>
      </div>
    </section>

    <section class="pf-automations-section" aria-label="Automation library">
      <div class="pf-automation-library-toolbar">
        <div class="pf-automation-tabs" role="tablist" aria-label="Automation library">
          <button
            id="your-automations-tab"
            type="button"
            role="tab"
            aria-selected={activeAutomationLibraryTab === "your"}
            aria-controls="your-automations-panel"
            tabindex={activeAutomationLibraryTab === "your" ? 0 : -1}
            onclick={() => (activeAutomationLibraryTab = "your")}
          >
            <span>Your automations</span>
            <small>{userAutomations.length}</small>
          </button>
          <button
            id="templates-tab"
            type="button"
            role="tab"
            aria-selected={activeAutomationLibraryTab === "templates"}
            aria-controls="templates-panel"
            tabindex={activeAutomationLibraryTab === "templates" ? 0 : -1}
            onclick={() => (activeAutomationLibraryTab = "templates")}
          >
            <span>Template Library</span>
            <small>{automationTemplates.length}</small>
          </button>
        </div>

        <button
          type="button"
          class="sc-btn pf-automation-new-button"
          data-variant="default"
          data-size="sm"
          onclick={() => openBlankAutomation()}
        >
          <Icon name="plus" size={13} />
          <span>new</span>
        </button>
      </div>

      <div class="pf-automation-library">
        {#if activeAutomationLibraryTab === "your"}
          <div
            id="your-automations-panel"
            class="pf-automation-group"
            role="tabpanel"
            aria-labelledby="your-automations-tab"
          >
            {#if userAutomations.length > 0}
              <ul class="pf-automation-grid" aria-label="Your automations">
                {#each userAutomations as item (item.id)}
                  <li>
                    <button type="button" class="pf-automation-card" onclick={() => openExistingAutomation(item)}>
                      <span class="pf-automation-row-icon"><Icon name={item.icon} size={14} /></span>
                      <span class="pf-automation-card-main">
                        <strong>{item.title}</strong>
                        <small>{item.description}</small>
                      </span>
                      <span class="pf-automation-card-meta">{item.status}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            {:else}
              <div class="pf-automation-empty" aria-label="Your automations empty state">
                <span class="pf-automation-empty-icon"><Icon name="bolt" size={16} /></span>
                <strong>No automations yet</strong>
                <p>创建你的第一个automation，处理重复的工作流</p>
                <button type="button" class="sc-btn" data-variant="outline" data-size="sm" onclick={() => openBlankAutomation()}>
                  <Icon name="plus" size={13} />
                  <span>create automation</span>
                </button>
              </div>
            {/if}
          </div>
        {:else}
          <div
            id="templates-panel"
            class="pf-automation-group"
            role="tabpanel"
            aria-labelledby="templates-tab"
          >
            <ul class="pf-automation-grid" aria-label="Template Library">
              {#each automationTemplates as starter (starter.id)}
                <li>
                  <button type="button" class="pf-automation-card" onclick={() => openTemplateAutomation(starter)}>
                    <span class="pf-automation-row-icon"><Icon name={starter.icon} size={14} /></span>
                    <span class="pf-automation-card-main">
                      <strong>{starter.title}</strong>
                      <small>{starter.description}</small>
                    </span>
                    <span class="pf-automation-card-meta">Template</span>
                  </button>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    </section>
  </section>
{/if}

<style>
  .pf-automation-home,
  .pf-automation-builder-page,
  .pf-automation-detail-page {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 16px;
    overflow: auto;
    background: var(--background);
  }

  .pf-automation-home {
    gap: 18px;
  }

  .pf-automation-compose,
  .pf-automations-section,
  .pf-automation-builder-page-head {
    width: min(100%, 980px);
    margin: 0 auto;
  }

  .pf-automation-compose {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding-top: 18px;
  }

  .pf-automation-compose-copy {
    text-align: center;
  }

  .pf-automation-compose h1 {
    margin: 0 0 6px;
    color: var(--foreground);
    font-size: 22px;
    letter-spacing: 0;
  }

  .pf-automation-compose p {
    margin: 0;
    color: var(--muted-foreground);
    font-size: 13px;
    line-height: 19px;
  }

  .pf-automation-name-input:focus,
  .pf-automation-detail-name:focus,
  .pf-automation-instructions-box:focus-within,
  .pf-automation-trigger-row:focus-visible,
  .pf-automation-tool-row:focus-visible,
  .pf-automation-add-row:focus-visible,
  .pf-automation-model-row:focus-visible,
  .pf-automation-card:focus-visible,
  .pf-automation-icon-action:focus-visible {
    border-color: color-mix(in oklab, var(--puffer-accent) 55%, var(--border));
    box-shadow: 0 0 0 2px color-mix(in oklab, var(--puffer-accent) 14%, transparent);
  }

  .pf-composer-wrap {
    width: min(100%, 980px);
    border-top: 0;
    background: transparent;
    padding: 0;
    margin-bottom: 14px;
    flex-shrink: 0;
  }

  .pf-composer {
    max-width: 820px;
    margin: 0 auto;
    position: relative;
  }

  .pf-composer textarea {
    overflow-y: hidden;
  }

  .pf-attachment-input {
    display: none;
  }

  .pf-composer-foot .picker {
    min-width: 0;
  }

  .pf-composer-foot .trigger {
    height: 28px;
    max-width: 220px;
    background: var(--background);
  }

  .picker {
    position: relative;
    display: inline-block;
    flex-shrink: 0;
  }

  .trigger {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--background);
    color: var(--foreground);
    cursor: pointer;
    font: inherit;
    font-size: 11.5px;
    line-height: 1.2;
    max-width: 240px;
    transition: background 120ms, border-color 120ms;
  }

  .trigger:hover {
    background: color-mix(in oklab, var(--background) 92%, var(--muted));
    border-color: color-mix(in oklab, var(--accent) 35%, var(--border));
  }

  .trigger .model {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-weight: 500;
  }

  .trigger .provider {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border-left: 1px solid var(--border);
    padding-left: 6px;
    color: var(--muted-foreground);
    font-size: 10.5px;
  }

  .pf-toggle-chip,
  .pf-add-content-btn,
  .pf-select-chip {
    height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--background);
    color: var(--muted-foreground);
    font-size: 11.5px;
    line-height: 1;
    white-space: nowrap;
  }

  .pf-attachment-menu {
    position: relative;
    flex: 0 0 auto;
  }

  .pf-add-content-btn {
    width: 28px;
    justify-content: center;
    padding: 0;
    cursor: pointer;
  }

  .pf-add-content-btn:hover {
    color: var(--foreground);
    background: var(--accent);
  }

  .pf-toggle-chip {
    cursor: pointer;
  }

  .pf-toggle-chip input {
    width: 12px;
    height: 12px;
    margin: 0;
    accent-color: var(--accent-foreground);
  }

  .pf-toggle-chip:has(input:checked) {
    border-color: color-mix(in oklab, var(--accent-foreground) 26%, var(--border));
    background: color-mix(in oklab, var(--accent) 70%, var(--background));
    color: var(--foreground);
  }

  .pf-select-chip select {
    border: 0;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 11.5px;
    padding: 0;
    outline: none;
  }

  .pf-select-chip:focus-within {
    border-color: color-mix(in oklab, var(--accent-foreground) 30%, var(--border));
  }

  .pf-composer-hint {
    color: var(--muted-foreground);
    font-family: var(--font-sans);
    font-size: var(--pf-chat-meta-size);
  }

  .pf-automations-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .pf-automation-library-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .pf-automation-tabs {
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--muted);
    padding: 3px;
  }

  .pf-automation-tabs button {
    min-width: 0;
    min-height: 30px;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--muted-foreground);
    font: inherit;
    font-size: 12px;
    line-height: 16px;
    padding: 5px 10px;
    cursor: pointer;
  }

  .pf-automation-tabs button:hover {
    color: var(--foreground);
    background: var(--pf-selected-bg-hover);
  }

  .pf-automation-tabs button[aria-selected="true"] {
    color: var(--foreground);
    background: var(--background);
    box-shadow: var(--shadow-xs);
  }

  .pf-automation-tabs span {
    white-space: nowrap;
  }

  .pf-automation-tabs small {
    color: var(--muted-foreground);
    font-family: var(--font-mono);
    font-size: 10.5px;
    font-weight: 650;
  }

  .pf-automation-new-button {
    flex: 0 0 auto;
    gap: 6px;
  }

  .pf-automation-library {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .pf-automation-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .pf-automation-card-meta {
    color: var(--muted-foreground);
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }

  .pf-automation-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .pf-automation-empty {
    min-height: 190px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 9px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in oklab, var(--background) 98%, var(--muted));
    color: var(--foreground);
    padding: 24px;
    text-align: center;
  }

  .pf-automation-empty-icon {
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid color-mix(in oklab, var(--puffer-accent) 24%, var(--border));
    border-radius: 8px;
    color: var(--puffer-accent);
    background: color-mix(in oklab, var(--puffer-accent) 8%, var(--background));
  }

  .pf-automation-empty strong {
    color: var(--foreground);
    font-size: 13px;
    font-weight: 650;
  }

  .pf-automation-empty p {
    max-width: 300px;
    margin: -2px 0 3px;
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 17px;
  }

  .pf-automation-empty .sc-btn {
    gap: 6px;
  }

  .pf-automation-card {
    width: 100%;
    min-height: 104px;
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr);
    grid-template-rows: auto 1fr;
    gap: 5px 9px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in oklab, var(--background) 98%, var(--muted));
    color: var(--foreground);
    padding: 12px;
    text-align: left;
    cursor: pointer;
  }

  .pf-automation-card:hover {
    border-color: color-mix(in oklab, var(--puffer-accent) 28%, var(--border));
    background: var(--pf-selected-bg-hover);
  }

  .pf-automation-row-icon {
    width: 30px;
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid color-mix(in oklab, var(--puffer-accent) 24%, var(--border));
    border-radius: 7px;
    color: var(--puffer-accent);
    background: color-mix(in oklab, var(--puffer-accent) 8%, var(--background));
    flex-shrink: 0;
  }

  .pf-automation-card .pf-automation-row-icon {
    grid-row: 1 / span 2;
  }

  .pf-automation-card-main {
    min-width: 0;
  }

  .pf-automation-card-main {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .pf-automation-card-main strong {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--foreground);
    font-size: 13px;
    font-weight: 650;
  }

  .pf-automation-card-main small {
    min-width: 0;
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 17px;
  }

  .pf-automation-card-meta {
    grid-column: 2;
  }

  .pf-automation-sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .pf-automation-builder-page,
  .pf-automation-detail-page {
    gap: 14px;
    padding-top: 10px;
  }

  .pf-automation-builder-page-head {
    width: min(100%, 760px);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-shrink: 0;
  }

  .pf-automation-breadcrumb {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 18px;
  }

  .pf-automation-breadcrumb button {
    border: 0;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    padding: 0;
    cursor: pointer;
  }

  .pf-automation-breadcrumb button:hover {
    color: var(--puffer-accent);
  }

  .pf-automation-builder-page-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .pf-automation-builder-page-body {
    width: min(100%, 760px);
    margin: 0 auto;
  }

  .pf-automation-detail-page-head,
  .pf-automation-detail-body {
    width: min(100%, 820px);
    margin: 0 auto;
  }

  .pf-automation-detail-page-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-shrink: 0;
  }

  .pf-automation-detail-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .pf-automation-detail-actions .sc-btn {
    gap: 6px;
  }

  .pf-automation-action-menu-wrap {
    position: relative;
    display: inline-flex;
  }

  .pf-automation-icon-action {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--background);
    color: var(--muted-foreground);
    padding: 0;
    cursor: pointer;
  }

  .pf-automation-icon-action:hover {
    background: var(--pf-selected-bg-hover);
    color: var(--foreground);
  }

  .pf-automation-action-menu {
    position: absolute;
    right: 0;
    top: calc(100% + 5px);
    z-index: 20;
    min-width: 138px;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--popover, var(--background));
    box-shadow: var(--shadow-sm);
    padding: 5px;
  }

  .pf-automation-action-menu button {
    min-height: 30px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 12px;
    line-height: 17px;
    padding: 6px 8px;
    text-align: left;
    cursor: pointer;
  }

  .pf-automation-action-menu button:hover,
  .pf-automation-action-menu button:focus-visible {
    background: var(--pf-selected-bg-hover);
    color: var(--pf-run-failed, var(--foreground));
    outline: none;
  }

  .pf-automation-builder-main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .pf-automation-detail-main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .pf-automation-detail-identity {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 2px;
  }

  .pf-automation-detail-name {
    width: 100%;
    min-width: 0;
    border: 1px solid transparent;
    border-radius: 5px;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 22px;
    font-weight: 650;
    line-height: 30px;
    letter-spacing: 0;
    padding: 2px 4px;
    outline: none;
  }

  .pf-automation-detail-name:hover {
    border-color: var(--border);
    background: color-mix(in oklab, var(--background) 98%, var(--muted));
  }

  .pf-automation-detail-status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 17px;
  }

  .pf-automation-detail-tabs {
    width: fit-content;
  }

  .pf-automation-detail-settings {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .pf-automation-builder-config {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .pf-automation-builder-prompt {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .pf-automation-builder-config h2,
  .pf-automation-builder-prompt h2 {
    margin: 0;
    color: var(--foreground);
    font-size: 12px;
    font-weight: 550;
    letter-spacing: 0;
  }

  .pf-automation-name-input {
    width: 100%;
    height: 32px;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--background);
    color: var(--foreground);
    font: inherit;
    font-size: 15px;
    line-height: 20px;
    padding: 4px 8px;
    outline: none;
  }

  .pf-automation-trigger-panel,
  .pf-automation-stack-panel,
  .pf-automation-environment-row,
  .pf-automation-instructions-box {
    border: 1px solid var(--border);
    border-radius: 5px;
    background: color-mix(in oklab, var(--background) 98%, var(--muted));
  }

  .pf-automation-trigger-panel,
  .pf-automation-stack-panel {
    display: flex;
    flex-direction: column;
  }

  .pf-automation-config-row {
    width: 100%;
    min-width: 0;
    display: flex;
    align-items: stretch;
  }

  .pf-automation-trigger-row,
  .pf-automation-add-row,
  .pf-automation-tool-row {
    width: 100%;
    min-height: 32px;
    display: flex;
    align-items: center;
    gap: 7px;
    border: 0;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 12px;
    line-height: 17px;
    padding: 7px 10px;
    text-align: left;
  }

  .pf-automation-config-row > .pf-automation-trigger-row,
  .pf-automation-config-row > .pf-automation-tool-row {
    flex: 1 1 auto;
    width: auto;
    min-width: 0;
  }

  .pf-automation-trigger-row,
  .pf-automation-tool-row,
  .pf-automation-add-row {
    cursor: pointer;
  }

  .pf-automation-add-row {
    color: var(--muted-foreground);
  }

  .pf-automation-trigger-row:hover,
  .pf-automation-tool-row:hover,
  .pf-automation-add-row:hover {
    background: var(--pf-selected-bg-hover);
    color: var(--foreground);
  }

  .pf-automation-token {
    display: inline-flex;
    align-items: center;
    min-height: 20px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--background);
    color: var(--foreground);
    padding: 1px 6px;
    font-size: 11px;
  }

  .pf-automation-trigger-menu-wrap {
    position: relative;
    border-top: 1px solid var(--border);
  }

  .pf-automation-trigger-menu {
    position: absolute;
    left: 8px;
    top: calc(100% + 4px);
    z-index: 20;
    width: 236px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--popover, var(--background));
    box-shadow: var(--shadow-sm);
    padding: 6px;
  }

  .pf-automation-trigger-menu label {
    height: 28px;
    display: flex;
    align-items: center;
    gap: 7px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: color-mix(in oklab, var(--background) 96%, var(--muted));
    color: var(--muted-foreground);
    padding: 0 7px;
  }

  .pf-automation-trigger-menu input {
    min-width: 0;
    width: 100%;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 12px;
  }

  .pf-automation-trigger-menu > span {
    margin: 7px 4px 3px;
    color: var(--muted-foreground);
    font-size: 10px;
    font-weight: 650;
  }

  .pf-automation-trigger-menu button {
    min-height: 27px;
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    align-items: center;
    gap: 7px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 12px;
    line-height: 16px;
    padding: 4px 6px;
    text-align: left;
    cursor: pointer;
  }

  .pf-automation-trigger-menu button:hover {
    background: var(--pf-selected-bg-hover);
  }

  .pf-automation-instructions-box {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .pf-automation-instructions-box textarea {
    width: 100%;
    min-height: 118px;
    resize: vertical;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 12px;
    line-height: 18px;
    padding: 11px 10px;
  }

  .pf-automation-model-row {
    width: 100%;
    min-height: 28px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 0;
    border-top: 1px solid var(--border);
    background: transparent;
    color: var(--muted-foreground);
    font: inherit;
    font-size: 12px;
    line-height: 18px;
    padding: 5px 10px;
    cursor: pointer;
  }

  .pf-automation-model-row:hover {
    color: var(--foreground);
  }

  .pf-automation-warning {
    margin: 0;
    color: oklch(0.62 0.12 75);
    font-size: 11px;
    line-height: 16px;
  }

  .pf-automation-tool-row {
    justify-content: flex-start;
    gap: 7px;
  }

  .pf-automation-stack-panel .pf-automation-config-row + .pf-automation-config-row,
  .pf-automation-tool-menu-wrap {
    border-top: 1px solid var(--border);
  }

  .pf-automation-tool-menu-wrap {
    position: relative;
  }

  .pf-automation-tool-menu-wrap .pf-automation-add-row {
    border-top: 0;
  }

  .pf-automation-tool-main {
    min-width: 0;
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 7px;
  }

  .pf-automation-tool-config-row {
    position: relative;
    align-items: center;
  }

  .pf-automation-tool-config-row > .pf-automation-tool-row {
    flex: 0 1 auto;
    width: auto;
    padding-right: 6px;
  }

  .pf-automation-row-actions {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
    padding: 4px 7px 4px 0;
  }

  .pf-automation-row-action {
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--muted-foreground);
    padding: 0;
    cursor: pointer;
  }

  .pf-automation-tool-target {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--foreground);
    font-size: 12px;
    line-height: 17px;
  }

  .pf-automation-target-chip {
    height: 24px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 0;
    border-radius: 6px;
    background: color-mix(in oklab, var(--muted) 58%, var(--background));
    color: var(--foreground);
    font: inherit;
    font-size: 11px;
    line-height: 16px;
    padding: 0 8px;
    cursor: pointer;
  }

  .pf-automation-target-chip:hover,
  .pf-automation-target-chip:focus-visible {
    background: var(--pf-selected-bg-hover);
  }

  .pf-automation-row-action:hover,
  .pf-automation-row-action:focus-visible {
    background: var(--pf-selected-bg-hover);
    color: var(--foreground);
  }

  .pf-automation-environment-row > button {
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 11px;
    line-height: 16px;
    padding: 3px 6px;
    cursor: pointer;
  }

  .pf-automation-environment-row > button:hover {
    background: var(--pf-selected-bg-hover);
  }

  .pf-automation-app-menu {
    position: absolute;
    left: 8px;
    top: calc(100% + 4px);
    z-index: 20;
    width: min(420px, calc(100vw - 48px));
    max-height: min(560px, calc(100vh - 220px));
    display: flex;
    flex-direction: column;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--popover, var(--background));
    box-shadow: var(--shadow-sm);
    padding: 6px;
    overflow-y: auto;
  }

  .pf-automation-app-search {
    height: 28px;
    display: flex;
    align-items: center;
    gap: 7px;
    border: 1px solid var(--border);
    border-radius: 5px;
    background: color-mix(in oklab, var(--background) 96%, var(--muted));
    color: var(--muted-foreground);
    padding: 0 7px;
  }

  .pf-automation-app-search input {
    min-width: 0;
    width: 100%;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    font-size: 12px;
  }

  .pf-automation-app-menu > span {
    margin: 7px 4px 3px;
    color: var(--muted-foreground);
    font-size: 10px;
    font-weight: 650;
  }

  .pf-automation-app-group {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding-bottom: 4px;
  }

  .pf-automation-app-group + .pf-automation-app-group {
    border-top: 1px solid var(--border);
    padding-top: 5px;
  }

  .pf-automation-app-heading,
  .pf-automation-app-menu button {
    min-height: 44px;
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--foreground);
    font: inherit;
    padding: 6px;
    text-align: left;
  }

  .pf-automation-app-heading {
    color: var(--muted-foreground);
    padding: 5px 6px 2px;
  }

  .pf-automation-app-menu button {
    cursor: pointer;
  }

  .pf-automation-app-menu button:hover,
  .pf-automation-app-menu button[data-selected="true"] {
    background: var(--pf-selected-bg-hover);
  }

  .pf-automation-app-heading > span,
  .pf-automation-app-menu button > span {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .pf-automation-app-menu strong {
    color: var(--foreground);
    font-size: 12px;
    line-height: 16px;
    font-weight: 600;
  }

  .pf-automation-app-menu small {
    min-width: 0;
    color: var(--muted-foreground);
    font-size: 11px;
    line-height: 15px;
  }

  .pf-automation-app-capabilities {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 0 0 2px 28px;
  }

  .pf-automation-app-capabilities button {
    min-height: 36px;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    padding: 5px 6px;
  }

  .pf-automation-app-capabilities button[data-selected="true"] {
    box-shadow: inset 0 0 0 1px color-mix(in oklab, var(--puffer-accent) 34%, transparent);
  }

  .pf-automation-app-target-preview {
    min-width: 0;
    max-width: 140px;
    display: inline-flex;
    align-items: center;
    border-radius: 6px;
    background: color-mix(in oklab, var(--muted) 56%, var(--background));
    color: var(--muted-foreground);
    font-size: 10.5px;
    line-height: 15px;
    padding: 2px 6px;
  }

  .pf-automation-app-capabilities span,
  .pf-automation-app-target-preview {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pf-automation-app-empty {
    margin: 4px;
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 17px;
  }

  .pf-automation-environment-row {
    min-height: 52px;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 10px;
    padding: 9px 10px;
  }

  .pf-automation-environment-row strong {
    display: block;
    color: var(--foreground);
    font-size: 12px;
    line-height: 17px;
    font-weight: 600;
  }

  .pf-automation-environment-row p {
    margin: 1px 0 0;
    color: var(--muted-foreground);
    font-size: 11px;
    line-height: 15px;
  }

  .pf-automation-history-panel {
    min-height: 220px;
  }

  .pf-automation-history-list {
    display: flex;
    flex-direction: column;
    gap: 0;
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: color-mix(in oklab, var(--background) 98%, var(--muted));
    overflow: hidden;
  }

  .pf-automation-history-list li {
    min-width: 0;
    display: grid;
    grid-template-columns: 26px minmax(0, 1fr) auto auto auto;
    align-items: center;
    gap: 10px;
    padding: 10px;
  }

  .pf-automation-history-list li + li {
    border-top: 1px solid var(--border);
  }

  .pf-automation-history-icon {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid color-mix(in oklab, var(--puffer-accent) 24%, var(--border));
    border-radius: 6px;
    color: var(--puffer-accent);
    background: color-mix(in oklab, var(--puffer-accent) 8%, var(--background));
  }

  .pf-automation-history-main {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .pf-automation-history-main strong {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--foreground);
    font-size: 12px;
    line-height: 17px;
    font-weight: 650;
  }

  .pf-automation-history-main small {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted-foreground);
    font-size: 11px;
    line-height: 15px;
  }

  .pf-automation-history-status,
  .pf-automation-history-meta {
    color: var(--muted-foreground);
    font-size: 11px;
    line-height: 15px;
    white-space: nowrap;
  }

  .pf-automation-history-status {
    color: var(--foreground);
  }

  .pf-automation-history-empty {
    min-height: 190px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: color-mix(in oklab, var(--background) 98%, var(--muted));
    color: var(--muted-foreground);
  }

  .pf-automation-history-empty span {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--background);
  }

  .pf-automation-history-empty strong {
    color: var(--foreground);
    font-size: 12px;
    line-height: 17px;
    font-weight: 650;
  }

  .pf-automation-switch {
    position: relative;
    width: 28px;
    height: 16px;
    flex: 0 0 auto;
  }

  .pf-automation-switch input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .pf-automation-switch span {
    position: absolute;
    inset: 0;
    border-radius: 999px;
    background: var(--muted);
    border: 1px solid var(--border);
    transition: background 120ms, border-color 120ms;
  }

  .pf-automation-switch span::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 10px;
    height: 10px;
    border-radius: 999px;
    background: var(--background);
    box-shadow: var(--shadow-xs);
    transition: transform 120ms;
  }

  .pf-automation-switch input:checked + span {
    border-color: color-mix(in oklab, var(--puffer-accent) 35%, var(--border));
    background: color-mix(in oklab, var(--puffer-accent) 72%, var(--background));
  }

  .pf-automation-switch input:checked + span::after {
    transform: translateX(12px);
  }

  @media (max-width: 640px) {
    .pf-automation-home,
    .pf-automation-builder-page,
    .pf-automation-detail-page {
      padding: 12px;
    }

    .pf-automation-builder-page-head,
    .pf-automation-detail-page-head {
      align-items: flex-start;
      flex-direction: column;
    }

    .pf-automation-builder-page-actions,
    .pf-automation-detail-actions {
      flex-wrap: wrap;
    }

    .pf-automation-library-toolbar {
      align-items: stretch;
      flex-direction: column;
    }

    .pf-automation-tabs {
      width: 100%;
    }

    .pf-automation-tabs button {
      flex: 1 1 0;
      justify-content: center;
    }

    .pf-automation-new-button {
      justify-content: center;
      width: 100%;
    }

    .pf-automation-grid {
      grid-template-columns: 1fr;
    }

    .pf-automation-environment-row {
      grid-template-columns: 1fr;
      gap: 8px;
    }

    .pf-automation-history-list li {
      grid-template-columns: 26px minmax(0, 1fr);
      align-items: flex-start;
    }

    .pf-automation-history-status,
    .pf-automation-history-meta {
      grid-column: 2;
    }

  }
</style>
