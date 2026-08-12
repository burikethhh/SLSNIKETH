// Solo Leveling Gym POS â€” Electron shell
//
// Spawns the PyInstaller-built GymPOS.exe as a child process, waits until
// the FastAPI server is listening on 127.0.0.1:<port>, then loads it in a
// BrowserWindow. On quit, the backend is killed so we never orphan a
// uvicorn process holding the port.

const { app, BrowserWindow, Menu, dialog, shell, session, screen } = require("electron");
const { spawn, execSync } = require("child_process");
const net = require("net");
const path = require("path");
const fs = require("fs");

// â”€â”€ File logger â€” writes to %APPDATA%/gympos-shell/gympos-shell.log â”€â”€
const LOG_DIR = path.join(app.getPath("userData"));
if (!fs.existsSync(LOG_DIR)) fs.mkdirSync(LOG_DIR, { recursive: true });
const LOG_PATH = path.join(LOG_DIR, "gympos-shell.log");
// Truncate on each launch so the file stays manageable
const logStream = fs.createWriteStream(LOG_PATH, { flags: "w" });

function log(tag, msg) {
  const ts = new Date().toISOString();
  const line = `${ts} [${tag}] ${msg}`;
  logStream.write(line + "\n");
  process.stdout.write(line + "\n");
}
log("shell", `Log file: ${LOG_PATH}`);

// â”€â”€ Portable data directory â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// All user data (DB, photos, snapshots) lives in a "GymPOS_Data" folder
// next to the Electron exe. This makes the app fully portable â€” just copy
// the exe + GymPOS_Data folder to any device and it works.
function resolveDataDir() {
  // In a packaged build, app.getPath("exe") gives the actual .exe location.
  // For portable builds, the exe extracts to a temp dir, but
  // process.env.PORTABLE_EXECUTABLE_DIR points to the REAL directory
  // where the user placed the .exe (set by electron-builder portable).
  const exeDir =
    process.env.PORTABLE_EXECUTABLE_DIR ||  // portable build
    path.dirname(app.getPath("exe"));       // installed build / dev
  const dataDir = path.join(exeDir, "GymPOS_Data");
  if (!fs.existsSync(dataDir)) fs.mkdirSync(dataDir, { recursive: true });
  return dataDir;
}


// â”€â”€ Injected error logger (runs inside every renderer page) â”€â”€â”€â”€â”€
// Captures JS errors, fetch failures, img load failures.
// Shows a small floating error panel AND logs to console (â†’ log file).
const ERROR_LOGGER_JS = `
(function() {
  if (window.__gymposErrorLogger) return;
  window.__gymposErrorLogger = true;

  var errors = [];
  var MAX = 50;

  function addError(obj) {
    if (errors.length >= MAX) errors.shift();
    obj.time = new Date().toLocaleTimeString();
    errors.push(obj);
    console.error('[GYMPOS-ERR] ' + obj.type + ': ' + obj.message + (obj.url ? ' | ' + obj.url : ''));
    renderPanel();
  }

  // 1) JS runtime errors
  window.addEventListener('error', function(e) {
    if (e.target && (e.target.tagName === 'IMG' || e.target.tagName === 'LINK' || e.target.tagName === 'SCRIPT')) {
      addError({ type: 'RESOURCE', message: e.target.tagName + ' failed to load', url: e.target.src || e.target.href });
    } else {
      addError({ type: 'JS', message: (e.message || 'Unknown error'), url: e.filename, line: e.lineno });
    }
  }, true);

  // 2) Unhandled promise rejections
  window.addEventListener('unhandledrejection', function(e) {
    var msg = e.reason ? (e.reason.message || String(e.reason)) : 'Promise rejected';
    addError({ type: 'PROMISE', message: msg });
  });

  // 3) Intercept fetch to log failures
  var origFetch = window.fetch;
  window.fetch = function() {
    var url = arguments[0];
    if (typeof url === 'object' && url.url) url = url.url;
    return origFetch.apply(this, arguments).then(function(resp) {
      if (!resp.ok) {
        addError({ type: 'FETCH', message: resp.status + ' ' + resp.statusText, url: String(url) });
      }
      return resp;
    }).catch(function(err) {
      addError({ type: 'FETCH', message: err.message, url: String(url) });
      throw err;
    });
  };

  // 4) Floating error panel
  var panel = null;
  function renderPanel() {
    if (!errors.length) return;
    if (!panel) {
      panel = document.createElement('div');
      panel.id = 'gympos-error-panel';
      panel.style.cssText = 'position:fixed;bottom:8px;right:8px;width:420px;max-height:220px;overflow-y:auto;' +
        'background:rgba(30,0,0,.92);border:1px solid #dc2626;border-radius:8px;padding:8px 10px;z-index:999999;' +
        'font-family:monospace;font-size:11px;color:#fca5a5;backdrop-filter:blur(4px);';
      var hdr = document.createElement('div');
      hdr.style.cssText = 'display:flex;justify-content:space-between;align-items:center;margin-bottom:6px;';
      hdr.innerHTML = '<span style=\"color:#ef4444;font-weight:bold;\">Error Log</span>' +
        '<button onclick=\"document.getElementById(\\'gympos-error-panel\\').style.display=\\'none\\'\" ' +
        'style=\"background:none;border:none;color:#999;cursor:pointer;font-size:14px;\">âœ•</button>';
      panel.appendChild(hdr);
      var list = document.createElement('div');
      list.id = 'gympos-error-list';
      panel.appendChild(list);
      document.body.appendChild(panel);
    }
    panel.style.display = '';
    var list = document.getElementById('gympos-error-list');
    var html = '';
    for (var i = errors.length - 1; i >= Math.max(0, errors.length - 20); i--) {
      var e = errors[i];
      html += '<div style=\"border-bottom:1px solid #450a0a;padding:3px 0;\">' +
        '<span style=\"color:#f87171;\">[' + e.type + ']</span> ' +
        '<span style=\"color:#fde68a;\">' + e.time + '</span> ' +
        e.message + (e.url ? ' <span style=\"color:#6b7280;\">' + e.url + '</span>' : '') +
        '</div>';
    }
    list.innerHTML = html;
  }
})();
`;

