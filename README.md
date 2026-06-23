# Grove

A Rust git worktree runner — a faster, typed, native alternative to
[`gtr`](https://github.com/coderabbitai/git-worktree-runner), with first-class
**GitHub and Gitea** integration and (later) per-branch databases.

> A grove is a cluster of trees — which is what a set of git *worktrees* is.

## Status

**M1 in progress.** Working today: `new`, `list`, `go`, `cd`, `run`, `tasks`,
`rm`, `mv`, `init`, `doctor`, `version`. Forge integration (`clean --merged`,
`pr`), editor/AI adapters, `copy`, and per-branch databases land in later
milestones. See the design spec in the Obsidian vault (`igm/grove/…`).

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

### Shell integration (`grove cd`)

A binary can't change its parent shell's directory, so `grove cd <name>` is a
shell function emitted by `grove init`:

```sh
eval "$(grove init zsh)"      # bash/zsh
grove init fish | source      # fish
grove cd my-feature           # jumps into that worktree
```

## Workspace layout

| crate | role |
|---|---|
| `grove-cli` | the `grove` binary (clap) |
| `grove-core` | worktree lifecycle: discover, name, create/remove/rename |
| `grove-git` | thin `git` process wrapper (porcelain parsing) |
| `grove-config` | layered config: `grove.kdl` + git-config (+ `.gtrconfig` compat) |
| `grove-adapters` | editor + AI adapters *(scaffold)* |
| `grove-forge` | GitHub / Gitea / GitLab clients *(scaffold, M2)* |
| `grove-shell` | `grove init` shell integration *(scaffold, M3)* |
| `grove-db` | per-branch databases, supersedes `dbbranch` *(scaffold, M4)* |

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
`rust-toolchain.toml` pins the same rustc the flake ships, for rustup users and
editors outside `nix develop`.

## License

MIT OR Apache-2.0
