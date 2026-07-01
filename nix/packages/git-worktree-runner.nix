# git-worktree-runner (gtr) — the bash CLI Grove reimplements.
#
# Packaged so the flake can provide a reproducible `gtr` binary for the
# differential test suite (grove vs gtr), pinned to the same commit the parity
# work was compared against. Not a Grove dependency at runtime.
{
  lib,
  stdenvNoCC,
  fetchFromGitHub,
  makeWrapper,
  bash,
  git,
  coreutils,
  gnused,
  gawk,
  fzf,
  gh,
  glab,
  jq,
}:
stdenvNoCC.mkDerivation (finalAttrs: {
  pname = "git-worktree-runner";
  version = "2.8.0";

  src = fetchFromGitHub {
    owner = "coderabbitai";
    repo = "git-worktree-runner";
    rev = "ad7a3c534fc36e6adfee44c480b03c2f7f959502";
    hash = "sha256-QJcpDq9guLgVjWhF2HwpMxK5ONQZtbDxFZnQrQjCRtI=";
  };

  nativeBuildInputs = [ makeWrapper ];

  # Pure-bash project: copy the tree and wrap the entry points with their
  # runtime tools on PATH and GTR_DIR pinned to the installed location.
  installPhase = ''
    runHook preInstall

    mkdir -p $out/share/git-worktree-runner $out/bin
    cp -r bin lib completions adapters templates $out/share/git-worktree-runner/

    for entry in gtr git-gtr; do
      makeWrapper ${bash}/bin/bash $out/bin/$entry \
        --add-flags "$out/share/git-worktree-runner/bin/git-gtr" \
        --set GTR_DIR "$out/share/git-worktree-runner" \
        --prefix PATH : ${
          lib.makeBinPath [
            git
            coreutils
            gnused
            gawk
            fzf
            gh
            glab
            jq
          ]
        }
    done

    runHook postInstall
  '';

  meta = {
    description = "Git worktree runner (gtr) — bash CLI, pinned for Grove's differential tests";
    homepage = "https://github.com/coderabbitai/git-worktree-runner";
    license = lib.licenses.mit;
    mainProgram = "gtr";
    platforms = lib.platforms.unix;
  };
})