const PREFERRED_PORT = parseInt(process.env.SOLO_PORT || "8000", 10);
const BACKEND_HOST = "127.0.0.1";

let backendPort = PREFERRED_PORT;        // resolved at runtime by pickPort()
let backendUrl = `http://${BACKEND_HOST}:${backendPort}/`;
let mainWindow = null;
let backendProc = null;

// Probe for a free port. Prefer the configured port (8000 by default) so the
// URL stays stable across launches; if something else is already on it, fall
// back to an OS-assigned ephemeral port so the app never fails to start just
// because another process is squatting on 8000.
//
// We test-bind on 0.0.0.0 (INADDR_ANY) because uvicorn binds there as well.
// On Windows, binding 127.0.0.1:N does NOT conflict with an existing
// 0.0.0.0:N bind, so probing on 127.0.0.1 would give false positives.
const PROBE_HOST = "0.0.0.0";

function pickPort(preferred) {
  return new Promise((resolve) => {
    const tryBind = (port, onFail) => {
      const srv = net.createServer();
      srv.unref();
      srv.once("error", onFail);
      srv.listen(port, PROBE_HOST, () => {
        const assigned = srv.address().port;
        srv.close(() => resolve(assigned));
      });
    };
    tryBind(preferred, () => {
      // Preferred port is taken â€” ask the OS for any free port.
      tryBind(0, () => resolve(preferred));
    });
  });
}

// â”€â”€ Backend discovery â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// Priority:
//   1. PyInstaller .exe (packaged or dev dist/)
//   2. Source mode: .venv/Scripts/python.exe + main.py (no build needed)
function resolveBackendExe() {
  const candidates = [
    // Packaged build: afterPack copies into resources/backend/
    path.join(process.resourcesPath || "", "backend", "GymPOS.exe"),
    // Exe placed next to electron main.js
    path.join(__dirname, "GymPOS.exe"),
    // Dev mode: onefile build
    path.join(__dirname, "..", "dist", "GymPOS.exe"),
    // Dev mode: onedir build (legacy)
    path.join(__dirname, "..", "dist", "GymPOS", "GymPOS.exe"),
  ];
  for (const c of candidates) {
    log("shell", `Checking backend candidate: ${c} â†’ ${fs.existsSync(c) ? "FOUND" : "missing"}`);
    if (c && fs.existsSync(c)) return { exe: c, args: [], cwd: null, mode: "pyinstaller" };
  }
  return null;
}

