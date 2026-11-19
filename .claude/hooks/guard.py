#!/usr/bin/env python3
"""Claude Code hooks for this repository (wired up in .claude/settings.json).

pre-tool-use   enforces the agent rules in AGENTS.md:
               1. no Claude/AI attribution in commits, tags, pull requests,
                  issues, comments and releases (Bash and GitHub MCP tools);
               2. no pushes to dev or main; force pushes need approval;
               3. no blanket staging (git add -A / . / commit -a);
               4. no secrets in the staged diff of a commit;
               5. no repo-local git identity;
               6. worktrees only in .claude/worktrees;
               7. no edits to generated files;
               8. merging a pull request asks first.
session-start  prints the open code scanning and Dependabot alert counts.

Prints a permission decision as JSON, nothing to allow. Fails open on its own
errors so a bug never blocks every tool call. Keep this file identical in
dreist-commons, dreist and einst.
"""

import json
import os
import re
import shlex
import subprocess
import sys

PROTECTED = {"dev", "main"}

ATTRIBUTION = [
    r"co-authored-by:\s*claude",
    r"noreply@anthropic\.com",
    r"claude-session:",
    r"generated (with|by) \[?claude",
    "\U0001F916",  # robot emoji
    r"claude\.ai/(code|chat|project|share)",
    r"claude-projects-attribution",
    r"\[project thread\]",
]

SECRET_HARD = [
    (r"AKIA[0-9A-Z]{16}", "AWS access key"),
    (r"-----BEGIN [A-Z ]*PRIVATE KEY-----", "private key"),
    (r"\bgh[pousr]_[A-Za-z0-9]{36,}", "GitHub token"),
    (r"\bgithub_pat_[A-Za-z0-9_]{40,}", "GitHub token"),
    (r"\bsk-ant-[A-Za-z0-9_-]{20,}", "Anthropic API key"),
    (r"\bsk-(proj-)?[A-Za-z0-9_-]{32,}", "API key (sk-)"),
    (r"\bxox[abprs]-[A-Za-z0-9-]{10,}", "Slack token"),
    (r"\bAIza[0-9A-Za-z_-]{35}", "Google API key"),
]
SECRET_SOFT = [
    (r"(?i)(password|passwd|secret|token|api[_-]?key)\s*[:=]\s*['\"][^'\"\s]{8,}['\"]", "hard-coded credential"),
]
SECRET_FILES = re.compile(r"(^|/)(\.env(\.(?!example$|sample$|template$)[^/]+)?|id_(rsa|ed25519|ecdsa)|credentials\.json|[^/]+\.(pem|p12|pfx|key))$")

# Generated files: change their generator and regenerate (AGENTS.md).
GENERATED = ("assets/ui/", "assets/ui-v1/", "THIRD_PARTY_NOTICES.md")

GITHUB_WRITES = re.compile(r"^mcp__github__(create|update|add|push|issue_write|pull_request_review_write|sub_issue_write)")
GITHUB_MERGE = re.compile(r"^mcp__github__(merge_pull_request|enable_pr_auto_merge)$")
GITHUB_FILE_WRITES = {"mcp__github__push_files", "mcp__github__create_or_update_file", "mcp__github__delete_file"}

GIT_OPTS_WITH_VALUE = {"-C", "-c", "--git-dir", "--work-tree", "--namespace"}

MERGE_REASON = "The owner merges pull requests; merge only when they asked for this one."


def decide(decision, reason):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": decision, "permissionDecisionReason": reason}}))
    sys.exit(0)


def segments(cmd):
    """Split a shell command into simple commands (rough, good enough)."""
    out = []
    for part in re.split(r"&&|\|\||;|\||\n", cmd):
        try:
            toks = shlex.split(part, comments=False)
        except ValueError:
            toks = part.split()
        while toks and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", toks[0]):
            toks = toks[1:]
        if toks:
            out.append(toks)
    return out


def parse_git(toks, cwd):
    """(subcommand, args, repo_dir) for a git invocation, else None."""
    if not toks or os.path.basename(toks[0]) != "git":
        return None
    i, repo = 1, cwd
    while i < len(toks) and toks[i].startswith("-"):
        if toks[i] == "-C" and i + 1 < len(toks):
            repo = os.path.join(repo, os.path.expanduser(toks[i + 1]))
            i += 2
        elif toks[i] in GIT_OPTS_WITH_VALUE:
            i += 2
        else:
            i += 1
    if i >= len(toks):
        return None
    return toks[i], toks[i + 1:], repo


