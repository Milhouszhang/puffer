# Permissions And Skills

Status: live architecture guardrail for permission, ACL, browser, skill, and
Lambda Skill behavior.

Puffer has two related but distinct systems:

- Permissions decide whether one tool call is allowed, denied, or must ask the
  user.
- Skills add instructions, optional slash commands, and optional tool scopes.
  A skill being present in the prompt is not permission to bypass runtime
  enforcement.

## Permission Inputs

The effective decision for a tool call comes from:

- request-scoped tool filters, such as a slash command or verified Lambda Skill
  restricting which tools may appear in a side turn;
- workspace permission settings in `.puffer/permissions.toml`;
- project ACL rules in `.puffer/permissions.acl`;
- tool metadata from `resources/tools/*.yaml`;
- browser policy and browser target classification;
- hard-coded tool-specific safety rules in `crates/puffer-core/permissions.rs`.

`RuntimePermissionContext::decision_for_tool_call` is the main decision point.
Tools can return `Allow`, `Ask`, or `Deny`; `Ask` must surface through the
permission prompt path rather than being silently coerced to allow.

## Project ACL

Project ACLs live at `.puffer/permissions.acl` under the active workspace. The
format is line oriented:

```text
[project workdir]
100 Allow bash argv bobo-link
100 Allow read dir docs
100 Deny browser evaluate example.com
```

Supported scopes:

- `all`
- `read file|dir <path>`
- `write file|dir <path>`
- `bash argv <command>`
- `browser domain <domain>`
- `browser action <inspect|navigate|interact|evaluate> <domain>`

Rules are project-scoped and priority ordered. Do not use broad `Allow all`
rules for product flows; prefer the narrowest executable, path, browser domain,
or browser action that satisfies the workflow.

Shell ACL allow rules intentionally do not bypass shell control operators or
redirection. Compound commands still ask because they can smuggle extra work
behind an allowed argv.

## Browser Permissions

Browser actions are normalized into coarse permission categories:

- inspect: snapshots, screenshots, DOM inspection, network idle waits
- navigate: open, focus, close, reload, history navigation
- interact: click, hover, type, upload, scroll, key presses
- evaluate: arbitrary page/script evaluation

Browser permissions must keep target information. A permission that only says
"Browser allowed" is too broad for connector/browser-product work. Prefer ACL
rules such as:

```text
100 Allow browser action inspect example.com
100 Allow browser action navigate example.com
100 Allow browser action interact example.com
```

Use `evaluate` sparingly and only where the product contract requires it.

## Skills

Skill resources are loaded from bundled, user, and workspace layers:

- bundled: `resources/skills/<name>/SKILL.md`
- user: `~/.puffer/resources/skills/<name>/SKILL.md`
- workspace: `.puffer/resources/skills/<name>/SKILL.md`

The skill frontmatter is modeled by `SkillSpec` and may include:

- `allowed_tools`
- `argument_hint`
- `user_invocable`
- `model`
- `effort`
- `disable_model_invocation`
- `requires_action`
- `verification`

`/skills` lists loaded skills. `/skills config` prints the Lambda Skill manifest
locations and template.

Important guardrail: ordinary skills are prompt/instruction resources. They do
not grant permission by themselves. If a skill needs a local executable, browser
action, connector action, or filesystem write, that capability must still be
covered by normal permission policy or ACL.

## Lambda Skills

Verified Lambda Skills add a stricter activation path:

- External library manifests live under
  `.puffer/resources/lambda_skill_libraries/` or
  `~/.puffer/resources/lambda_skill_libraries/`.
- The manifest points at a verified skill library root and host catalogue.
- Verified skills must have non-empty `allowed_tools`.
- `LambdaHostCall` and `LambdaInternal` are added to the side-turn tool scope
  when absent.
- The Lambda gate is installed while the skill runs and removed afterward.
- `require_approval` controls whether concrete verified host actions require a
  user approval step.

If the precompiled host catalogue is missing, the verified skill must fail closed
instead of falling back to broad model/tool execution.

## Connector And External Actions

Connector/browser/product actions are side-effecting. Keep the permission
contract precise:

- The model should request a typed action such as `ConnectorAct`,
  `MonitorReplySend`, or a specific browser action.
- The permission layer should know which connector/action/domain is being used.
- User-facing approval should preserve the exact target, action, and payload
  summary.
- Success-shaped skips, denied actions, and failed sends should remain distinct
  in traces and task state.

Do not encode real authorization solely in prompts. Prompts can guide behavior,
but runtime guards must enforce the boundary.

## Change Checklist

When adding or changing a tool, skill, connector action, or browser capability:

1. Update the resource manifest or registry.
2. Decide whether the tool is visible to the model under default permissions.
3. Add or update permission/ACL tests for allow, ask, and deny paths.
4. Keep permission reasons specific enough for UI and diagnostics.
5. Update this doc when a new permission dimension is introduced.
