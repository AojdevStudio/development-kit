# Hardened self-hosted CI runner recipe

This repo runs one check on every proposed change: `cargo xtask gate`, wired up
in `.github/workflows/gate.yml`. The default runner is GitHub-hosted
(`ubuntu-latest`), which is free for public repositories and needs nothing in
this document. This recipe is for the **opt-in** case only: if cloud minutes
ever become a real cost, you can point the same `gate` job at your own hardware
without touching the workflow logic or branch protection.

The repository is **public**. A self-hosted runner on a public repo is a
standing target: anyone can open a pull request, and a misconfigured runner will
happily execute attacker-supplied code (a `build.rs`, a test, a `cargo xtask`
step) on your machine. The whole point of this recipe is to make that execution
safe by default: ephemeral, isolated, secret-free, and (for outside
contributors) approval-gated. Do not run a self-hosted runner on this repo
without all four properties in place.

## How the swap works (and why branch protection never changes)

The job in `gate.yml` chooses its runner from a repository variable:

```yaml
jobs:
  gate:
    runs-on: ${{ vars.GATE_RUNNER || 'ubuntu-latest' }}
```

`vars.GATE_RUNNER` is a **repository variable** (not a secret), so it is safe to
read in any workflow and is never exposed to job steps as a credential. When it
is unset, the expression falls back to `ubuntu-latest` and the gate runs on
GitHub-hosted infrastructure. To move the gate onto a self-hosted runner, set
the variable to the labels that select your runner pool:

```bash
# Point the gate at a self-hosted runner pool labelled "gate-runner".
gh variable set GATE_RUNNER --repo AojdevStudio/development-kit --body "self-hosted,linux,x64,gate-runner"

# Revert to free GitHub-hosted runners at any time by deleting the variable.
gh variable delete GATE_RUNNER --repo AojdevStudio/development-kit
```

(You can also set it under Settings, Secrets and variables, Actions, Variables.)

The critical design property: the **job name stays `gate`** regardless of where
it runs. Branch protection on `main` requires a status check named `gate`
(see ADR-0002 below). Because that name is fixed, swapping the runner is a
one-line variable change. You never edit branch protection, never re-approve the
required check, and never risk a window where `main` is briefly unprotected
because the required check name drifted. Runner location and merge enforcement
are fully decoupled.

A useful consequence: if a self-hosted runner is offline or misbehaving, you
delete the variable, the gate immediately falls back to `ubuntu-latest`, and PRs
keep merging on green. The required check never disappears, so the protection
on `main` is continuous across the swap.

## Property 1: Ephemeral runners (no state leaks between jobs)

Run every runner as **ephemeral**: it accepts exactly one job, runs it on a
fresh VM or container, and then deregisters and is destroyed. The next job gets a
brand-new environment. Nothing (no clone, no cargo cache poisoned by a malicious
`build.rs`, no leftover process, no written file) survives from one job to the
next.

Register an ephemeral runner with the `--ephemeral` flag:

```bash
# Obtain a short-lived registration token (expires fast; do not store it).
REG_TOKEN=$(gh api -X POST repos/AojdevStudio/development-kit/actions/runners/registration-token --jq .token)

./config.sh \
  --url https://github.com/AojdevStudio/development-kit \
  --token "$REG_TOKEN" \
  --labels "self-hosted,linux,x64,gate-runner" \
  --ephemeral \
  --unattended \
  --name "gate-$(uuidgen)"

./run.sh   # runs one job, then exits
```

Because `--ephemeral` exits after a single job, drive it from a supervisor that
rebuilds the environment each time. The robust pattern is one disposable
container or microVM per job:

```bash
#!/usr/bin/env bash
# gate-runner-loop.sh: one fresh, throwaway container per job, forever.
set -euo pipefail
IMAGE="gate-runner:latest"   # pinned image holding the runner agent + build deps

while true; do
  REG_TOKEN=$(gh api -X POST \
    repos/AojdevStudio/development-kit/actions/runners/registration-token \
    --jq .token)

  docker run --rm \
    --user 10001:10001 \
    --read-only \
    --tmpfs /home/runner:exec \
    --cap-drop ALL \
    --security-opt no-new-privileges \
    --network gate-egress \
    -e REG_TOKEN \
    "$IMAGE"
  # --rm + --ephemeral inside => container and runner are gone after one job.
done
```