def git_out(repo, *args):
    try:
        return subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True, timeout=10).stdout
    except Exception:
        return ""


def read_file(base, path):
    try:
        with open(os.path.join(base, os.path.expanduser(path)), errors="ignore") as f:
            return f.read()
    except Exception:
        return ""


def check_attribution_text(text):
    for pat in ATTRIBUTION:
        if re.search(pat, text, re.IGNORECASE):
            decide("deny", "Attribution rule (AGENTS.md): commits, pull requests, issues, comments and release notes carry no Claude/AI attribution or claude.ai links. "
                           f"Remove the text matching /{pat}/ and run it again.")


def check_attribution(cmd, segs, cwd):
    relevant, extra = False, ""
    for toks in segs:
        g = parse_git(toks, cwd)
        if g and g[0] in ("commit", "tag", "notes", "merge"):
            relevant = True
            args = g[1]
            for j, a in enumerate(args):
                if a in ("-F", "--file") and j + 1 < len(args):
                    extra += read_file(g[2], args[j + 1])
                elif a.startswith("--file="):
                    extra += read_file(g[2], a.split("=", 1)[1])
        if os.path.basename(toks[0]) == "gh" and len(toks) > 2 and toks[1] in ("pr", "issue", "release", "api"):
            relevant = True
            for j, a in enumerate(toks):
                if a in ("--body-file", "-F", "--notes-file") and j + 1 < len(toks):
                    extra += read_file(cwd, toks[j + 1].lstrip("@"))
    if relevant:
        check_attribution_text(cmd + "\n" + extra)


def check_push(args, repo):
    deny, force, positional = [], False, []
    i = 0
    while i < len(args):
        a = args[i]
        if a in ("--all", "--mirror", "--branches"):
            deny.append(a)
        elif a.startswith("--force") or a == "--force-if-includes":
            force = True
        elif a in ("-o", "--push-option", "--repo", "--receive-pack", "--exec"):
            i += 1
        elif a.startswith("--"):
            pass
        elif a.startswith("-") and len(a) > 1:
            if "f" in a[1:]:
                force = True
        else:
            positional.append(a)
        i += 1
    refspecs = positional[1:]
    current = git_out(repo, "rev-parse", "--abbrev-ref", "HEAD").strip()
    if not refspecs and current in PROTECTED:
        deny.append(f"current branch {current}")
    for r in refspecs:
        if r.startswith("+"):
            force = True
            r = r[1:]
        if r.startswith(":"):
            continue  # deleting a remote branch
        dst = r.split(":", 1)[1] if ":" in r else r
        if dst == "HEAD":
            dst = current
        dst = re.sub(r"^refs/heads/", "", dst)
        if dst in PROTECTED:
            deny.append(dst)
    if deny:
        decide("deny", f"Git flow (AGENTS.md): never push to dev or main; push a feature/<name> branch and open a pull request into dev. Blocked: {', '.join(deny)}.")
    if force:
        decide("ask", "Force push: needs the owner's explicit approval. Use --force-with-lease=<ref>:<sha> if approved.")


def short_flag(a, letter):
    return a.startswith("-") and not a.startswith("--") and letter in a[1:]


def check_add(args):
    for a in args:
        if a in ("-A", "--all", ".", "./", ":/", "*", "--no-ignore-removal") or short_flag(a, "A"):
            decide("deny", f"Staging (AGENTS.md): no blanket staging ('git add {a}'). Stage only the paths of this change: git add <path> ...")


def check_commit(args, repo):
    if any(a == "--all" or short_flag(a, "a") for a in args):
        decide("deny", "Staging (AGENTS.md): no 'git commit -a'. Stage only the paths of this change with git add <path>, then commit.")
    diff = git_out(repo, "diff", "--cached", "-U0", "--no-color")
    names = git_out(repo, "diff", "--cached", "--name-only")
    bad_files = sorted({n for n in names.splitlines() if SECRET_FILES.search(n)})
    if bad_files:
        decide("deny", f"Secret check: staged file(s) look like secrets: {', '.join(bad_files)}. Unstage them and gitignore them.")
    hard, soft, current = [], [], "?"
    for line in diff.splitlines():
        if line.startswith("+++ "):
            current = line[6:] if line.startswith("+++ b/") else line[4:]
            continue
        if not line.startswith("+"):
            continue
        hard += [f"{current}: {label}" for pat, label in SECRET_HARD if re.search(pat, line)]
        soft += [f"{current}: {label}" for pat, label in SECRET_SOFT if re.search(pat, line)]
    if hard:
        decide("deny", f"Secret check: the staged diff contains {'; '.join(sorted(set(hard))[:5])}. Remove it before committing.")
    if soft:
        decide("ask", f"Secret check: possible credential in the staged diff: {'; '.join(sorted(set(soft))[:5])}. Approve only if it is not a real secret.")


