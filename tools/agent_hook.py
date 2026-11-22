#!/usr/bin/env python3
"""The repository's half of the agent hooks — Claude Code's and Codex's alike.

    python3 tools/agent_hook.py post-edit   # PostToolUse, after a file edit
    python3 tools/agent_hook.py stop        # Stop, before a turn is allowed to end

Both read the hook's JSON payload on stdin, and both have to understand two
shapes of it. Claude Code names the file it edited (`tool_input.file_path`);
Codex edits through `apply_patch` and sends only the patch, so the files are
read off its `*** Update File:` lines. Getting that wrong is silent — the old
shell hook read `file_path` with sed, found nothing in a Codex payload, and so
had never once run there.

post-edit
    `rustfmt` on every `.rs` file the edit wrote, with the edition of the
    package it belongs to, so formatting is never a finding and never a round
    trip. Then, if the edit touched the agent instructions, the checks in
    tools/check_docs.py, reported back as context rather than as an error: a
    half-finished edit of an agent file is a normal state to be in for one
    step.

stop
    The clippy gate CI runs (`--workspace --all-targets -- -D warnings`), run
    whenever the Rust in the working tree differs from HEAD and this exact state
    has not been judged before. A failure sends the turn back (exit 2, the
    errors on stderr) so a turn does not end on a tree CI would reject without
    the session hearing about it. Warm, it costs one to six seconds on this
    crate; a state that has been judged is not judged again, which is also what
    stops it looping when the failure is deliberate and the session has said so.
    Committed work is CI's and rust-verify's to judge; a clean tree is skipped.
"""

import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GATE_MARK = ROOT / "target" / "agent-gate" / "last"
GATE = ["cargo", "clippy", "--workspace", "--all-targets", "--message-format=short", "--", "-D", "warnings"]
# Well inside the hook's own timeout in both configs, so the hook reports
# rather than being killed. A cold first build of every dependency fits.
GATE_TIMEOUT = 540
# What counts as "the agent instructions" for the docs check.
DOC_PATHS = [
    re.compile(r"^(AGENTS|CLAUDE)\.md$"),
    re.compile(r"^\.claude/(rules|agents)/[^/]+\.md$"),
    re.compile(r"^\.claude/skills/[^/]+/SKILL\.md$"),
    re.compile(r"^\.codex/agents/[^/]+\.toml$"),
]


def payload():
    try:
        return json.load(sys.stdin)
    except (ValueError, OSError):
        return {}


def edited_paths(data):
    """Absolute paths of the files an edit wrote, from either harness's payload."""
    ti = data.get("tool_input") or {}
    base = Path(data.get("cwd") or ROOT)
    found = [ti[k] for k in ("file_path", "notebook_path") if isinstance(ti.get(k), str)]
    patch = ti.get("command") if data.get("tool_name") == "apply_patch" else None
    if isinstance(patch, list):
        patch = "\n".join(str(p) for p in patch)
    if isinstance(patch, str):
        found += re.findall(r"^\*\*\* (?:Update File|Add File|Move to): (.+?)\s*$", patch, re.M)
    out = []
    for p in found:
        path = Path(p) if Path(p).is_absolute() else base / p
        try:
            out.append(path.resolve())
        except OSError:
            continue
    return out


def edition_of(path):
    """The edition of the package a source file belongs to — what cargo fmt would pass."""
    for parent in path.parents:
        manifest = parent / "Cargo.toml"
        if manifest.exists():
            m = re.search(r'^edition\s*=\s*"(\d{4})"', manifest.read_text(encoding="utf-8"), re.M)
            return m.group(1) if m else "2021"
        if parent == ROOT:
            break
    return None


