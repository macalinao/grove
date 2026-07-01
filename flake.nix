{
  description = "Grove — a Rust git worktree runner (a gtr alternative)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crate2nix = {
      url = "github:nix-community/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks.flakeModule
      ];

      perSystem =
        {
          config,
          self',
          pkgs,
          system,
          ...
        }:
        let
          # crate2nix generates a Cargo.nix from Cargo.lock at eval time (IFD)
          # and builds each workspace member as its own derivation.
          cargoNix = inputs.crate2nix.tools.${system}.appliedCargoNix {
            name = "grove";
            src = ./.;
          };

          rustTools = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.rust-analyzer
          ];
        in
        {
          packages.default = self'.packages.grove;
          packages.grove = cargoNix.workspaceMembers.grove-cli.build;

          # The upstream gtr CLI, pinned, for the grove-vs-gtr differential
          # tests. Exposed as a package and put on the dev shell PATH so
          # `cargo test` can run those tests reproducibly via the flake.
          packages.git-worktree-runner = pkgs.callPackage ./nix/packages/git-worktree-runner.nix { };

          # `nix flake check` builds the binary (fmt + git-hooks checks are
          # contributed by the treefmt-nix / git-hooks flake modules). The
          # workspace test suite runs as a dedicated CI step via `cargo test`
          # in the dev shell, which covers every crate (not just grove-cli).
          checks.grove = self'.packages.grove;

          # treefmt-nix: one formatter for Rust + Nix, exposed as `nix fmt`.
          treefmt = {
            projectRootFile = "flake.nix";
            programs.rustfmt.enable = true;
            programs.nixfmt.enable = true;
          };

          # git-hooks.nix: format on commit, lint before push.
          pre-commit.settings.hooks = {
            # treefmt (rustfmt + nixfmt) runs on every commit — cheap.
            treefmt = {
              enable = true;
              package = config.treefmt.build.wrapper;
            };

            # Clippy is a full workspace typecheck, so it's too heavy to run
            # on every commit — scope it to `pre-push`. It uses the same
            # cargo/clippy as the dev shell (pinned via the flake's nixpkgs),
            # so hook, editor, and CI all agree. Mirrors the CI clippy step.
            clippy = {
              enable = true;
              stages = [ "pre-push" ];
              packageOverrides = {
                cargo = pkgs.cargo;
                clippy = pkgs.clippy;
              };
              settings = {
                # Match CI: lint all workspace crates and all targets
                # (tests, benches, examples), not just the default lib/bin.
                extraArgs = "--workspace --all-targets";
                # denyWarnings stays false: let the workspace
                # `[workspace.lints.clippy]` table decide. Its deny-level
                # lints (complexity, unwrap_used, std_instead_of_core, …)
                # already fail the push; the ~72 pedantic entries remain
                # warnings. Flip this to true only after the pedantic
                # baseline is cleaned up.
                denyWarnings = false;
              };
            };
          };

          devShells.default = pkgs.mkShell {
            packages = rustTools ++ [
              pkgs.git
              config.treefmt.build.wrapper
              # gtr on PATH enables the differential tests (skipped otherwise).
              self'.packages.git-worktree-runner
            ];
            shellHook = ''
              ${config.pre-commit.installationScript}
            '';
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
        };
    };
}
