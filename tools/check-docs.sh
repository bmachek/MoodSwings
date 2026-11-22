#!/usr/bin/env bash
# The agent instructions — AGENTS.md, the CLAUDE.md that imports it, the
# .claude/rules/ chapters and both agent rosters — held in agreement. The
# contract, and why it replaced the old "CLAUDE.md and AGENTS.md are twins",
# is written down at the top of tools/check_docs.py, which does the work.
#
#   tools/check-docs.sh          # report; exit 1 on any finding
#   tools/check-docs.sh --fix    # regenerate .codex/agents/ and the rules table
#
# This name is what CI, the release workflow, the PR template and every page
# that mentions the check call, so it stays a shell script in front of the
# Python. `--hook` is the PostToolUse entry the settings used to carry; it is
# tools/agent_hook.py's job now, and is forwarded so an old config keeps working.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
if [ "${1:-}" = --hook ]; then
    exec python3 "$here/agent_hook.py" post-edit
fi
exec python3 "$here/check_docs.py" "$@"
