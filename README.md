# Grove

A Rust git worktree runner — a faster, typed, native alternative to
[`gtr`](https://github.com/coderabbitai/git-worktree-runner), with first-class
**GitHub and Gitea** integration and (later) per-branch databases.

> A grove is a cluster of trees — which is what a set of git *worktrees* is.

## Status

**M1 and M2 (forge) complete.** Working today: `new` (with `--from`/
`--from-current`/`--remote`/`--track`/`--name`/`--no-fetch`/`--no-copy`/
`--no-hooks`/`-e`/`-a`, and PR/branch/issue refs: `#42`, `--pr`, `--issue`,
URLs), `list` (`--json` with PR state), `go`, `cd` (with an fzf picker), `run`,
`tasks`, `rm`, `mv`, `copy`, `clean --merged/--closed`, `pr`, `open`, `config`
(get/set/add/unset/list, `--local`/`--global`/`--system`), `trust`, `editor`,
`ai`, `adapter`, `init`, `doctor`, `version`. Native GitHub and Gitea clients
(tokens from `gh`/`tea`, no login step). Lifecycle hooks
(`postCreate`/`preRemove`/`postRemove`/`postCd`), a copy-on-write copy engine
(files, directories, `.worktreeinclude`) and opt-in btrfs subvolume worktrees
are wired in. Per-branch databases (`grove-db`) remain a later milestone. See
the design spec in the Obsidian vault (`igm/grove/…`).

```sh
cargo install grove-wt   # installs `grove` and `git-grove`
```

The CLI uses [`bpaf`](https://docs.rs/bpaf); shell completion is bpaf's
env-driven autocomplete (e.g. `COMPLETE=bash grove`), not a subcommand.

### Task graph

`grove new` runs a parallel task DAG declared in `grove.kdl` (replacing gtr's
serial `postCreate`). Independent tasks run concurrently; `needs` edges order
them; cycles and unknown deps are rejected; a failure skips its dependents.

```kdl
tasks {
    task "install" { run "bun install" }
    task "codegen" { run "bun run codegen"; needs "install" }
    task "build"   { run "cargo build";    needs "install" }
    task "test"    { run "cargo test";     needs "codegen" "build" }
}
```

`grove tasks` runs the graph in the current worktree (`--list`, `--concurrency
N`, `--keep-going`); `grove new --no-tasks` skips it.

A task with `jobs N` runs under [batonpass](https://github.com/macalinao/batonpass)
(`baton -n N`), taking its share of the machine's jobserver tokens; the count
it gets is in `$BATON_JOBS`. Without `baton` on `PATH` it runs directly.

```kdl
task "test" { run "cargo nextest run -j ${BATON_JOBS:-8}"; jobs 8 }
```

### Btrfs subvolume worktrees

On btrfs, each worktree can be its own subvolume, so checkouts and builds in
different worktrees don't contend on one fs-tree lock, and removing one is a
single `btrfs subvolume delete`. Off by default; turn it on at any git-config
scope (or `worktrees { subvolume #true }` in `grove.kdl`):

```sh
git config --system grove.worktrees.subvolume true   # or --global / --local
```

`grove new` then creates the worktree dir with `btrfs subvolume create` before
`git worktree add`; off btrfs it warns and uses a plain directory. `grove rm`
and `grove clean` delete a clean (or `--force`d) subvolume worktree with
`btrfs subvolume delete`, then unregister it. Without root that needs the
filesystem mounted with `user_subvol_rm_allowed`; otherwise git removes the
files one by one and `rmdir` drops the empty subvolume.

### Shell integration (`grove cd`)

A binary can't change its parent shell's directory, so `grove cd <name>` is a
shell function emitted by `grove init`:

```sh
eval "$(grove init zsh)"      # bash/zsh
grove init fish | source      # fish
grove cd my-feature           # jumps into that worktree
```

## Workspace layout

Published to crates.io as `grove-wt` / `grove-wt-*` (`grove`, `grove-cli` and
`grove-core` are taken there); in the code they're still `grove_*`.

| crate (dir) | crates.io | role |
|---|---|---|
| `grove-cli` | `grove-wt` | the `grove` and `git-grove` binaries (bpaf) |
| `grove-core` | `grove-wt-core` | worktree lifecycle: discover, name, create/remove/rename, subvolumes |
| `grove-git` | `grove-wt-git` | thin `git` process wrapper (porcelain parsing) |
| `grove-config` | `grove-wt-config` | layered config: `grove.kdl` + git-config (+ `.gtrconfig` compat) |
| `grove-tasks` | `grove-wt-tasks` | parallel task DAG |
| `grove-adapters` | `grove-wt-adapters` | editor + AI adapters |
| `grove-forge` | `grove-wt-forge` | GitHub / Gitea clients |
| `grove-shell` | `grove-wt-shell` | `grove init` shell integration |
| `grove-db` | (unpublished) | per-branch databases, supersedes `dbbranch` *(scaffold, M4)* |

## Develop

```sh
direnv allow         # optional: auto-load the dev shell on cd (nix-direnv)
nix develop          # rust toolchain + treefmt + pre-commit
cargo build
cargo test
nix build .#grove    # build the binary via crate2nix
nix fmt              # treefmt (rustfmt + nixfmt)
```

The Nix flake uses flake-parts, builds the package with crate2nix, and wires
treefmt + git-hooks. CI runs `nix flake check` and `nix build`.
Releases go through [release-plz](https://release-plz.dev): pushes to `master`
open a release PR (version bump + `CHANGELOG.md` from conventional commits);
merging it publishes to crates.io and tags `vX.Y.Z`.
`rust-toolchain.toml` pins the same rustc the flake ships, for rustup users and
editors outside `nix develop`.

## License

[Apache-2.0](LICENSE)
