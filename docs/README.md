# Puffer Docs

This index separates live architecture from historical specs. Prefer live docs
when they disagree with dated plans/specs.

## Live Architecture

- `architecture/agent-loop.md` - provider-neutral turn loop and provider adapter
  contract.
- `architecture/bobo-daemon-contract.md` - daemon RPC and compatibility surface
  consumed by Bobo.
- `architecture/permissions-and-skills.md` - permission profiles, project ACL,
  browser permissions, skills, and Lambda Skill activation.
- `architecture/autodream-prd.md` - AutoDream product/runtime requirements.

## Reference

- `reference/slash-commands.md` - generated built-in slash-command surface from
  `crates/puffer-core/command/registry.rs`.

## Observability

- `observability/langfuse-design.md` - Langfuse tracing design.

## Historical Change Records

- `../specs/` contains numbered implementation specs. They are append-only
  records for work history and should not be treated as the latest contract
  unless a live doc links to one.

## Local Guardrails

```bash
bash scripts/check-doc-links.sh
bash scripts/check-slash-commands.sh
bash scripts/report-large-files.sh
```
