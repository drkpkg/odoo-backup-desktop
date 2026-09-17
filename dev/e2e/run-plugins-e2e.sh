#!/usr/bin/env bash
# End-to-end check of the plugin system in the real app (WebKitGTK) through WebDriver.
#
# Requirements (Linux): tauri-driver (`cargo install tauri-driver --locked`), WebKitWebDriver
# (Arch: webkitgtk-6.0, Debian/Ubuntu: webkit2gtk-driver) and either `kwin_wayland` (virtual
# display, used when available) or `xvfb-run`.
#
#   dev/e2e/run-plugins-e2e.sh            # builds the debug app (target/e2e), prints PASS/FAIL
#   OUT=/tmp/obd-e2e dev/e2e/run-plugins-e2e.sh
#
# Isolation: private D-Bus session, temporary XDG dirs; nothing touches your real app data.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
out="${OUT:-$(mktemp -d -t obd-plugins-e2e-XXXXXX)}"
# Separate target dir: `cargo test`/`cargo build` produce a dev binary (devUrl) at target/debug.
e2e_target="$repo/target/e2e"
app_bin="$e2e_target/debug/odoo-backup-desktop"
driver="${TAURI_DRIVER:-$(command -v tauri-driver || true)}"
native="${WEBKIT_WEBDRIVER:-$(command -v WebKitWebDriver || true)}"

[[ -n $driver ]] || { echo "tauri-driver not found (cargo install tauri-driver --locked)" >&2; exit 2; }
[[ -n $native ]] || { echo "WebKitWebDriver not found" >&2; exit 2; }
# Always build (incremental): the binary must embed the current frontend.
(cd "$repo" && CARGO_TARGET_DIR="$e2e_target" pnpm tauri build --debug --no-bundle)
mkdir -p "$out"

inner="$out/session.sh"
cat > "$inner" <<INNER
#!/usr/bin/env bash
set -u
export WAYLAND_DISPLAY=obd-e2e
export XDG_DATA_HOME="$out/xdg-data" XDG_CONFIG_HOME="$out/xdg-config" XDG_CACHE_HOME="$out/xdg-cache"
plugins="\$XDG_DATA_HOME/io.github.drkpkg.odoo-backup-desktop/plugins"
mkdir -p "\$plugins" "\$XDG_CONFIG_HOME" "\$XDG_CACHE_HOME"
cp -r "$repo/examples/plugins/hello-obd" "\$plugins/"
"$driver" --native-driver "$native" > "$out/tauri-driver.log" 2>&1 &
driver_pid=\$!
E2E_OUT="$out" APP_BIN="$app_bin" timeout 180 python3 "$here/plugins_webdriver.py" > "$out/result.txt" 2>&1
echo \$? > "$out/exit-code"
kill "\$driver_pid" 2>/dev/null || true
INNER
chmod +x "$inner"

if command -v kwin_wayland >/dev/null; then
  env -u DISPLAY -u WAYLAND_DISPLAY dbus-run-session -- \
    kwin_wayland --virtual --width 1280 --height 860 --socket obd-e2e --exit-with-session "$inner" \
    > "$out/display.log" 2>&1 || true
elif command -v xvfb-run >/dev/null; then
  sed -i '/^export WAYLAND_DISPLAY=/d' "$inner"
  env -u WAYLAND_DISPLAY GDK_BACKEND=x11 dbus-run-session -- xvfb-run -a -s "-screen 0 1280x860x24" "$inner" > "$out/display.log" 2>&1 || true
else
  echo "neither kwin_wayland nor xvfb-run is available" >&2
  exit 2
fi

cat "$out/result.txt"
echo "artifacts: $out"
exit "$(cat "$out/exit-code" 2>/dev/null || echo 1)"