def check_config(args):
    if "--global" in args or "--system" in args:
        return
    values = [a for a in args if not a.startswith("-")]
    if len(values) >= 2 and re.fullmatch(r"user\.(name|email)", values[0]):
        decide("deny", "Identity (AGENTS.md): use the global git identity; never set a repo-local user.name or user.email.")


def check_worktree(args, cwd, project):
    if args[:1] != ["add"]:
        return
    paths, skip = [], False
    for a in args[1:]:
        if skip:
            skip = False
        elif a in ("-b", "-B", "--reason"):
            skip = True
        elif not a.startswith("-"):
            paths.append(a)
    if not paths:
        return
    path = os.path.realpath(os.path.join(cwd, os.path.expanduser(paths[0])))
    allowed = os.path.realpath(os.path.join(project, ".claude", "worktrees"))
    if not path.startswith(allowed + os.sep):
        decide("deny", "Worktrees (AGENTS.md): put them in .claude/worktrees/<name> of this repository.")


def check_bash(cmd, cwd, project):
    if "git" not in cmd and "gh" not in cmd:
        return
    segs = segments(cmd)
    check_attribution(cmd, segs, cwd)
    for toks in segs:
        if toks[0] == "cd" and len(toks) > 1:
            cwd = os.path.join(cwd, os.path.expanduser(toks[1]))
            continue
        if os.path.basename(toks[0]) == "gh" and toks[1:3] == ["pr", "merge"]:
            decide("ask", MERGE_REASON)
        g = parse_git(toks, cwd)
        if not g:
            continue
        sub, args, repo = g
        if sub == "push":
            check_push(args, repo)
        elif sub == "add":
            check_add(args)
        elif sub == "commit":
            check_commit(args, repo)
        elif sub == "config":
            check_config(args)
        elif sub == "worktree":
            check_worktree(args, cwd, project)


def check_edit(tool_input, project):
    path = tool_input.get("file_path") or tool_input.get("notebook_path") or ""
    if not path:
        return
    rel = os.path.relpath(os.path.realpath(path), os.path.realpath(project)).replace(os.sep, "/")
    if rel.startswith(".."):
        return
    for gen in GENERATED:
        if rel == gen or rel.startswith(gen):
            decide("deny", f"{rel} is generated (AGENTS.md): change its generator and regenerate.")


def strings(value):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for v in value.values():
            yield from strings(v)
    elif isinstance(value, list):
        for v in value:
            yield from strings(v)


def pre_tool_use(event):
    tool = event.get("tool_name", "")
    tool_input = event.get("tool_input") or {}
    cwd = event.get("cwd") or os.getcwd()
    project = os.environ.get("CLAUDE_PROJECT_DIR") or cwd
    if tool == "Bash":
        check_bash(tool_input.get("command") or "", cwd, project)
    elif tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
        check_edit(tool_input, project)
    elif GITHUB_MERGE.match(tool):
        decide("ask", MERGE_REASON)
    elif GITHUB_WRITES.match(tool):
        check_attribution_text("\n".join(strings(tool_input)))
        if tool in GITHUB_FILE_WRITES and re.sub(r"^refs/heads/", "", tool_input.get("branch", "")) in PROTECTED:
            decide("deny", "Git flow (AGENTS.md): dev and main take changes only through pull requests; write to a feature branch.")


def session_start(event):
    cwd = event.get("cwd") or os.getcwd()
    m = re.search(r"github\.com[:/]([^/]+/[^/]+?)(\.git)?$", git_out(cwd, "remote", "get-url", "origin").strip())
    if not m:
        return
    repo, counts = m.group(1), []
    for kind, label in (("code-scanning", "code scanning"), ("dependabot", "Dependabot")):
        out = subprocess.run(["gh", "api", f"repos/{repo}/{kind}/alerts?state=open&per_page=100", "--jq", "length"], capture_output=True, text=True, timeout=8)
        if out.returncode != 0:
            return
        counts.append(f"{out.stdout.strip()} open {label}")
    text = f"{repo}: {', '.join(counts)} alerts. Check them at the start and end of this work (AGENTS.md, Security alerts)."
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}))


def main():
    event = json.load(sys.stdin)
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "pre-tool-use":
        pre_tool_use(event)
    elif mode == "session-start":
        session_start(event)


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception:
        sys.exit(0)
