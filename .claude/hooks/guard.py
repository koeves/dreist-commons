#!/usr/bin/env python3
"""Claude Code hooks for this repository (wired up in .claude/settings.json).

pre-tool-use   blocks what the agent rules in AGENTS.md forbid: Claude
               attribution in commits, pull requests, issues and comments;
               pushes to dev or main; force pushes without a lease; blanket
               staging; a repo-local git identity; worktrees outside
               .claude/worktrees; edits to generated files. Merging a pull
               request asks first.
session-start  prints the open code scanning and Dependabot alert counts.

Keep this file identical in dreist-commons, dreist and einst.
"""

import json
import os
import re
import shlex
import subprocess
import sys

ATTRIBUTION = re.compile(
    r"co-authored-by:\s*claude"
    r"|claude-session:"
    r"|generated (with|by) \[?claude"
    r"|claude-projects-attribution"
    r"|claude\.ai/code"
    r"|claude\.ai/project"
    r"|\[project thread\]",
    re.IGNORECASE,
)

PROTECTED_BRANCHES = {"dev", "main"}

# Generated files: edit their generator instead (AGENTS.md).
GENERATED = ("assets/ui/", "assets/ui-v1/", "THIRD_PARTY_NOTICES.md")

# Tools that write text to GitHub.
GITHUB_WRITES = re.compile(r"^mcp__github__(create|update|add|push|issue_write|pull_request_review_write|sub_issue_write)")
GITHUB_MERGE = re.compile(r"^mcp__github__(merge_pull_request|enable_pr_auto_merge)$")


def decide(decision, reason):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": decision, "permissionDecisionReason": reason}}))
    sys.exit(0)


def deny(reason):
    decide("deny", reason)


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for v in value.values():
            yield from strings(v)
    elif isinstance(value, list):
        for v in value:
            yield from strings(v)


def check_attribution(texts):
    for text in texts:
        m = ATTRIBUTION.search(text)
        if m:
            deny(f"No Claude attribution or claude.ai links in commits, pull requests, issues or comments (found {m.group(0)!r}). Remove it and retry.")


