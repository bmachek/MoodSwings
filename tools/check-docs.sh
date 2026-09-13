#!/usr/bin/env bash
# CLAUDE.md and AGENTS.md are twins, the way the two fetch scripts are twins:
# the same architectural tour, addressed to two different agents. The fetch
# scripts have a Rust test holding them together. These two had nothing, and
# drifted — AGENTS.md sat forty-two lines behind, missing the atlas buildings,
# the relief, the hill clip and the queue, which is exactly the kind of thing a
# reader of the stale copy then walks into.
#
# The contract: everything from line 4 down must be identical. The first three
# lines are the title and the one sentence naming the tool, and they are the
# only lines allowed to differ.
#
#   tools/check-docs.sh          # report; exit 1 if they have drifted
#   tools/check-docs.sh --fix    # rewrite AGENTS.md from CLAUDE.md, keeping its header
#   tools/check-docs.sh --hook   # for the PostToolUse hook; silent unless a twin was edited
#
# Which way `--fix` copies is a decision, not a coin toss: CLAUDE.md is the one
# a Claude Code session loads automatically on every turn, so it is the one that
# gets edited in passing and therefore the source. If a change was made in
# AGENTS.md instead, copy it over by hand before running --fix, or it is lost.
set -uo pipefail

cd "$(dirname "$0")/.."

HEADER_LINES=3
MODE=check
case "${1:-}" in
    --fix) MODE=fix ;;
    --hook) MODE=hook ;;
    "") ;;
    *)
        echo "check-docs: unknown argument: $1" >&2
        exit 2
        ;;
esac

if [ "$MODE" = hook ]; then
    # PostToolUse fires on every edit in the session; only the twins are our
    # business. The payload arrives on stdin as JSON.
    payload=$(cat)
    path=$(printf '%s' "$payload" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)
    case "$path" in
        *CLAUDE.md | *AGENTS.md) ;;
        *) exit 0 ;;
    esac
fi

body() { tail -n +$((HEADER_LINES + 1)) "$1"; }

if diff -q <(body AGENTS.md) <(body CLAUDE.md) >/dev/null 2>&1; then
    [ "$MODE" = hook ] || echo "CLAUDE.md and AGENTS.md agree below line $HEADER_LINES."
    exit 0
fi

DRIFT=$(diff <(body AGENTS.md) <(body CLAUDE.md) | grep -c '^[<>]')

case "$MODE" in
    hook)
        printf '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"CLAUDE.md and AGENTS.md have drifted apart (%s differing lines below line %s). They are twins and must be kept in sync in the same commit: mirror the edit, or run tools/check-docs.sh --fix to rewrite AGENTS.md from CLAUDE.md (its first %s lines are kept)."}}\n' \
            "$DRIFT" "$HEADER_LINES" "$HEADER_LINES"
        exit 0
        ;;
    fix)
        tmp=$(mktemp)
        head -n "$HEADER_LINES" AGENTS.md >"$tmp"
        body CLAUDE.md >>"$tmp"
        mv "$tmp" AGENTS.md
        echo "Rewrote AGENTS.md from CLAUDE.md ($DRIFT lines differed); its first $HEADER_LINES lines are unchanged."
        exit 0
        ;;
    check)
        echo "CLAUDE.md and AGENTS.md have drifted: $DRIFT differing lines below line $HEADER_LINES."
        echo "(< AGENTS.md, > CLAUDE.md)"
        diff <(body AGENTS.md) <(body CLAUDE.md)
        echo
        echo "Mirror the change by hand, or run: tools/check-docs.sh --fix"
        exit 1
        ;;
esac
