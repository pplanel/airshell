# Make arrakis the cache builder (self-hosted runner)

This is the concrete, arrakis-specific version of
[`self-hosted-cache-runner.md`](./self-hosted-cache-runner.md). Follow it end to end and the
`Publish Binary Cache` workflow will build airshell on arrakis and push to
`ghcr.io/pplanel/airshell/nix-cache`, so caladan and imperium can substitute instead of compiling.

## Why arrakis

airshell's flake build is **impure** (`flake.nix`: `__noChroot` + Xcode's `clang`/`swift`/`xcrun` and
the macOS 26 SDK, because `networkframework`'s `build.rs` compiles a Swift bridge). It only builds on
an Apple-Silicon Mac that has **full Xcode.app** and runs Nix with the **sandbox off**. GitHub-hosted
`macos` runners build sandboxed against Nix's own SDK and fail; imperium has only Command Line Tools
(no Xcode.app) and also fails.

arrakis was verified to meet every requirement and to build the flake cleanly:

| Requirement | arrakis | imperium |
|---|---|---|
| Apple Silicon (`arm64`) | ✅ | ✅ |
| Full **Xcode.app** | ✅ `/Applications/Xcode.app` | ❌ CLT only |
| Nix `sandbox` | ✅ `false` | ✅ `false` |
| `trusted-users` has you | ✅ `root pplanel @admin` | ✅ |
| `nix build .#airshell-proxy` | ✅ builds | ❌ (no Xcode.app) |

The workflow already targets the custom runner label **`airshell-builder`** (see
`.github/workflows/publish-cache.yml`); the steps below give arrakis that label.

## Security (already safe)

`publish-cache.yml` triggers only on `push` to `main`, `v*` tags, and manual `workflow_dispatch` —
there is **no `pull_request` trigger**, so forks of this public repo can never run jobs on arrakis.
Keep it that way. The runner still executes anything merged to `main`, so treat arrakis accordingly.

## Setup (run on arrakis)

Do this once, logged in as `pplanel` on arrakis (its Nix has the sandbox off and sees Xcode).

### 1. Get a registration token

From any machine with `gh` (repo admin on `pplanel/airshell`):

```bash
gh api -X POST repos/pplanel/airshell/actions/runners/registration-token --jq .token
```

The token is single-use and expires in ~1 hour. Copy it.

### 2. Download the runner (on arrakis)

```bash
mkdir -p ~/actions-runner && cd ~/actions-runner
VER=$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p')
curl -fsSL -o runner.tar.gz \
  "https://github.com/actions/runner/releases/download/v${VER}/actions-runner-osx-arm64-${VER}.tar.gz"
tar xzf runner.tar.gz && rm runner.tar.gz
```

### 3. Configure it with the `airshell-builder` label

```bash
cd ~/actions-runner
./config.sh --url https://github.com/pplanel/airshell \
  --token "<PASTE_TOKEN_FROM_STEP_1>" \
  --labels airshell-builder \
  --name arrakis-airshell \
  --unattended --replace
```

### 4. Run it as a launchd service (survives logout/reboot)

```bash
./svc.sh install
./svc.sh start
./svc.sh status        # should show "started"
```

arrakis already stays awake (your `modules/darwin/power.nix` sets `disablesleep`/`autorestart`), so
it'll remain reachable as a runner.

### 5. Sanity-check the build environment

```bash
nix show-config | grep -E '^sandbox'          # sandbox = false
xcode-select -p                                # /Applications/Xcode.app/Contents/Developer
cd ~/src/airshell && scripts/build-mac.sh      # must succeed locally (it does today)
```

## Trigger and verify the cache publish

From anywhere:

```bash
# Confirm the runner is online with the right label:
gh api repos/pplanel/airshell/actions/runners --jq '.runners[] | {name, status, labels:[.labels[].name]}'

# Kick a run (or just push to main):
gh workflow run "Publish Binary Cache" -R pplanel/airshell

# Watch it:
RID=$(gh run list -R pplanel/airshell --workflow "Publish Binary Cache" --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$RID" -R pplanel/airshell --exit-status
```

> **nix-installer caveat.** The reusable `pplanel/nixcache` workflow runs
> `DeterminateSystems/nix-installer-action` at the start of each job. arrakis already has Nix, so
> watch the first run's **Install Nix** step. If it errors on the existing install, the fix is in the
> `pplanel/nixcache` reusable workflow (skip install when Nix is present), not in this repo.

## After the first green run

1. Make the GHCR package public: GitHub → your profile → **Packages** → `airshell/nix-cache` →
   **Package settings** → **Change visibility** → Public. (Clients pull anonymously.)
2. In `~/.config/nix-darwin`, the consumer entry is already staged in
   `modules/darwin/nixcache.nix` (name `airshell`, repo `pplanel/airshell`, port **37517**, the
   `airshell-cache-1:…` public key). Commit the nix-darwin changes and
   `darwin-rebuild switch --flake ~/.config/nix-darwin#<host>` on each Mac.
3. caladan and especially **imperium** (which can't build airshell locally) will now **substitute**
   the prebuilt binaries from the cache instead of compiling.

## Teardown (if you ever move the builder)

```bash
cd ~/actions-runner
./svc.sh stop && ./svc.sh uninstall
TOKEN=$(gh api -X POST repos/pplanel/airshell/actions/runners/remove-token --jq .token)
./config.sh remove --token "$TOKEN"
```

## State of this work (for reference)

Already done:
- `NIX_SIGNING_KEY` secret set on `pplanel/airshell`; private key stored in 1Password **Personal**
  (`airshell nix cache signing key (airshell-cache-1)`).
- Committed to this repo and pushed: `.github/workflows/publish-cache.yml` (targets
  `airshell-builder`), `public-key.txt`, and the runner docs.
- Staged (uncommitted) in `~/.config/nix-darwin`: the `airshell` flake input (no `follows`, locked)
  and the `nixcache.nix` consumer entry on port 37517.

Still to do: register the runner on arrakis (this doc) → first green run → make the package public →
commit + `darwin-rebuild switch` the nix-darwin changes.

---

## Appendix: alternative — Nix distributed builds

If you'd rather not run a GitHub runner at all, arrakis can serve as a **Nix remote builder** so
caladan/imperium delegate the impure airshell build to it over SSH:

```nix
# on caladan / imperium, in nix-darwin:
nix.buildMachines = [{
  hostName = "arrakis";            # reachable over your tailnet
  systems = ["aarch64-darwin"];
  maxJobs = 4;
  protocol = "ssh-ng";
  sshUser = "pplanel";
  sshKey = "/var/root/.ssh/id_builder";   # a key root can read; arrakis must trust it
}];
nix.distributedBuilds = true;
nix.settings.builders-use-substitutes = true;
```

This avoids GHCR entirely but requires root-usable SSH keys and arrakis staying online. The binary
cache is usually the better fit for three machines because it decouples consumers from the builder
being reachable at build time — which is the whole point when you're off-grid.