function resolveSourceBackend() {
  // Look for .venv/Scripts/python.exe and main.py in the project root
  const projectRoot = path.resolve(__dirname, "..");
  const pythonExe = path.join(projectRoot, ".venv", "Scripts", "python.exe");
  const mainPy = path.join(projectRoot, "main.py");
  log("shell", `Source mode check: python=${fs.existsSync(pythonExe)}, main.py=${fs.existsSync(mainPy)}`);
  if (fs.existsSync(pythonExe) && fs.existsSync(mainPy)) {
    return { exe: pythonExe, args: [mainPy], cwd: projectRoot, mode: "source" };
  }
  return null;
}

function probePort(host, port, timeoutMs = 500) {
  return new Promise((resolve) => {
    const sock = new net.Socket();
    let done = false;
    const finish = (ok) => {
      if (done) return;
      done = true;
      sock.destroy();
      resolve(ok);
    };
    sock.setTimeout(timeoutMs);
    sock.once("connect", () => finish(true));
    sock.once("error", () => finish(false));
    sock.once("timeout", () => finish(false));
    sock.connect(port, host);
  });
}

async function waitForBackend(host, port, deadlineMs = 120000) {
  const start = Date.now();
  while (Date.now() - start < deadlineMs) {
    if (await probePort(host, port, 500)) return true;
    await new Promise((r) => setTimeout(r, 300));
  }
  return false;
}

async function startBackend() {
  let backend = resolveBackendExe() || resolveSourceBackend();
  if (!backend) {
    dialog.showErrorBox(
      "GymPOS â€” Backend missing",
      "Could not locate GymPOS.exe or Python source.\n\n" +
        "Expected one of:\n" +
        "  â€¢ resources/backend/GymPOS.exe (packaged)\n" +
        "  â€¢ ../dist/GymPOS.exe (built)\n" +
        "  â€¢ ../.venv/Scripts/python.exe + ../main.py (source)\n\n" +
        "Install the Python backend first."
    );
    app.quit();
    return null;
  }

  backendPort = await pickPort(PREFERRED_PORT);
  backendUrl = `http://${BACKEND_HOST}:${backendPort}/`;
  if (backendPort !== PREFERRED_PORT) {
    console.log(`[shell] Port ${PREFERRED_PORT} busy â€” using ${backendPort} instead`);
  }

  // In source mode, use the project root as data dir (keeps existing DB/photos).
  // In exe mode, use the portable GymPOS_Data folder next to the Electron exe.
  const dataDir = backend.mode === "source" ? backend.cwd : resolveDataDir();
  log("shell", `Data directory: ${dataDir}`);
  log("shell", `Spawning backend [${backend.mode}] on port ${backendPort}: ${backend.exe} ${backend.args.join(" ")}`);
  const proc = spawn(backend.exe, backend.args, {
    cwd: backend.cwd || dataDir,       // source mode uses project root; exe mode uses data dir
    env: {
      ...process.env,
      SOLO_HEADLESS: "1",              // don't auto-open a browser tab
      SOLO_PORT: String(backendPort),
      SOLO_DATA_DIR: dataDir,          // all user data (DB, photos, snapshots)
      PYTHONUNBUFFERED: "1",           // flush logs immediately
      CAM1_INDEX: process.env.CAM1_INDEX || "1",
      CAM2_INDEX: process.env.CAM2_INDEX || "0",
    },
    windowsHide: true,                 // no console window for the child
    detached: false,
    stdio: ["ignore", "pipe", "pipe"],
  });

  proc.stdout.on("data", (d) => {
    const s = d.toString().trimEnd();
    log("backend", s);
  });
  proc.stderr.on("data", (d) => {
    const s = d.toString().trimEnd();
    log("backend:err", s);
  });
  proc.on("exit", (code, signal) => {
    console.log(`[shell] Backend exited (code=${code}, signal=${signal})`);
    backendProc = null;
    if (mainWindow && !app.isQuitting) {
      dialog.showErrorBox(
        "GymPOS â€” Backend stopped",
        `The GymPOS backend process exited unexpectedly (code ${code}). The app will now close.`
      );
      app.quit();
    }
  });

  return proc;
}

