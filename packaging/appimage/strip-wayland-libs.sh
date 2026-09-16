#!/usr/bin/env bash
# Removes the Wayland / xkbcommon / xcb libraries bundled into a Tauri AppImage and repacks it.
#
# Why: linuxdeploy copies libwayland-*, libxkbcommon* and libxcb* from the build host
# (ubuntu-22.04). On hosts with Wayland + Mesa 25+ (Ubuntu 24.04+, Fedora 44, Arch) those
# old copies make EGL abort (EGL_BAD_PARAMETER) and the window never renders. Using the
# host libraries fixes it. Upstream: https://github.com/tauri-apps/tauri/issues/15665,
# https://github.com/tauri-apps/tauri/issues/15976 (PR #15662 not merged as of 2026-09).
#
# Usage: strip-wayland-libs.sh <path/to/App.AppImage>
# The AppImage is replaced in place. Running it again is a no-op (idempotent).
#
# Env:
#   APPIMAGETOOL   path to an appimagetool binary (skips the download)
#   KEEP_WORKDIR=1 keep the temporary extraction directory for inspection
set -euo pipefail

readonly APPIMAGETOOL_VERSION="1.9.1"
readonly APPIMAGETOOL_SHA256_X86_64="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
readonly LIB_PATTERNS=('libwayland-*.so*' 'libxkbcommon*.so*' 'libxcb*.so*')

log() { printf '[strip-wayland-libs] %s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

[[ $# -eq 1 ]] || die "usage: $0 <path/to/App.AppImage>"
[[ -f $1 ]] || die "file not found: $1"
[[ $(uname -m) == "x86_64" ]] || die "only x86_64 is supported (pinned appimagetool checksum)"

appimage="$(realpath "$1")"
chmod +x "$appimage"

workdir="$(mktemp -d)"
cleanup() {
  if [[ ${KEEP_WORKDIR:-0} == 1 ]]; then
    log "workdir kept at $workdir"
  else
    rm -rf "$workdir"
  fi
}
trap cleanup EXIT

# Run AppImages without FUSE (CI containers usually lack it).
export APPIMAGE_EXTRACT_AND_RUN=1

cd "$workdir"
"$appimage" --appimage-extract >/dev/null
[[ -d squashfs-root ]] || die "extraction failed: squashfs-root missing"

find_args=()
for pattern in "${LIB_PATTERNS[@]}"; do
  [[ ${#find_args[@]} -gt 0 ]] && find_args+=(-o)
  find_args+=(-name "$pattern")
done

mapfile -t libs < <(find squashfs-root \( -type f -o -type l \) \( "${find_args[@]}" \) -print | sort)

if [[ ${#libs[@]} -eq 0 ]]; then
  log "no bundled Wayland/xkbcommon/xcb libraries found; nothing to do"
  exit 0
fi

log "removing ${#libs[@]} bundled libraries:"
for lib in "${libs[@]}"; do
  log "  ${lib#squashfs-root/}"
  rm -f -- "$lib"
done

# Reuse the runtime of the original AppImage so the repack works offline and keeps
# the same runtime type.
offset="$("$appimage" --appimage-offset)"
[[ $offset =~ ^[0-9]+$ ]] || die "could not read the AppImage runtime offset"
head -c "$offset" "$appimage" >runtime

tool="${APPIMAGETOOL:-}"
if [[ -z $tool ]]; then
  tool="$workdir/appimagetool"
  log "downloading appimagetool $APPIMAGETOOL_VERSION"
  curl -fsSL --retry 3 --retry-all-errors -o "$tool" \
    "https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_VERSION}/appimagetool-x86_64.AppImage"
  echo "${APPIMAGETOOL_SHA256_X86_64}  $tool" | sha256sum --check --status \
    || die "appimagetool checksum mismatch"
  chmod +x "$tool"
fi

repacked="$workdir/repacked.AppImage"
ARCH=x86_64 "$tool" --no-appstream --runtime-file runtime squashfs-root "$repacked" >&2

chmod +x "$repacked"
mv -f -- "$repacked" "$appimage"
log "repacked $appimage"
