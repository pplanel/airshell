# Self-hosted runner for the binary cache

`.github/workflows/publish-cache.yml` builds airshell's `aarch64-darwin` outputs and pushes them to
`ghcr.io/pplanel/airshell/nix-cache`. That build is **impure** — `flake.nix` uses `__noChroot` plus
Xcode's `clang`/`swift`/`xcrun` and the macOS 26 SDK, because `networkframework`'s `build.rs`
compiles a Swift bridge. GitHub-hosted `macos` runners build in a clean/sandboxed Nix environment
where that routing never reaches Xcode, so the Swift bridge compiles against Nix's `apple-sdk` 14,
the macOS 26 Network.framework symbols are missing, and the build fails.

So the cache must be built on a **self-hosted Apple-Silicon Mac set up like the dev machines**:
Xcode installed, and Nix with the sandbox off (`nix show-config | grep ^sandbox` → `false`) so
`__noChroot` derivations can reach `/usr/bin`. If `scripts/build-mac.sh` works on the machine, the
runner will too. `imperium`, `caladan`, or `arrakis` all qualify.

The workflow targets the custom runner label **`airshell-builder`** (`with: runs-on:
airshell-builder`).

## Security: public repo + self-hosted runner

GitHub [warns against self-hosted runners on public repositories](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/about-self-hosted-runners#self-hosted-runner-security):
a pull request from a fork could otherwise run arbitrary code on your machine. This workflow is safe
because it triggers **only** on `push` to `main`, on `v*` tags, and on manual `workflow_dispatch` —
there is **no `pull_request` trigger**, so a fork can never cause a job to run on the runner. Keep it
that way: do not add `pull_request`/`pull_request_target` to this workflow. Still treat the runner
host as able to execute anything that lands on `main`.

## Register the runner (once, on the chosen Mac)

Run these on the Mac itself (it needs Xcode + sandbox-off Nix):

```bash
# 1. Get a short-lived registration token (needs gh with repo admin on pplanel/airshell):
TOKEN=$(gh api -X POST repos/pplanel/airshell/actions/runners/registration-token --jq .token)

# 2. Download the current macOS arm64 runner into ~/actions-runner:
mkdir -p ~/actions-runner && cd ~/actions-runner
RUNNER_VER=$(gh api repos/actions/runner/releases/latest --jq '.tag_name' | sed 's/^v//')
curl -fsSL -o runner.tar.gz \
  "https://github.com/actions/runner/releases/download/v${RUNNER_VER}/actions-runner-osx-arm64-${RUNNER_VER}.tar.gz"
tar xzf runner.tar.gz && rm runner.tar.gz

# 3. Configure it against the repo with the label the workflow expects:
./config.sh --url https://github.com/pplanel/airshell \
  --token "$TOKEN" \
  --labels airshell-builder \
  --name "$(hostname -s)-airshell" \
  --unattended --replace

# 4. Run it as a login-scoped launchd service so it survives logout/reboot:
./svc.sh install
./svc.sh start
./svc.sh status
```

The runner service runs as the installing user, so it inherits that user's Nix (sandbox off) and
Xcode — the same environment `scripts/build-mac.sh` relies on.

### Verify the environment before trusting a build

```bash
nix show-config | grep -E '^sandbox'        # want: sandbox = false (or relaxed)
xcode-select -p                              # Xcode toolchain present
cd /path/to/airshell && scripts/build-mac.sh # must succeed locally first
```

## Caveat: the nix installer step on an already-Nix machine

The reusable `pplanel/nixcache` workflow runs `DeterminateSystems/nix-installer-action` at the start
of every job. On a machine that already has (Determinate) Nix, the action normally detects the
existing install and continues, but watch the first run's "Install Nix" step. If it conflicts or
tries to reinstall, the fix lives in `pplanel/nixcache`'s reusable workflow (make the install step a
no-op when Nix is already present), not here.

## Trigger a build

Push to `main`, or run it on demand:

```bash
gh workflow run "Publish Binary Cache" -R pplanel/airshell
gh run watch "$(gh run list -R pplanel/airshell --workflow 'Publish Binary Cache' --limit 1 --json databaseId --jq '.[0].databaseId')" -R pplanel/airshell
```

After the first green run, make the `nix-cache` GHCR package **public** (GitHub → your packages →
`airshell/nix-cache` → Package settings → Change visibility) so the three Macs pull anonymously, then
`darwin-rebuild switch` on each to start substituting instead of building locally.