function killBackend() {
  if (!backendProc) return;
  const pid = backendProc.pid;
  log("shell", `Killing backend process tree (PID ${pid})â€¦`);
  try {
    // taskkill /T kills the entire process tree â€” child workers, camera
    // threads, uvicorn, everything.  /F forces termination.
    execSync(`taskkill /PID ${pid} /T /F`, { windowsHide: true, timeout: 10000 });
    log("shell", "Backend process tree killed via taskkill.");
  } catch (e) {
    // taskkill may fail if process already exited â€” that's fine.
    log("shell", `taskkill returned: ${e.message || e}`);
    try { backendProc.kill(); } catch (_) {}
  }
  backendProc = null;
}

// Nuke any leftover process still holding our backend port.
function killOrphanOnPort(port) {
  try {
    const out = execSync(
      `netstat -aon | findstr ":${port}.*LISTENING"`,
      { windowsHide: true, timeout: 5000, encoding: "utf8" }
    );
    const pids = new Set(
      out.split("\n").map(l => l.trim().split(/\s+/).pop()).filter(p => p && p !== "0")
    );
    for (const p of pids) {
      log("shell", `Killing orphan PID ${p} on port ${port}`);
      try { execSync(`taskkill /PID ${p} /T /F`, { windowsHide: true, timeout: 5000 }); } catch (_) {}
    }
  } catch (_) {
    // No listeners found â€” good.
  }
}

// â”€â”€ Camera / media permissions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
// The renderer needs getUserMedia access for the live-preview feature on the
// cameras admin page. Without explicit handlers Chromium's sandbox blocks
// all media requests from non-https origins (our backend is http://127.0.0.1).
function setupMediaPermissions() {
  const ses = session.defaultSession;

  // Grant camera & microphone permission requests from our own backend origin.
  ses.setPermissionRequestHandler((webContents, permission, callback) => {
    const url = webContents.getURL();
    const isLocal =
      url.startsWith("http://127.0.0.1") || url.startsWith("http://localhost");
    if (isLocal && (permission === "media" || permission === "camera" || permission === "microphone")) {
      callback(true);
      return;
    }
    // Default: deny anything unexpected.
    callback(false);
  });

  // Synchronous permission check â€” used by enumerateDevices / getUserMedia
  // before the async request handler fires.
  ses.setPermissionCheckHandler((webContents, permission) => {
    if (permission === "media" || permission === "camera" || permission === "microphone") {
      return true;
    }
    return false;
  });
}