Do not reuse a long-lived runner that picks up job after job. A persistent runner
on a public repo means job N can plant something that job N+1 (possibly a
maintainer's trusted PR) silently inherits.

## Property 2: Isolation (unprivileged, no host mounts, egress limits)

Assume every job runs hostile code and contain the blast radius.

- **Dedicated unprivileged user.** Never run the runner as root or as your own
  login user. Create a throwaway account with no sudo and no access to anything
  else on the box:

  ```bash
  sudo useradd --system --create-home --shell /usr/sbin/nologin gate-runner
  # Install and run the runner agent strictly as this user.
  ```

  In containers, the equivalent is `--user 10001:10001` (a non-root UID) plus
  `--cap-drop ALL` and `--security-opt no-new-privileges`, as shown above.

- **Rootless container or microVM.** Prefer rootless Podman, a rootless Docker
  setup, or a microVM (Firecracker, a throwaway cloud-init VM) so a container
  escape does not land on a privileged host. Drop all Linux capabilities the
  build does not need.

- **No host mounts.** Do not bind-mount host paths into the runner (`-v
  /:/host`, the Docker socket `/var/run/docker.sock`, your home directory, SSH
  keys, cloud credential files). The gate only needs the checked-out repo, which
  Actions provides inside the workspace. Mounting the Docker socket in
  particular is equivalent to giving the job root on the host: never do it.

- **Network egress limits.** The gate clones the repo, may pull pinned
  toolchains and crates, and otherwise should not phone home. Default-deny
  outbound traffic and allow only what the build needs (GitHub, your crate
  registry mirror, the package mirrors the toolchain steps use). A custom Docker
  network plus host firewall rules, or an egress proxy that allowlists known
  hosts, both work:

  ```bash
  # Example: a dedicated bridge network you attach the runner container to,
  # with host nftables/iptables rules that default-deny egress from its subnet
  # and allow only the registries and GitHub endpoints the build requires.
  docker network create gate-egress
  ```

  Tightening egress is the single most effective control against a malicious PR
  exfiltrating data or pivoting into your network from the runner.

- **Host placement.** Put the runner on an isolated VLAN or a cloud VM that
  cannot reach your homelab, NAS, or other trusted hosts. Treat the runner box
  as compromised-by-default and make sure that compromise stays contained.

## Property 3: Secret-free (the gate needs no secrets)

`cargo xtask gate` only builds and tests the repository. It compiles Rust, runs
clippy and rustfmt, runs the workspace and frontend tests, checks migrations,
and runs the dependency/leak/edge checks. **None of that requires a repository
or organization secret, a Stripe key, a database password, or a deploy
credential.** This is by design and matches ADR-0001: no Stripe, DB, webhook, or
signing secrets exist on the client/CI build path.

Keep it that way:

- **Do not** add `secrets.*` references to `gate.yml`. The job declares
  `permissions: contents: read` and needs nothing more.
- **Do not** put any secret on the runner host (no `.env`, no cloud credential
  files, no SSH keys to other machines, no registry tokens beyond an anonymous
  read mirror).
- **Do not** reuse this runner for deploy or release jobs that *do* need
  secrets. If a future job needs credentials, it must run on a **separate,
  GitHub-hosted** runner (or a separate, non-public, approval-only pool), never
  on the public-PR gate runner.

A secret that is never present on the runner cannot be stolen by a malicious
pull request. The gate's secret-free design is what makes running untrusted fork
code on it tolerable at all.

## Property 4: Approval-gated for outside contributors

Even ephemeral and isolated, a self-hosted runner must not automatically execute
code from forks opened by people you do not trust. GitHub's default for public
repos already requires approval for first-time contributors; tighten it to cover
**all** outside collaborators:

1. Go to **Settings, Actions, General**.
2. Under **Fork pull request workflows from outside collaborators**, select
   **Require approval for all outside collaborators**.
3. Save.

With this set, a fork PR from anyone who is not a repo collaborator will **not**
start workflow runs (including the gate) until a maintainer clicks
**Approve and run**. On a public repo this is non-negotiable for self-hosted
hardware: without it, any stranger's PR would auto-execute on your machine the
instant they open it. Review the diff before approving, and only approve runs
whose changes you have actually read.

GitHub-hosted runs do not need this control for safety (they run on disposable
GitHub infrastructure), but it is still good hygiene and costs nothing to leave
on.

## Relationship to ADR-0002 and the default posture

ADR-0002 establishes **mechanical enforcement**: the project's quality and
authority rules are enforced by a single gate (`cargo xtask gate`) that runs in
CI on every PR, and branch protection makes a red gate physically block merge.
No human has to remember to run the checks; the machine does, and a failing
check stops the merge with no discretion involved. The fixed `gate` job name and
the `GATE_RUNNER` indirection in `gate.yml` are what let that enforcement stay
stable while the *location* of execution is a swappable detail.

This document does **not** change the default. The default runner is and remains
GitHub-hosted (`ubuntu-latest`), which is free for this public repo and requires
zero infrastructure, zero maintenance, and zero added attack surface. Self-hosted
is an **opt-in escape hatch** for one reason only: if cloud minutes ever become a
cost worth optimizing. Until then, leave `GATE_RUNNER` unset and run nothing of
your own. If you do opt in, all four properties above (ephemeral, isolated,
secret-free, approval-gated) are mandatory, not optional, because the repo is
public and the gate executes contributor-supplied code.
