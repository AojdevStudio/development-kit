# Issue tracker: GitHub

Issues and PRDs for this repo live as GitHub issues in `AojdevStudio/development-kit`.
Use the `gh` CLI for all operations. GitHub is the source of truth for work
tracking; do not create local markdown task files.

## Prerequisites

- Run `gh auth status` before attempting writes (currently authed as `AojdevStudio`).
- If GitHub auth is missing or expired, stop and report the auth blocker instead of falling back to local markdown.
- `gh` infers the repo from `git remote -v` when run inside the clone.
- Check `docs/agents/workflow.md` before creating issues so project-board, PR, review, and release conventions stay in sync.
- Every new issue must be added to the linked **development-kit Backlog** project (User project #6).

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`. Use a heredoc for multi-line bodies.
- **Read an issue**: `gh issue view <number> --comments`.
- **List issues**: `gh issue list --state open --json number,title,body,labels,comments --jq '[.[] | {number, title, body, labels: [.labels[].name], comments: [.comments[].body]}]'` with appropriate `--label` and `--state` filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply / remove labels**: `gh issue edit <number> --add-label "..."` / `--remove-label "..."`
- **Add to project board**: `gh project item-add 6 --owner AojdevStudio --url <issue-url>`
- **Close**: `gh issue close <number> --comment "..."`

## Label dimensions

Every actionable issue should carry two label dimensions (see `docs/agents/triage-labels.md`):

1. A **triage role** — `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, or `wontfix`.
2. A **model route** — `model:opus-4.8`, `model:sonnet-4.6`, or `model:gpt-5.5`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue and add it to project #6.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.
