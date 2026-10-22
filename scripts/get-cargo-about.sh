#!/usr/bin/env bash
# Installs the pinned cargo-about (prebuilt binary, checked against its published sha256) and puts it on PATH for the
# steps after it. It makes THIRD-PARTY-NOTICES.md (app/scripts/third-party-notices.mjs). CI + release only.
# Bumping: new VERSION + the three sha256 of the assets named below (published beside each release).
set -euo pipefail

VERSION=0.9.2
case "${RUNNER_OS:?}-${RUNNER_ARCH:?}" in
  Windows-X64) target=x86_64-pc-windows-msvc; sha=1c03e5890238562497c2d89a3b75b02560af349c1fc3e713d3284f532a5cd748 ;;
  macOS-ARM64) target=aarch64-apple-darwin; sha=ae72f0df0c399a1e96336f696fa55b1b28679fd725632eba8cf8e4568467cc3e ;;
  Linux-X64) target=x86_64-unknown-linux-musl; sha=9099a59e820c38a68b9d65f300662a567d56562f9a10f6aa4c7e86c17c2566af ;;
  *) echo "no cargo-about build for ${RUNNER_OS}-${RUNNER_ARCH}"; exit 1 ;;
esac

dir="${RUNNER_TEMP:?}/cargo-about"
mkdir -p "$dir"
# Relative names from here on: GNU tar on Windows reads "D:\..." as a remote host.
cd "$dir"
curl -fsSL --retry 3 -o cargo-about.tar.gz \
  "https://github.com/EmbarkStudios/cargo-about/releases/download/${VERSION}/cargo-about-${VERSION}-${target}.tar.gz"
got=$(openssl dgst -sha256 -r cargo-about.tar.gz | cut -d' ' -f1)
if [ "$got" != "$sha" ]; then
  echo "unexpected sha256 for cargo-about ${VERSION} ${target}: $got"
  exit 1
fi
tar -xzf cargo-about.tar.gz
bin="$dir/cargo-about-${VERSION}-${target}"
if command -v cygpath >/dev/null 2>&1; then bin=$(cygpath -w "$bin"); fi
echo "$bin" >> "$GITHUB_PATH"
echo "cargo-about ${VERSION} ready"
