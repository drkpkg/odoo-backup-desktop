---
name: desktop-release
description: Use when building installers or cutting a release of Appex Backup — Tauri bundle targets (.deb, .rpm, AppImage, Windows NSIS .exe), the Arch/AUR package, the AppImage Wayland library fix, GitHub Actions workflows (.github/workflows), Windows code signing, or enabling the updater.
---

# Packaging and releases

Deliverables: Linux binary + AppImage, `.deb` (Ubuntu 22.04/24.04), `.rpm`, Arch package
(`appex-backup-bin`, AUR + `.pkg.tar.zst` asset), Windows installer (NSIS `-setup.exe`).

## Where things are

| Path | Purpose |
|---|---|
| `src-tauri/tauri.conf.json` → `bundle` | targets `deb`, `rpm`, `appimage`, `nsis`; NSIS `installMode: currentUser`; WebView2 `embedBootstrapper` (~1.8 MB) |
| `.github/workflows/ci.yml` | fmt, clippy `-D warnings`, cargo tests, frontend typecheck/test/build |
| `.github/workflows/release.yml` | tag `v*` → draft GitHub Release with all bundles |
| `packaging/appimage/strip-wayland-libs.sh` | removes bundled libwayland/libxkbcommon/libxcb and repacks |
| `packaging/aur/appex-backup-bin/PKGBUILD` | Arch package from the release `.deb` |
| `packaging/aur/build-local.sh` | builds the Arch package from a local `target/release/bundle/deb/*.deb` |

Bundles land in `target/release/bundle/<type>/` at the **workspace root** (not `src-tauri/target`).

## Cutting a release

1. Bump `version` in `package.json` (Tauri reads it: `"version": "../package.json"`) and in
   `[workspace.package]` of `Cargo.toml`; update `pkgver` in the PKGBUILD.
2. Commit, tag `vX.Y.Z`, push the tag → `release.yml` builds Linux (ubuntu-22.04) + Windows and
   creates a **draft** release. Assets are renamed by `releaseAssetNamePattern`
   `[mainBinaryName]_[version]_[arch][setup][ext]` → e.g. `appex-backup_0.1.0_amd64.deb`
   (*the exact `[arch]` string per bundle type must be checked on the first release*).
3. The `arch-package` job downloads the `.deb`, builds `appex-backup-bin-*.pkg.tar.zst` in an
   `archlinux` container and attaches it plus the generated PKGBUILD/.SRCINFO with real sha256.
4. Review the draft, publish. For the AUR: copy `PKGBUILD` + `.SRCINFO` from the release to the
   `ssh://aur@aur.archlinux.org/appex-backup-bin.git` repo.

## Linux notes

- Build on **ubuntu-22.04**: oldest runner with WebKitGTK 4.1 → lowest glibc requirement; the
  result runs on 22.04, 24.04 and Arch.
- Build deps: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libdbus-1-dev patchelf build-essential file`.
- **AppImage is best-effort.** Its bundled `libwayland-*`, `libxkbcommon*`, `libxcb*` make EGL abort
  (`EGL_BAD_PARAMETER`) on Wayland + Mesa 25+ (Ubuntu 24.04+, Fedora 44, Arch). Upstream: tauri
  issues #15665, #15976 (open), PR #15662 (unmerged). The release job runs
  `strip-wayland-libs.sh` (appimagetool 1.9.1 pinned by sha256). Prefer `.deb`/Arch packages.
- Arch runtime deps: `cairo desktop-file-utils gdk-pixbuf2 glib2 gtk3 hicolor-icon-theme libsoup3 pango webkit2gtk-4.1 openssl`.
- Updating the `.deb` via the Tauri updater prompts for elevation; *unverified* UX.

## Windows notes

- NSIS per-user install (no UAC). MSI (WiX) is not built (only on Windows hosts and not needed).
- **SmartScreen:** unsigned installers show warnings. EV certificates no longer bypass SmartScreen.
  - Microsoft Artifact Signing (ex Azure Trusted Signing, US$9.99/month) plugs in via
    `bundle.windows.signCommand`, but organizations only qualify in US, CA, EU, UK, AU, NZ, JP, KR,
    SG, CH, NO, IL (individuals US/CA) → likely **not available** for a LatAm company.
  - Fallback: OV code-signing certificate on a cloud HSM + `signCommand`.
  - `release.yml` has commented placeholders (`WINDOWS_SIGN_*` secrets); signing is off by default.

## Updater (not enabled yet)

`tauri-plugin-updater` requires a signing key pair (`pnpm tauri signer generate`), the public key in
`tauri.conf.json` (`plugins.updater.pubkey` + `endpoints`), `bundle.createUpdaterArtifacts: true` and
secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in CI. Its changelog
(2.10.0) says deb/rpm/AppImage/NSIS are supported; docs page still says AppImage-only (*verify*).
tauri-action uploads `latest.json` when updater artifacts exist.

## Local checks

```bash
NO_STRIP=true pnpm tauri build                     # all configured bundles (NO_STRIP: see below)
packaging/appimage/strip-wayland-libs.sh target/release/bundle/appimage/*.AppImage
packaging/aur/build-local.sh                        # needs makepkg (Arch/Manjaro)
pnpm tauri icon src-tauri/icons/app-icon.svg        # regenerate icons (delete android/ and ios/ after)
```

- **Arch/Manjaro local AppImage builds:** linuxdeploy's bundled `strip` does not understand the
  `.relr.dyn` section of current Arch libraries (`unknown type [0x13] section .relr.dyn` →
  "failed to run linuxdeploy"). Use `NO_STRIP=true`. Not needed on the ubuntu-22.04 CI runner.
  An AppImage built on Arch bundles Arch's newer libraries and is for local testing only.
- Bundle file names use `productName` (`Appex Backup_0.1.0_amd64.deb`, with a space); the release
  workflow renames assets with `[mainBinaryName]_[version]_[arch]…`. The deb package name is `appex-backup`.
- Downloads of linuxdeploy/appimagetool from GitHub occasionally fail with TLS errors
  (`cannot decrypt peer's message`): just retry.