def post_edit(data):
    paths = [p for p in edited_paths(data) if ROOT in p.parents]
    for path in paths:
        if path.suffix == ".rs" and path.exists():
            edition = edition_of(path)
            if edition:
                try:
                    # A file that does not parse yet is left alone; the compiler
                    # will say so, and more usefully than rustfmt.
                    subprocess.run(["rustfmt", "--edition", edition, str(path)], capture_output=True, timeout=30)
                except (OSError, subprocess.TimeoutExpired):
                    pass
    rel = [str(p.relative_to(ROOT)) for p in paths]
    if not any(rx.match(r) for r in rel for rx in DOC_PATHS):
        return 0
    sys.path.insert(0, str(ROOT / "tools"))
    import check_docs

    # Only what this edit is party to. An edit to one agent file is told about
    # its own Codex copy, not about the eight others a refactor has yet to
    # reach; an edit to the shared documents hears everything but the copies.
    # The rest is one line, so a half-finished change is not shouted at on
    # every step of finishing it.
    every = check_docs.findings()

    def concerns(finding):
        for r in rel:
            if r.startswith((".claude/agents/", ".codex/agents/", ".claude/skills/")):
                if Path(r).stem in finding.message or r in finding.message:
                    return True
            elif ".codex/agents/" not in finding.message:
                return True
        return False

    found = [f for f in every if concerns(f)]
    if found:
        listing = "\n".join(f"- {f}" for f in found)
        rest = len(every) - len(found)
        elsewhere = f"\n({rest} more elsewhere; tools/check-docs.sh lists them.)" if rest else ""
        context = (
            "The agent instructions disagree after this edit (tools/check-docs.sh):\n"
            f"{listing}{elsewhere}\nCI fails on any of these. Fix them in this change; "
            "tools/check-docs.sh --fix repairs the marked ones."
        )
        event = data.get("hook_event_name") or "PostToolUse"
        print(json.dumps({"hookSpecificOutput": {"hookEventName": event, "additionalContext": context}}))
    return 0


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, check=True).stdout


def rust_state():
    """A fingerprint of the Rust in the tree, or None when it is HEAD's."""
    specs = ["--", "*.rs", "*Cargo.toml", "Cargo.lock"]
    diff = git("diff", "HEAD", "--binary", *specs)
    untracked = git("ls-files", "--others", "--exclude-standard", "--", "*.rs").decode().split()
    if not diff and not untracked:
        return None
    h = hashlib.sha256(git("rev-parse", "HEAD") + diff)
    for rel in sorted(untracked):
        h.update(rel.encode() + b"\0")
        try:
            h.update((ROOT / rel).read_bytes())
        except OSError:
            pass
    return h.hexdigest()


def stop(data):
    if data.get("stop_hook_active") or not (ROOT / "Cargo.toml").exists():
        return 0
    try:
        state = rust_state()
    except (OSError, subprocess.CalledProcessError):
        return 0
    if state is None:
        return 0
    if GATE_MARK.exists() and GATE_MARK.read_text() == state:
        return 0
    env = dict(os.environ, CARGO_TERM_COLOR="never")
    try:
        run = subprocess.run(GATE, cwd=ROOT, capture_output=True, text=True, timeout=GATE_TIMEOUT, env=env)
    except subprocess.TimeoutExpired:
        print(f"clippy gate: not finished after {GATE_TIMEOUT} s, so this tree is unjudged, not passed.", file=sys.stderr)
        return 1
    except OSError as err:
        print(f"clippy gate: could not run cargo ({err}); this tree is unjudged, not passed.", file=sys.stderr)
        return 1
    GATE_MARK.parent.mkdir(parents=True, exist_ok=True)
    GATE_MARK.write_text(state)
    if run.returncode == 0:
        return 0
    diagnostics = [
        line for line in run.stderr.splitlines()
        if re.match(r"^\S+:\d+:\d+: (error|warning)", line) or line.startswith("error")
    ]
    shown = "\n".join(diagnostics[:40]) or run.stderr[-3000:]
    more = f"\n… and {len(diagnostics) - 40} more" if len(diagnostics) > 40 else ""
    print(
        "The tree this turn leaves fails the clippy gate CI runs "
        f"(`{' '.join(GATE[:4])} -- -D warnings`):\n{shown}{more}\n"
        "Fix it before finishing — or, if leaving it failing is deliberate, say so and why; "
        "this exact state will not be gated again.",
        file=sys.stderr,
    )
    return 2


def main(argv):
    if argv[:1] == ["post-edit"]:
        return post_edit(payload())
    if argv[:1] == ["stop"]:
        return stop(payload())
    print("usage: agent_hook.py post-edit|stop", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
