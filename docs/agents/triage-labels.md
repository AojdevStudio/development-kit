# Triage Labels

Issues in this repo carry two label dimensions: a **triage role** (where the
issue is in its lifecycle) and a **model route** (which model should pick it up).

## Triage roles

The skills speak in terms of five canonical triage roles. This repo uses the
canonical names verbatim.

| Canonical role     | Label in our tracker | Meaning                                  |
| ------------------ | -------------------- | ---------------------------------------- |
| `needs-triage`     | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`       | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`  | `ready-for-agent`    | Fully specified, AFK-ready for a coding agent |
| `ready-for-human`  | `ready-for-human`    | Requires human implementation            |
| `wontfix`          | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the
corresponding label string from this table.

## Model routes

Each actionable task is also tagged with the model that should run it. This
encodes the model stack as a routing dimension (mirrors the gitworkflow
`agent:<name>` pattern with a `model:` prefix so the labels group together).

| Label              | Route to            |
| ------------------ | ------------------- |
| `model:opus-4.8`   | Claude Opus 4.8     |
| `model:sonnet-4.6` | Claude Sonnet 4.6   |
| `model:gpt-5.5`    | GPT-5.5             |

A `ready-for-agent` issue should also carry exactly one `model:*` label so an
AFK agent knows which model to launch. If no model label is present, treat the
task as needing a triage decision on model routing first.

To add a new model to the stack, create the label
(`gh label create "model:<name>" --color <hex> --description "Route this task to <name>"`)
and add a row above.
