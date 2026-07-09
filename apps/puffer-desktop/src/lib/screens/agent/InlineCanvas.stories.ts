import type { Meta, StoryObj } from "@storybook/svelte-vite";
import InlineCanvas from "./InlineCanvas.svelte";
import StoryFrame from "../../storybook/StoryFrame.svelte";

// The two canonical compositions from resources/canvas/components.md — a
// findings page with action entry points, and a parameter-intake form.

const reviewSpec = {
  title: "Audit · payment flow",
  meta: ["3 files", "+92 −14"],
  summary: "One high-risk issue; fix before release.",
  body: [
    {
      type: "metrics",
      items: [
        { value: "1", label: "high", tone: "high" },
        { value: "2", label: "info" }
      ]
    },
    {
      type: "section",
      title: "Findings",
      children: [
        {
          type: "finding",
          severity: "high",
          title: "Retry loop double-charges on timeout",
          locations: [{ path: "src/pay.rs", line: 210 }],
          body: "The retry wrapper re-invokes charge() without an idempotency key.",
          evidence: "- charge(order)\n+ charge_idempotent(order, key)",
          actions: [
            { id: "fix-retry", label: "Suggest change", intent: "fix" },
            { id: "explain-retry", label: "Explain", intent: "explain" }
          ]
        }
      ]
    },
    {
      type: "section",
      title: "Scope the fix",
      children: [
        {
          type: "singleSelect",
          id: "fix-scope",
          label: "Apply to",
          options: [
            { id: "retry", label: "Retry path only" },
            { id: "all", label: "All charge sites" }
          ]
        }
      ]
    }
  ]
};

const intakeSpec = {
  title: "Release plan · confirm parameters",
  summary: "Adjust and Submit; the agent proceeds with these values.",
  body: [
    {
      type: "section",
      title: "Scope",
      children: [
        { type: "toggle", id: "include-migrations", label: "Run DB migrations", value: true },
        {
          type: "singleSelect",
          id: "target-env",
          label: "Environment",
          options: [
            { id: "staging", label: "Staging" },
            { id: "prod", label: "Production" }
          ]
        },
        { type: "slider", id: "canary-pct", label: "Canary %", min: 0, max: 50, value: 10 }
      ]
    },
    {
      type: "section",
      title: "Notes",
      children: [
        { type: "textarea", id: "operator-notes", label: "Anything I should know?", rows: 3 }
      ]
    }
  ]
};

const meta = {
  title: "Agent/InlineCanvas",
  component: InlineCanvas,
  parameters: {
    layout: "fullscreen"
  },
  decorators: [
    () => ({
      Component: StoryFrame,
      props: {
        style: [
          "min-height: 720px",
          "padding: 32px",
          "background: var(--background)",
          "color: var(--foreground)",
          "display: flex",
          "justify-content: center"
        ].join(";")
      }
    }),
    () => ({
      Component: StoryFrame,
      props: {
        // the chat column the canvas actually renders in
        style: "width: min(800px, 100%);"
      }
    })
  ],
  args: {
    spec: reviewSpec,
    canvasId: "canvas-storybook",
    sessionId: "session-storybook",
    onSubmitCanvasState: () => true
  }
} satisfies Meta<typeof InlineCanvas>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ReviewWithActions: Story = {};

export const ParameterIntake: Story = {
  args: {
    spec: intakeSpec
  }
};
