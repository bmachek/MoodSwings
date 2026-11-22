#!/usr/bin/env python3
"""Hold the agent instructions to one copy, and the copies we cannot avoid to
their source.

    tools/check-docs.sh          # report; exit 1 on any finding
    tools/check-docs.sh --fix    # regenerate what is generated, then report the rest

CLAUDE.md and AGENTS.md used to be twins — the same 650-line tour twice, held
identical below line 3 by this script. That had two costs nobody saw. Codex
stops reading AGENTS.md at `project_doc_max_bytes`, 32 KiB by default, and the
tour had grown to 41 KiB, so Codex was working without the last fifth of it:
the traps, the rendering rule, the conventions. And Claude Code loads its copy
whole on every turn and into every subagent, three times the length its own
documentation asks for, most of it chapters that matter to one or two files.

So the contract is now:

1. AGENTS.md is the copy. CLAUDE.md imports it (`@AGENTS.md`) and adds only
   what is Claude Code's alone; a heading in both means the twins are growing
   back. AGENTS.md stays under Codex's 32 KiB, and CLAUDE.md's own text under
   the 200 lines at which Claude Code starts warning.
2. A chapter that matters to a few files lives in `.claude/rules/<name>.md`
   with a `paths:` list. Claude Code loads it when a session reads a matching
   file; everybody else finds it through AGENTS.md's *Read before you touch*
   table, whose first column is each chapter's `paths:` written out. Every glob
   has to match a file in the tree, because a rule whose file was renamed
   never loads again and says nothing about it.
3. `.codex/agents/<name>.toml` is generated from `.claude/agents/<name>.md` —
   name, description, reasoning effort and instructions — and `--fix` writes
   it. It was ported by hand once, by search and replace, and the port of
   docs-steward told Codex to keep "AGENTS.md and AGENTS.md in sync".
4. A skill that forks into an agent names one that exists.

Python rather than shell because of (3): writing TOML that survives a
backslash or a quote is not a job for sed. It stays 3.9-compatible — the
python3 a Mac ships — and needs nothing outside the standard library.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AGENTS = ROOT / "AGENTS.md"
CLAUDE = ROOT / "CLAUDE.md"
RULES = ROOT / ".claude" / "rules"
CLAUDE_AGENTS = ROOT / ".claude" / "agents"
CODEX_AGENTS = ROOT / ".codex" / "agents"
SKILLS = ROOT / ".claude" / "skills"

# Codex's default `project_doc_max_bytes`: it stops adding instructions here.
CODEX_DOC_LIMIT = 32 * 1024
# Claude Code warns about a CLAUDE.md longer than this, at startup and in /status.
CLAUDE_OWN_LINES = 200
TABLE_HEADING = "## Read before you touch"
# Agents a skill may fork into without one of ours being defined.
BUILTIN_AGENTS = {"general-purpose", "Explore", "Plan"}
# Claude Code's effort levels against what a Codex agent file takes. Codex has
# no level above `high` that every model accepts, so the top two fold into it.
CODEX_EFFORT = {"low": "low", "medium": "medium", "high": "high", "xhigh": "high", "max": "high"}
GENERATED_MARK = "# Generated from .claude/agents/"


class Finding:
    def __init__(self, message, fix=None):
        self.message = message
        self.fix = fix  # a callable that repairs it, or None

    def __str__(self):
        return self.message + ("" if self.fix is None else "  [--fix repairs this]")


# -- small parsers ------------------------------------------------------------


def split_front(text):
    """(frontmatter, body) of a Markdown file; frontmatter is None without one."""
    m = re.match(r"---\n(.*?)\n---\n", text, re.S)
    if not m:
        return None, text
    return m.group(1), text[m.end():]


def scalar(front, key):
    m = re.search(rf"^{re.escape(key)}:[ \t]*(.*)$", front or "", re.M)
    if not m:
        return None
    return m.group(1).strip().strip("\"'")


def listval(front, key):
    """A YAML list written either inline (`a, b`) or as `- item` lines."""
    lines = (front or "").splitlines()
    for i, line in enumerate(lines):
        m = re.match(rf"^{re.escape(key)}:[ \t]*(.*)$", line)
        if not m:
            continue
        inline = m.group(1).strip()
        if inline:
            return [p.strip().strip("\"'") for p in inline.strip("[]").split(",") if p.strip()]
        items = []
        for follow in lines[i + 1:]:
            item = re.match(r"^\s+-\s+(.*)$", follow)
            if not item:
                break
            items.append(item.group(1).strip().strip("\"'"))
        return items
    return []


def headings(text):
    """Every heading outside a fenced block, without its #s."""
    found, fence = set(), False
    for line in text.splitlines():
        if line.startswith("```"):
            fence = not fence
        elif not fence:
            m = re.match(r"^#{1,6} (.+?)\s*$", line)
            if m:
                found.add(m.group(1))
    return found


