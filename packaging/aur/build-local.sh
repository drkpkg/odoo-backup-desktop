#!/usr/bin/env bash
# Builds the Arch package (appex-backup-bin) from a locally built .deb, for testing.
#
# Usage: packaging/aur/build-local.sh [path/to/appex-backup.deb]
# Default .deb: newest file in target/release/bundle/deb/ (run `pnpm tauri build` first).
# Output: target/release/bundle/arch/appex-backup-bin-<version>-<rel>-x86_64.pkg.tar.zst
#         (override the directory with OUT_DIR=...)
set -euo pipefail

log() { printf '[aur-build-local] %s\n' "$*" >&2; }
die() { log "error: $*"; exit 1; }

command -v makepkg >/dev/null || die "makepkg not found (run this on Arch/Manjaro)"
command -v bsdtar >/dev/null || die "bsdtar not found (install libarchive)"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
pkgbuild_dir="${script_dir}/appex-backup-bin"

deb="${1:-}"
if [[ -z ${deb} ]]; then
  deb_dir="${repo_root}/target/release/bundle/deb"
  if [[ -d ${deb_dir} ]]; then
    # Newest .deb by modification time.
    deb="$(find "${deb_dir}" -maxdepth 1 -type f -name '*.deb' -printf '%T@ %p\n' | sort -nr | head -n 1 | cut -d' ' -f2-)"
  fi
  [[ -n ${deb} ]] || die "no .deb in target/release/bundle/deb/ (run: pnpm tauri build --bundles deb)"
fi
[[ -f ${deb} ]] || die "file not found: ${deb}"
deb="$(realpath "${deb}")"

# Version from the .deb control file; pacman versions cannot contain '-'.
control_tar="$(bsdtar -tf "${deb}" | grep -E '^control\.tar' | head -n 1)"
[[ -n ${control_tar} ]] || die "control.tar.* not found in ${deb}"
deb_version="$(bsdtar -xOf "${deb}" "${control_tar}" | bsdtar -xOf - | awk -F': ' '/^Version:/ {print $2; exit}')"
[[ -n ${deb_version} ]] || die "could not read Version from ${deb}"
pkgver="${deb_version//-/_}"

workdir="$(mktemp -d)"
trap 'rm -rf "${workdir}"' EXIT

cp "${pkgbuild_dir}/PKGBUILD" "${workdir}/PKGBUILD"
sed -i "s/^pkgver=.*/pkgver=${pkgver}/" "${workdir}/PKGBUILD"
# makepkg uses a local file whose name matches the source entry instead of downloading.
cp "${deb}" "${workdir}/appex-backup-${pkgver}-x86_64.deb"

log "building appex-backup-bin ${pkgver} from $(basename "${deb}")"
(
  cd "${workdir}"
  # --nodeps: runtime deps are not needed to repackage; --skipchecksums: local .deb.
  makepkg --force --nodeps --skipchecksums --noconfirm
)

out_dir="${OUT_DIR:-${repo_root}/target/release/bundle/arch}"
mkdir -p "${out_dir}"
shopt -s nullglob
pkgs=("${workdir}"/*.pkg.tar.*)
shopt -u nullglob
[[ ${#pkgs[@]} -gt 0 ]] || die "makepkg produced no package"
mv -f "${pkgs[@]}" "${out_dir}/"
log "package(s) written to ${out_dir}"
ls -1 "${out_dir}"
