"""WebDriver e2e: real Tauri app (WebKitGTK) + the hello-obd example plugin.

Run through `dev/e2e/run-plugins-e2e.sh`, which starts tauri-driver and an isolated display.
Env: E2E_OUT (output dir for logs/screenshots), APP_BIN (debug app binary).
"""
import base64, json, os, sys, time, urllib.request

BASE = "http://127.0.0.1:4444"
OUT = os.environ["E2E_OUT"]
APP = os.environ["APP_BIN"]
log = open(os.path.join(OUT, "drive.log"), "w")


def say(*args):
    print(*args, file=log, flush=True)


def req(method, path, body=None, timeout=60):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(BASE + path, data=data, method=method, headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            return json.loads(resp.read() or b"{}")
    except urllib.error.HTTPError as e:
        payload = e.read().decode()
        raise RuntimeError(f"{method} {path} -> {e.code}: {payload[:500]}")


def wait_server():
    for _ in range(60):
        try:
            req("GET", "/status", timeout=2)
            return
        except Exception:
            time.sleep(0.5)
    raise RuntimeError("tauri-driver did not start")


def screenshot(sid, name):
    png = req("GET", f"/session/{sid}/screenshot")["value"]
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(base64.b64decode(png))
    say("screenshot", name)


def execute(sid, script, args=None):
    return req("POST", f"/session/{sid}/execute/sync", {"script": script, "args": args or []})["value"]


def execute_async(sid, script, args=None):
    return req("POST", f"/session/{sid}/execute/async", {"script": script, "args": args or []})["value"]


def find(sid, xpath, timeout=20):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        try:
            value = req("POST", f"/session/{sid}/element", {"using": "xpath", "value": xpath})["value"]
            return list(value.values())[0]
        except Exception as e:
            last = e
            time.sleep(0.5)
    raise RuntimeError(f"element not found: {xpath}: {last}")


def click(sid, element):
    req("POST", f"/session/{sid}/element/{element}/click", {})


def text(sid, element):
    return req("GET", f"/session/{sid}/element/{element}/text")["value"]


results = {}
wait_server()
session = req("POST", "/session", {"capabilities": {"alwaysMatch": {"browserName": "wry", "tauri:options": {"application": APP}}}})
sid = session["value"]["sessionId"]
say("session", sid)
def wait_app_loaded(sid, timeout=60):
    """Waits until the webview shows the embedded app (tauri:// or http://tauri.localhost)."""
    deadline = time.time() + timeout
    href = ""
    while time.time() < deadline:
        try:
            href = execute(sid, "return String(window.location.href) + '|' + document.readyState;")
            if (href.startswith("tauri://") or href.startswith("http://tauri.localhost")) and href.endswith("complete"):
                return
        except Exception:
            pass
        time.sleep(0.5)
    raise RuntimeError(f"app did not load (last location: {href})")


try:
    wait_app_loaded(sid)
    time.sleep(2)
    screenshot(sid, "01-start.png")
    # Create the vault through the real IPC and reload the UI.
    created = execute_async(
        sid,
        "const done = arguments[arguments.length - 1];"
        "window.__TAURI_INTERNALS__.invoke('create_vault', { useKeychain: false, masterPassword: 'e2e-password-123' })"
        ".then(r => done(JSON.stringify(r))).catch(e => done('ERR ' + JSON.stringify(e)));",
    )
    say("create_vault", created)
    execute(sid, "window.location.reload(); return true;")
    time.sleep(1)
    wait_app_loaded(sid)
    time.sleep(2)

    plugins = execute_async(
        sid,
        "const done = arguments[arguments.length - 1];"
        "window.__TAURI_INTERNALS__.invoke('list_plugins').then(r => done(JSON.stringify(r))).catch(e => done('ERR ' + JSON.stringify(e)));",
    )
    say("list_plugins", plugins[:600])
    results["plugin_enabled"] = '"id":"hello-obd"' in plugins and '"status":"enabled"' in plugins

    nav = find(sid, "//nav//button[contains(normalize-space(.), 'Hola')]")
    click(sid, nav)
    time.sleep(3)
    screenshot(sid, "02-plugin-page.png")

    frame = find(sid, "//iframe")
    src = req("GET", f"/session/{sid}/element/{frame}/attribute/src")["value"]
    sandbox = req("GET", f"/session/{sid}/element/{frame}/attribute/sandbox")["value"]
    say("iframe", src, sandbox)
    results["iframe_src"] = src

    req("POST", f"/session/{sid}/frame", {"id": 0})
    time.sleep(2)
    body = find(sid, "//body")
    content = text(sid, body)
    say("iframe text", content[:800])
    results["bridge_context"] = "hello-obd" in content
    # IPC must not be reachable from the plugin iframe.
    has_ipc = execute(sid, "return typeof window.__TAURI_INTERNALS__ !== 'undefined';")
    results["iframe_has_tauri_ipc"] = has_ipc
    origin = execute(sid, "return String(window.origin);")
    results["iframe_origin"] = origin
    say("iframe ipc", has_ipc, "origin", origin)

    # Use the storage counter through the bridge.
    buttons = req("POST", f"/session/{sid}/elements", {"using": "xpath", "value": "//button"})["value"]
    labels = [text(sid, list(b.values())[0]) for b in buttons]
    say("iframe buttons", labels)
    for b in buttons:
        el = list(b.values())[0]
        if "+1" in text(sid, el) or "Sumar" in text(sid, el) or "Incrementar" in text(sid, el):
            click(sid, el)
            click(sid, el)
            break
    time.sleep(1)
    content_after = text(sid, find(sid, "//body"))
    say("iframe text after counter", content_after[:800])
    req("POST", f"/session/{sid}/frame/parent", {})
    stored = execute_async(
        sid,
        "const done = arguments[arguments.length - 1];"
        "window.__TAURI_INTERNALS__.invoke('plugin_storage_get', { pluginId: 'hello-obd', key: 'counter' })"
        ".then(r => done(JSON.stringify(r))).catch(e => done('ERR ' + JSON.stringify(e)));",
    )
    say("storage counter", stored)
    results["storage_after_clicks"] = stored
    screenshot(sid, "03-after-counter.png")

    # Open the plugin window from the main window IPC and inspect it.
    opened = execute_async(
        sid,
        "const done = arguments[arguments.length - 1];"
        "window.__TAURI_INTERNALS__.invoke('open_plugin_window', { pluginId: 'hello-obd', windowId: 'detail', params: { instanceId: null } })"
        ".then(r => done(JSON.stringify(r))).catch(e => done('ERR ' + JSON.stringify(e)));",
    )
    say("open_plugin_window", opened)
    time.sleep(4)
    handles = req("GET", f"/session/{sid}/window/handles")["value"]
    say("handles", handles)
    results["window_handles"] = len(handles)
    current = req("GET", f"/session/{sid}/window")["value"]
    other = [h for h in handles if h != current][0]
    req("POST", f"/session/{sid}/window", {"handle": other})
    time.sleep(3)
    results["plugin_window_title"] = req("GET", f"/session/{sid}/title")["value"]
    screenshot(sid, "04-plugin-window.png")
    ctx = execute_async(
        sid,
        "const done = arguments[arguments.length - 1];"
        "window.__TAURI_INTERNALS__.invoke('get_plugin_window_context').then(r => done(JSON.stringify(r))).catch(e => done('ERR ' + JSON.stringify(e)));",
    )
    results["plugin_window_context"] = ctx
    denied = execute_async(
        sid,
        "const done = arguments[arguments.length - 1];"
        "window.__TAURI_INTERNALS__.invoke('get_settings').then(r => done('ALLOWED')).catch(e => done('DENIED ' + String(e)));",
    )
    results["plugin_window_get_settings"] = denied[:160]
    req("POST", f"/session/{sid}/frame", {"id": 0})
    time.sleep(1)
    results["plugin_window_text"] = text(sid, find(sid, "//body"))[:200]
finally:
    with open(os.path.join(OUT, "results.json"), "w") as f:
        json.dump(results, f, indent=2)
    try:
        req("DELETE", f"/session/{sid}")
    except Exception as e:
        say("delete session", e)
    say("done", results)

CHECKS = {
    "plugin is enabled": results.get("plugin_enabled") is True,
    "iframe serves the plugin page": str(results.get("iframe_src", "")).endswith("/hello-obd/ui/index.html?rev=1"),
    "bridge answers context.get": results.get("bridge_context") is True,
    "iframe has no Tauri IPC": results.get("iframe_has_tauri_ipc") is False,
    "iframe origin is opaque": results.get("iframe_origin") == "null",
    "storage.set persisted through the bridge": results.get("storage_after_clicks") == "2",
    "plugin window opened": results.get("window_handles") == 2,
    "plugin window context": '"pluginId":"hello-obd"' in str(results.get("plugin_window_context")),
    "plugin window capability denies get_settings": str(results.get("plugin_window_get_settings", "")).startswith("DENIED"),
    "plugin window renders the plugin": "Detalle" in str(results.get("plugin_window_text", "")),
}
failed = [name for name, ok in CHECKS.items() if not ok]
for name, ok in CHECKS.items():
    print(("PASS " if ok else "FAIL ") + name)
sys.exit(1 if failed else 0)
