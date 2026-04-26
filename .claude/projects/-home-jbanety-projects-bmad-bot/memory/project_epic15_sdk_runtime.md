---
name: Epic 15 SDK runtime decision
description: Approved sprint change — dual runtime (API via rig + SDK via CLI subprocess) with Claude Code and Codex as providers
type: project
---

Epic 15 adds SDK-based providers (Claude Code, Codex) alongside existing API providers (Anthropic, OpenAI-compatible via rig).

**Why:** Leverage Claude Code and Codex's built-in tools, context management, and agentic capabilities without maintaining custom tool implementations. User chooses per-role.

**How to apply:**
- Pipeline orchestration is UNCHANGED — same phases, same Decision 10 consultations
- Skills invoked natively (slash commands) — no inlined content, no daemon preamble for SDK mode
- Supervisor exposed as MCP server (stdio) — only `ask_supervisor`, no consultation MCP tools
- BMAD manifest (`_bmad/_config/manifest.yaml` → `ides[]`) used to discover skill paths — replaces hardcoded `.claude/skills/`
- Session IDs tracked per phase in WAL for `--resume` (consultation injection + crash recovery)
- SDK subprocess supervisor must NOT receive MCP config (prevents recursion loop)
- BMAD installer handles skill placement — daemon only validates presence at startup