def current_branch(cwd):
    try:
        return subprocess.run(["git", "-C", cwd, "branch", "--show-current"], capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def git_segments(command):
    """Each `git ...` invocation in a shell command, as a token list after `git`."""
    for part in re.split(r"&&|\|\||;|\||\n", command):
        try:
            tokens = shlex.split(part)
        except ValueError:
            tokens = part.split()
        while tokens and ("=" in tokens[0] and not tokens[0].startswith("-") or tokens[0] in ("sudo", "command", "env")):
            tokens = tokens[1:]
        if not tokens or os.path.basename(tokens[0]) != "git":
            continue
        args = tokens[1:]
        # Drop global options (-C dir, -c key=value, --no-pager ...).
        while args and args[0].startswith("-"):
            args = args[2:] if args[0] in ("-C", "-c") else args[1:]
        if args:
            yield args


def check_bash(command, cwd, project):
    if re.search(r"\b(git\s+commit|git\s+tag|gh\s+(pr|issue|api|release))\b", command):
        check_attribution([command])
    if re.search(r"\bgh\s+pr\s+merge\b", command):
        decide("ask", "The user merges pull requests themselves; merge only when they asked for this one.")

    for args in git_segments(command):
        sub, rest = args[0], args[1:]
        if sub == "push":
            if any(a in ("-f", "--force") or a.startswith("--force=") for a in rest) or any(a.startswith("+") for a in rest if not a.startswith("-")):
                deny("No force pushes without a lease; use --force-with-lease, and only with the user's approval.")
            refs = [a for a in rest if not a.startswith("-")][1:]
            if not refs and current_branch(cwd) in PROTECTED_BRANCHES:
                deny("dev and main take changes only through pull requests; push a feature branch instead.")
            for ref in refs:
                target = ref.split(":")[-1].removeprefix("refs/heads/")
                if target in PROTECTED_BRANCHES and not ref.startswith(":"):
                    deny("dev and main take changes only through pull requests; push a feature branch instead.")
        elif sub == "add":
            if any(a in ("-A", "--all", ".", "./", ":/", "*") for a in rest):
                deny("Stage only the paths that belong to this change (git add <paths>), never everything.")
        elif sub == "commit":
            if any(a in ("-a", "--all") or re.fullmatch(r"-[a-zA-Z]*a[a-zA-Z]*", a) and not a.startswith("--") for a in rest):
                deny("Stage only the paths that belong to this change (git add <paths>), then commit without -a.")
        elif sub == "config":
            if any(re.fullmatch(r"user\.(name|email)", a) for a in rest) and "--global" not in rest and not {"--get", "--list", "-l"} & set(rest):
                values = [a for a in rest if not a.startswith("-")]
                if len(values) >= 2:
                    deny("Use the global git identity; never set a repo-local user.name or user.email.")
        elif sub == "worktree" and rest[:1] == ["add"]:
            # The first argument that is neither an option nor an option's value.
            paths, skip = [], False
            for a in rest[1:]:
                if skip:
                    skip = False
                elif a in ("-b", "-B", "--reason"):
                    skip = True
                elif not a.startswith("-"):
                    paths.append(a)
            if paths:
                path = os.path.realpath(os.path.join(cwd, paths[0]))
                allowed = os.path.realpath(os.path.join(project, ".claude", "worktrees"))
                if not path.startswith(allowed + os.sep):
                    deny("Worktrees go in .claude/worktrees/<name> of this repository.")


def check_edit(tool_input, project):
    path = tool_input.get("file_path") or tool_input.get("notebook_path") or ""
    if not path:
        return
    rel = os.path.relpath(os.path.realpath(path), os.path.realpath(project))
    if rel.startswith(".."):
        return
    rel = rel.replace(os.sep, "/")
    for gen in GENERATED:
        if rel == gen or rel.startswith(gen):
            deny(f"{rel} is generated; change its generator (scripts/uigen or the notices generator) and regenerate.")


def pre_tool_use(event):
    tool = event.get("tool_name", "")
    tool_input = event.get("tool_input") or {}
    cwd = event.get("cwd") or os.getcwd()
    project = os.environ.get("CLAUDE_PROJECT_DIR") or cwd
    if tool == "Bash":
        check_bash(tool_input.get("command", ""), cwd, project)
    elif tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
        check_edit(tool_input, project)
    elif GITHUB_MERGE.match(tool):
        decide("ask", "The user merges pull requests themselves; merge only when they asked for this one.")
    elif GITHUB_WRITES.match(tool):
        check_attribution(strings(tool_input))
        branch = tool_input.get("branch", "")
        if tool in ("mcp__github__push_files", "mcp__github__create_or_update_file", "mcp__github__delete_file") and branch in PROTECTED_BRANCHES:
            deny("dev and main take changes only through pull requests; write to a feature branch instead.")


def session_start(event):
    cwd = event.get("cwd") or os.getcwd()
    try:
        url = subprocess.run(["git", "-C", cwd, "remote", "get-url", "origin"], capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return
    m = re.search(r"github\.com[:/]([^/]+/[^/.]+?)(\.git)?$", url)
    if not m:
        return
    repo = m.group(1)
    counts = []
    for kind, label in (("code-scanning", "code scanning"), ("dependabot", "Dependabot")):
        try:
            out = subprocess.run(["gh", "api", f"repos/{repo}/{kind}/alerts?state=open&per_page=100", "--jq", "length"], capture_output=True, text=True, timeout=8)
        except (OSError, subprocess.SubprocessError):
            return
        if out.returncode != 0:
            return
        counts.append(f"{out.stdout.strip()} open {label}")
    text = f"{repo}: {', '.join(counts)} alerts. Check them at the start and end of this work (AGENTS.md, Security alerts)."
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}))


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "pre-tool-use":
        pre_tool_use(event)
    elif mode == "session-start":
        session_start(event)


if __name__ == "__main__":
    main()
