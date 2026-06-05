# Agent Workflow

How agents move work through this repo once the issue tracker is known.

## Source of truth

- **Issues:** GitHub Issues (`AojdevStudio/development-kit`)
- **Project board:** GitHub Project — `development-kit Backlog`, owner `AojdevStudio`, number `6`, <https://github.com/users/AojdevStudio/projects/6>
- **Default branch:** `main` (no `develop`; `main` is the integration branch)
- **Release surface:** none configured yet (see Release conventions)

GitHub Issues plus the linked GitHub Project are the default work hub. Keep
status there. Do not rely on local markdown task files.

## Operating loop

1. Start from the relevant issue, PRD, or user-approved spec.
2. If the work is not yet broken down, write a short plan/spec before creating implementation tickets.
3. Create or update issues in GitHub; attach each to project #6.
4. Set project status to `Todo` when issues are created and `In Progress` when implementation starts.
5. Work one issue at a time unless the user explicitly asks for parallel work or the issue set is already independent.
6. Branch with the gitworkflow convention (`feature/*` → PR into `main`).
7. Before opening or updating a PR, run the focused local checks and **self-review the diff**.
8. Open or update a PR that references the issue with a real closing keyword (`Closes #123`) and summarizes validation.
9. **Fetch review comments** with `gh`, fix valid ones, and reply to each resolved or rejected finding.
10. **Wait for and summarize automated reviews** before merging.
11. Merge and clean up branches only when the repo's merge policy allows it.
12. Update issue and project-board status before calling the work complete.
13. **On full task completion, run the code-simplify agent** as the final pass before reporting done.

## Review conventions

- **Self-review before PR:** yes — review the full diff before opening/updating.
- **Fetch & answer PR review comments:** yes — via `gh api repos/AojdevStudio/development-kit/pulls/<n>/comments` and `gh pr view <n> --json reviews`.
- **Automated review:** wait for and summarize automated reviewers (e.g. CodeRabbit, Codex, GitGuardian) before merge; allow ~240s settle time after checks pass.
- **Human review required before merge:** depends on branch protection on `main`.
- **Review replies:** when a reviewer raises a point, reply with what changed or why no change was made.

## Release conventions

Release/deploy workflows are part of the agent operating surface. There is no
release automation yet. When release work is requested, prefer the gitworkflow
Release workflow and document the exact command or tag pattern here.

- **Trigger:** none configured (proposed: tag push `v*` once CI exists)
- **Artifacts:** none yet (future: Tauri desktop bundles, Rust backend binary)
- **Release notes:** generate from conventional commits via the gitworkflow `changelog` tool

Do not create or modify release automation unless the user asks for release work
or the current issue explicitly requires it.

## Post-completion convention

After a full task is complete and merged, run the **code-simplify agent** over
the changed surface as a final cleanup pass. Treat this as a standing
expectation, not an optional step.
