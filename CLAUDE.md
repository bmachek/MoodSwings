# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENTS.md

## What only Claude Code has

Everything above is shared with Codex. This is the part of the harness that is
Claude Code's alone; `tools/check-docs.sh` refuses a heading here that the
shared file already has, so the two cannot grow back into twins.

- **Slash commands are skills** (`.claude/skills/`). `/verify`, `/traps`,
  `/survey` and `/docs-sync` run *inside* their agent (`context: fork`) and
  wait for it; `/shot`, `/patrol`, `/perf` and `/audition` do the same in the
  background, because they take minutes. Either way the compiler output, the
  PNGs and the scorecards stay in the agent and only the verdict comes back.
  `/ship` runs here, because it is the one that fans out — verify, traps, docs
  and the bundled `/code-review`, in parallel — and `/setup` runs here too.
- **Rules load themselves.** A `.claude/rules/` chapter arrives the first time
  this session (or a subagent) reads a file its `paths:` names. The *Read before
  you touch* table above is the same list for agents that cannot do that.
- **Code intelligence.** `.claude/settings.json` enables the
  `rust-analyzer-lsp` plugin. It needs `rust-analyzer` on `PATH` and, once per
  clone, `claude plugin install rust-analyzer-lsp@claude-plugins-official
  --scope project`. After that every edit of a `.rs` file comes back with the
  compiler's diagnostics, and the `LSP` tool answers definition, references,
  implementations and call hierarchy exactly. Reach for it before `grep` on a
  symbol; reach for graphify (below) when the question is where something lives
  or how two things connect.
- **Hooks** (`.claude/settings.json`), each silent when it has nothing to say:
  - `SessionStart` — `tools/dev-setup.sh --hook`: toolchain, sound bank, GPU.
  - `PostToolUse` on an edit — `tools/agent_hook.py post-edit` runs `rustfmt`
    on the `.rs` file just written and `tools/check-docs.sh`'s checks when the
    edit touched the agent instructions.
  - `Stop` — `tools/agent_hook.py stop` runs CI's clippy gate whenever the
    Rust in the tree has changed since the gate last saw it, and sends the turn
    back with the errors if it fails. A turn does not end on a tree CI would
    reject without the session hearing about it; a state it has already judged
    is not judged twice, which is also what keeps it from looping.
  - graphify's guards on search and read, below.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. When the answer is under a Markdown heading, `graphify section "<heading>"` prints that section's text, so the .md need not be opened. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
- A committed graphify-out/graph.json is rewritten whole on every build, so a large diff to it in a pull request is expected. Do not flag it as churn or suggest skipping the graph rebuild or its hooks.