def expand_braces(pattern):
    m = re.search(r"\{([^{}]*)\}", pattern)
    if not m:
        return [pattern]
    head, tail = pattern[: m.start()], pattern[m.end():]
    return [x for alt in m.group(1).split(",") for x in expand_braces(head + alt + tail)]


def glob_regex(pattern):
    out, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif pattern.startswith("**", i):
            out, i = out + ".*", i + 2
        elif pattern[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif pattern[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(pattern[i]), i + 1
    return re.compile(out + r"\Z")


def tree_files():
    try:
        listed = subprocess.run(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
            cwd=ROOT, capture_output=True, text=True, check=True,
        ).stdout.splitlines()
    except (OSError, subprocess.CalledProcessError):
        listed = [str(p.relative_to(ROOT)) for p in ROOT.rglob("*") if p.is_file()]
    return listed


def toml_string(s):
    escaped = s.replace("\\", "\\\\").replace('"', '\\"')
    escaped = re.sub(r"[\x00-\x1f\x7f]", lambda m: "\\u%04x" % ord(m.group()), escaped)
    return '"' + escaped + '"'


# -- the four checks ----------------------------------------------------------


def check_import():
    findings = []
    claude, agents = CLAUDE.read_text(encoding="utf-8"), AGENTS.read_text(encoding="utf-8")
    if not re.search(r"^@AGENTS\.md\s*$", claude, re.M):
        findings.append(Finding("CLAUDE.md does not import AGENTS.md: it needs a line that is exactly `@AGENTS.md`."))
    shared = (headings(claude) & headings(agents)) - {"CLAUDE.md", "AGENTS.md"}
    for heading in sorted(shared):
        findings.append(Finding(
            f"`{heading}` is a heading in both CLAUDE.md and AGENTS.md. The shared text lives in AGENTS.md "
            "only; CLAUDE.md keeps what is Claude Code's alone."
        ))
    size = len(agents.encode("utf-8"))
    if size > CODEX_DOC_LIMIT:
        findings.append(Finding(
            f"AGENTS.md is {size} bytes; Codex stops reading at {CODEX_DOC_LIMIT}. Move a chapter that only "
            "matters to a few files into .claude/rules/ and give it a row in *Read before you touch*."
        ))
    own = claude.count("\n")
    if own > CLAUDE_OWN_LINES:
        findings.append(Finding(
            f"CLAUDE.md's own text is {own} lines; Claude Code warns above {CLAUDE_OWN_LINES}. "
            "Shared guidance belongs in AGENTS.md."
        ))
    return findings


def read_table(agents_text):
    """{rule name: (row index, [globs], blurb)} from *Read before you touch*."""
    lines = agents_text.splitlines()
    try:
        start = lines.index(TABLE_HEADING)
    except ValueError:
        return lines, None, {}
    rows = {}
    for i in range(start + 1, len(lines)):
        line = lines[i]
        if line.startswith("## "):
            break
        m = re.match(r"^\|(.*)\|\s*`\.claude/rules/([\w-]+)\.md`(.*)\|\s*$", line)
        if m:
            rows[m.group(2)] = (i, re.findall(r"`([^`]+)`", m.group(1)), m.group(3))
    return lines, start, rows


def check_rules():
    findings = []
    files = tree_files()
    agents_text = AGENTS.read_text(encoding="utf-8")
    lines, start, rows = read_table(agents_text)
    if start is None:
        return [Finding(f"AGENTS.md has no `{TABLE_HEADING}` section listing the .claude/rules/ chapters.")]

    on_disk = {p.stem: p for p in sorted(RULES.glob("*.md"))} if RULES.is_dir() else {}
    for name, path in on_disk.items():
        front, _ = split_front(path.read_text(encoding="utf-8"))
        globs = listval(front, "paths")
        if not globs:
            findings.append(Finding(
                f".claude/rules/{name}.md has no `paths:`. Without one Claude Code loads it on every turn, "
                "which is what moving it out of AGENTS.md was meant to stop."
            ))
            continue
        for glob in globs:
            for expanded in expand_braces(glob):
                rx = glob_regex(expanded)
                if not any(rx.match(f) for f in files):
                    findings.append(Finding(
                        f".claude/rules/{name}.md: `{expanded}` matches no file in the tree, so the chapter "
                        "never loads for it. A file was renamed or removed; follow it."
                    ))
        if name not in rows:
            findings.append(Finding(
                f".claude/rules/{name}.md has no row in AGENTS.md's *Read before you touch*, so no agent but "
                "Claude Code will ever read it. Add one: the paths, then the file and what it is about."
            ))
        elif sorted(rows[name][1]) != sorted(globs):
            index, _, blurb = rows[name]

            def fix(index=index, globs=globs, blurb=blurb, name=name):
                text = AGENTS.read_text(encoding="utf-8").splitlines()
                cells = ", ".join(f"`{g}`" for g in globs)
                text[index] = f"| {cells} | `.claude/rules/{name}.md`{blurb}|"
                AGENTS.write_text("\n".join(text) + "\n", encoding="utf-8")

            findings.append(Finding(
                f"AGENTS.md's row for .claude/rules/{name}.md lists other paths than the chapter's `paths:`.",
                fix,
            ))
    for name in rows:
        if name not in on_disk:
            findings.append(Finding(f"AGENTS.md's *Read before you touch* names .claude/rules/{name}.md, which does not exist."))
    return findings


def codex_agent(md_path):
    """The .codex/agents/<name>.toml that .claude/agents/<name>.md implies."""
    front, body = split_front(md_path.read_text(encoding="utf-8"))
    name, description = scalar(front, "name"), scalar(front, "description")
    if not name or not description:
        raise ValueError(f"{md_path.relative_to(ROOT)} needs a `name:` and a `description:`")
    if "'''" in body:
        raise ValueError(f"{md_path.relative_to(ROOT)} contains ''' , which a TOML literal string cannot hold")
    out = (
        f"{GENERATED_MARK}{md_path.name} by tools/check-docs.sh --fix.\n"
        "# Edit that file, not this one: check-docs fails on any difference.\n"
        f"name = {toml_string(name)}\n"
        f"description = {toml_string(description)}\n"
    )
    effort = scalar(front, "effort")
    if effort:
        if effort not in CODEX_EFFORT:
            raise ValueError(f"{md_path.relative_to(ROOT)}: unknown effort `{effort}`")
        out += f'model_reasoning_effort = "{CODEX_EFFORT[effort]}"\n'
    # A literal string: nothing in it is an escape, so the instructions arrive
    # exactly as written — backslashes, quotes and all.
    out += "developer_instructions = '''\n" + body.strip("\n") + "\n'''\n"
    return name, out


def check_agents():
    findings, names = [], set()
    for md in sorted(CLAUDE_AGENTS.glob("*.md")):
        try:
            name, want = codex_agent(md)
        except ValueError as err:
            findings.append(Finding(str(err)))
            continue
        names.add(name)
        target = CODEX_AGENTS / f"{md.stem}.toml"
        have = target.read_text(encoding="utf-8") if target.exists() else None
        if have != want:

            def fix(target=target, want=want):
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(want, encoding="utf-8")

            what = "is missing" if have is None else "differs from its source"
            findings.append(Finding(f".codex/agents/{target.name} {what} (.claude/agents/{md.name}).", fix))
    for toml in sorted(CODEX_AGENTS.glob("*.toml")) if CODEX_AGENTS.is_dir() else []:
        if not (CLAUDE_AGENTS / f"{toml.stem}.md").exists():
            generated = toml.read_text(encoding="utf-8").startswith(GENERATED_MARK)
            findings.append(Finding(
                f".codex/agents/{toml.name} has no .claude/agents/{toml.stem}.md to come from.",
                (lambda toml=toml: toml.unlink()) if generated else None,
            ))
    return findings, names


def check_skills(agent_names):
    findings = []
    for skill in sorted(SKILLS.glob("*/SKILL.md")) if SKILLS.is_dir() else []:
        front, _ = split_front(skill.read_text(encoding="utf-8"))
        agent = scalar(front, "agent")
        if agent and agent not in agent_names and agent not in BUILTIN_AGENTS:
            findings.append(Finding(f"{skill.relative_to(ROOT)} forks into `{agent}`, which no .claude/agents/ file defines."))
        if agent and scalar(front, "context") != "fork":
            findings.append(Finding(f"{skill.relative_to(ROOT)} names an agent but not `context: fork`, so the agent is ignored."))
    return findings


def findings():
    agent_findings, names = check_agents()
    return check_import() + check_rules() + agent_findings + check_skills(names)


def main(argv):
    if argv and argv[0] not in ("--fix",):
        print(f"check-docs: unknown argument: {argv[0]}", file=sys.stderr)
        return 2
    found = findings()
    if argv and argv[0] == "--fix":
        repaired = [f for f in found if f.fix is not None]
        for f in repaired:
            f.fix()
            print(f"fixed: {f.message}")
        found = findings()
    if not found:
        print("The agent instructions agree: one AGENTS.md, a CLAUDE.md that imports it, rules that load, agents in step.")
        return 0
    for f in found:
        print(f"- {f}")
    if any(f.fix for f in found):
        print("\nRun tools/check-docs.sh --fix for the ones marked; the rest need a hand.")
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
