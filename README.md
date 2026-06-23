# Grove

A Rust git worktree runner — a faster, typed, native alternative to
[`gtr`](https://github.com/coderabbitai/git-worktree-runner), with first-class
**GitHub and Gitea** integration and (later) per-branch databases.

> A grove is a cluster of trees — which is what a set of git *worktrees* is.

## Status

**M1 in progress.** Working today: `new`, `list`, `go`, `run`, `rm`, `mv`,
`doctor`. Forge integration (`clean --merged`, `pr`), editor/AI adapters,
`copy`, the task graph, and per-branch databases are scaffolded and land in
later milestones. See the design spec in the Obsidian vault (`igm/grove/…`).

The CLI uses [`bpaf`](https://docs.rs/bpaf); shell completion is bpaf's
env-driven autocomplete (e.g. `COMPLETE=bash grove`), not a subcommand.

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