// â”€â”€ Main window â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
function createWindow() {
  // Auto-adapt to the device's screen size
  const { width: sw, height: sh } = screen.getPrimaryDisplay().workAreaSize;
  mainWindow = new BrowserWindow({
    width: Math.min(sw, 1600),
    height: Math.min(sh, 1000),
    minWidth: 900,
    minHeight: 600,
    backgroundColor: "#0b0f1a",
    show: false,
    autoHideMenuBar: true,
    icon: path.join(__dirname, "build", "icon.ico"),
    webPreferences: {
      preload: path.join(__dirname, "preload.js"),
      contextIsolation: true,
      nodeIntegration: false,
      zoomFactor: sw <= 1366 ? 0.9 : 1.0,   // slightly smaller on low-res screens
    },
  });
  // Maximize on launch for POS â€” full screen real estate
  mainWindow.maximize();

  // External links (target=_blank) open in the default browser, not in-app.
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url);
    return { action: "deny" };
  });

  // Show a friendly loading page while we wait for the backend.
  mainWindow.loadURL(
    "data:text/html;charset=utf-8," +
      encodeURIComponent(`
        <html><head><title>GymPOS â€” Startingâ€¦</title><style>
          html,body{margin:0;height:100%;background:#0b0f1a;color:#e5e7eb;
            font-family:system-ui,-apple-system,Segoe UI,sans-serif;
            display:flex;align-items:center;justify-content:center;}
          .box{text-align:center}
          .ring{width:52px;height:52px;margin:0 auto 18px;border:4px solid #1f2937;
            border-top-color:#8b5cf6;border-radius:50%;animation:spin 1s linear infinite;}
          @keyframes spin{to{transform:rotate(360deg)}}
          h1{font-size:18px;font-weight:600;margin:0 0 6px;letter-spacing:.5px}
          p{font-size:13px;color:#9ca3af;margin:0}
        </style></head><body>
        <div class="box"><div class="ring"></div>
          <h1>GYMPOS</h1><p>Starting servicesâ€¦</p></div>
        </body></html>`)
  );

  // Forward renderer console messages to our log file
  mainWindow.webContents.on("console-message", (ev, level, message, line, sourceId) => {
    const lvl = ["DEBUG","INFO","WARN","ERROR"][level] || "LOG";
    log("renderer", `[${lvl}] ${message}`);
  });

  // F12 = DevTools, F5 = Reload, Ctrl+Shift+I = DevTools (works inside sandbox)
  mainWindow.webContents.on("before-input-event", (event, input) => {
    if (input.key === "F12" && input.type === "keyDown") {
      mainWindow.webContents.toggleDevTools();
      event.preventDefault();
    }
    if (input.key === "F5" && input.type === "keyDown") {
      mainWindow.webContents.reload();
      event.preventDefault();
    }
    if (input.key === "I" && input.control && input.shift && input.type === "keyDown") {
      mainWindow.webContents.toggleDevTools();
      event.preventDefault();
    }
  });

  // Inject automatic error logger into every page load
  mainWindow.webContents.on("did-finish-load", () => {
    mainWindow.webContents.executeJavaScript(ERROR_LOGGER_JS).catch(() => {});
  });

  // Capture renderer crashes
  mainWindow.webContents.on("render-process-gone", (ev, details) => {
    log("renderer", `CRASH: ${details.reason} exitCode=${details.exitCode}`);
  });

  // Capture failed network requests (404, 500, connection refused, etc.)
  mainWindow.webContents.session.webRequest.onCompleted(
    { urls: ["http://127.0.0.1:*/*", "http://localhost:*/*"] },
    (details) => {
      if (details.statusCode >= 400 || details.error) {
        log("network", `${details.method} ${details.url} â†’ ${details.statusCode} ${details.error || ""}`);
      }
    }
  );
  mainWindow.webContents.session.webRequest.onErrorOccurred(
    { urls: ["http://127.0.0.1:*/*", "http://localhost:*/*"] },
    (details) => {
      log("network:err", `${details.method} ${details.url} â†’ ${details.error}`);
    }
  );

  mainWindow.once("ready-to-show", () => {
    mainWindow.show();
    mainWindow.focus();
  });
  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

// â”€â”€ App lifecycle â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
app.on("window-all-closed", () => {
  app.isQuitting = true;
  killBackend();
  killOrphanOnPort(backendPort);
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => {
  app.isQuitting = true;
  killBackend();
  killOrphanOnPort(backendPort);
});

app.on("will-quit", () => {
  // Final safety net â€” if anything is still alive, nuke it.
  killBackend();
  killOrphanOnPort(backendPort);
});

// Absolute last resort â€” runs even if Electron crashes.
process.on("exit", () => {
  try { killBackend(); } catch (_) {}
  try { killOrphanOnPort(backendPort); } catch (_) {}
});

app.whenReady().then(async () => {
  // Strip the default menu â€” this is a POS app, not a browser.
  Menu.setApplicationMenu(null);

  // Allow the renderer to access cameras for live preview.
  setupMediaPermissions();

  createWindow();

  // DEV MODE: If SOLO_DEV=1, connect to an already-running Python dev server
  // instead of spawning GymPOS.exe. This avoids two processes fighting for
  // the camera and is the normal workflow during development.
  const devMode = process.env.SOLO_DEV === "1";

  if (devMode) {
    log("shell", "DEV MODE â€” connecting to existing backend on port " + backendPort);
  } else {
    const started = await startBackend();
    if (started !== null) backendProc = started;
  }

  const alive = await waitForBackend(BACKEND_HOST, backendPort, devMode ? 5000 : 120000);
  if (!alive) {
    dialog.showErrorBox(
      "GymPOS â€” Backend did not start",
      devMode
        ? `No backend found on ${backendUrl}.\n\nStart the dev server first:\n  $env:SOLO_HEADLESS="1"; .venv\\Scripts\\python.exe main.py`
        : `Timed out waiting for the backend on ${backendUrl}.\n\nTry running GymPOS.exe manually from the install directory to see the error.`
    );
    app.quit();
    return;
  }

  log("shell", `Backend is alive â€” loading ${backendUrl}`);
  if (mainWindow) mainWindow.loadURL(backendUrl);
});


