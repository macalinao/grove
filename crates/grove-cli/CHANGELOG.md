# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/macalinao/grove/releases/tag/v0.1.0) - 2026-11-14

### Added

- *(core)* grove new --detach; grove trust approves the current worktree's grove.kdl ([#31](https://github.com/macalinao/grove/pull/31))
- *(tasks)* jobs N runs a task under batonpass's baton -n N
- *(core)* opt-in btrfs subvolume worktrees (grove.worktrees.subvolume)
- *(forge)* grove list --json annotates worktrees with PR number/state/url ([#19](https://github.com/macalinao/grove/pull/19))
- *(forge)* grove pr / grove open open compare, branch & PR URLs ([#18](https://github.com/macalinao/grove/pull/18))
- *(forge)* address worktrees by PR/branch/issue ref in new/go/cd ([#17](https://github.com/macalinao/grove/pull/17))
- *(forge)* native Gitea provider via /api/v1 HTTP client ([#15](https://github.com/macalinao/grove/pull/15))
- *(forge)* native GitHub HTTP client + zero-login gh/tea token resolvers ([#14](https://github.com/macalinao/grove/pull/14))
- *(cli)* grove list --json and a progress spinner for grove clean
- *(cli)* git-grove subcommand + working `cd` tab completion
- *(config)* add `grove config migrate` to convert .gtrconfig → grove.kdl
- *(new)* gtr-exact --track semantics; ui.color config; fill test gaps
- *(shell)* new --cd, init --as <name>, fzf picker keybindings
- *(adapters)* match gtr's full editor/AI registry + none + feature flags
- *(copy)* real copy-on-write via reflink-copy crate + CoW detection
- *(parity)* '1' main-repo alias + custom editor/ai adapters via config
- *(parity)* grove copy uses full copy engine (dirs + .worktreeinclude)
- *(parity)* fzf picker for grove cd + gtr-style list --porcelain
- *(parity)* forge integration via gh/glab + grove clean
- *(parity)* hooks, confirmations, config scopes, adapter/completion, sanitize
- *(parity)* expand config, copy engine, and grove new toward gtr parity
- *(copy)* smart file copy engine + grove copy command ([#9](https://github.com/macalinao/grove/pull/9))
- *(trust)* trust model + grove trust ([#8](https://github.com/macalinao/grove/pull/8))
- *(cli)* grove config get/set/list/unset ([#7](https://github.com/macalinao/grove/pull/7))
- *(adapters)* editor/ai adapters + grove editor/ai commands ([#6](https://github.com/macalinao/grove/pull/6))
- *(tasks,shell)* parallel task graph + grove cd shell integration
- *(tasks)* scaffold grove-tasks crate with frozen contracts
- *(cli)* add `version` subcommand (bpaf)

### Fixed

- *(config)* read real .gtrconfig schema (was reading wrong keys)

### Other

- *(readme)* crates are not on crates.io yet
- Apache-2.0 license, crates.io metadata and release-plz
- *(lint)* make clippy -D warnings green across the workspace
- use shlex for adapter commands, tempfile for test dirs
- *(new)* cover --force same-branch multi-worktree
- grove-vs-gtr differential suite; align sanitize + porcelain status
- behavioral integration suite driving the grove binary vs gtr semantics
- treefmt master (fix #5/#6 formatting) ([#10](https://github.com/macalinao/grove/pull/10))
- Initial scaffold: Grove M1 worktree runner
