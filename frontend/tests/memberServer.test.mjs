import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

const require = createRequire(import.meta.url);
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const servers = JSON.parse(readFileSync(resolve(root, "..", "crates", "train-api", "tests", "fixtures", "member-servers.json"), "utf8"));

// Execute the actual TypeScript pages, stubbing only desktop I/O and page state.
function loadSource(relativePath, overrides = {}, warnings = []) {
  const path = resolve(root, "src", relativePath);
  const exports = {};
  const code = ts.transpileModule(readFileSync(path, "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX },
  }).outputText;
  runInNewContext(code, {
    exports,
    console: { ...console, warn: (...args) => warnings.push(args) },
    window: { setInterval: () => 0, clearInterval: () => {} },
    require: (name) => {
      if (name in overrides) return overrides[name];
      if (name.startsWith(".")) {
        return loadSource(resolve(dirname(path), name) + ".ts", overrides, warnings);
      }
      return require(name);
    },
  }, { filename: path });
  return exports;
}

const { workspaceSurfaceStyle, workspaceBadge } = loadSource("memberServer.ts");

test("all API palettes and verification badges reach the presentation", () => {
  assert.equal(workspaceSurfaceStyle(servers[0]), undefined);
  assert.equal(workspaceBadge(servers[0]), undefined);
  for (const server of servers.slice(1)) {
    assert.equal(JSON.stringify(workspaceSurfaceStyle(server)), JSON.stringify({
      backgroundColor: server.style.palette.background,
      color: server.style.palette.foreground,
      borderColor: server.style.palette.border,
      "--train-workspace-border": server.style.palette.border,
    }));
    assert.equal(workspaceBadge(server), "検証");
  }
});

test("legacy, null and unknown environments stay unstyled", () => {
  for (const server of [
    servers[0], { ...servers[0], environment: null, style: null },
    { ...servers[1], environment: "production" }, { ...servers[1], environment: "future" },
  ]) {
    assert.equal(workspaceSurfaceStyle(server), undefined);
    assert.equal(workspaceBadge(server), undefined);
  }
  for (const style of [undefined, null, {}, { palette: null }]) {
    const server = { ...servers[1], style };
    assert.equal(workspaceSurfaceStyle(server), undefined);
    assert.equal(workspaceBadge(server), "検証");
  }
});

test("partial palettes and CSS payloads are rejected atomically with a warning", () => {
  const warnings = [];
  const { workspaceSurfaceStyle } = loadSource("memberServer.ts", {}, warnings);
  const invalid = ["red", "#fff", "#12345678", "#12345g", "url(https://example.com)", "var(--color)", "#352613; color:red", "#352613\n", null, 42, {}, undefined];
  for (const key of ["background", "foreground", "border"]) {
    for (const value of invalid) {
      const server = { ...servers[1], style: { palette: { ...servers[1].style.palette, [key]: value } } };
      assert.equal(workspaceSurfaceStyle(server), undefined);
    }
  }
  assert.equal(warnings.length, invalid.length * 3);
});

test("badges use safe React text and reject malformed or oversized labels", () => {
  const warnings = [];
  const { workspaceBadge } = loadSource("memberServer.ts", {}, warnings);
  for (const badge of ["", " ", "x".repeat(33), "test\nlabel", 42, {}]) {
    assert.equal(workspaceBadge({ ...servers[1], style: { badge } }), "検証");
  }
  assert.equal(warnings.length, 6);
  assert.equal(workspaceBadge({ ...servers[1], style: { badge: "検証 blue" } }), "検証 blue");
});

function renderPage(page, serverList = servers, stateChanges = [], callbacks = [], effects = [], invocations = []) {
  let stateIndex = 0;
  const overrides = {
    react: {
      ...React,
      useEffect: (fn) => { effects.push(fn); },
      useCallback: (fn) => { callbacks.push(fn); return fn; },
      useState: (initial) => {
        const index = stateIndex++;
        const listState = { 0: false, 1: true, 2: serverList };
        return [
          page === "Servers" && index in listState ? listState[index] : initial,
          (value) => stateChanges.push({ index, value }),
        ];
      },
    },
    "@tauri-apps/api/core": { invoke: (command, args) => {
      invocations.push({ command, args });
      return Promise.reject("not_configured");
    } },
    "@tauri-apps/api/event": { listen: () => { throw new Error("Unexpected event subscription"); } },
    "../LaunchStatus": { useLaunchStatus: () => ({ beginLaunch: () => {}, endLaunch: () => {} }) },
    "./ServerDetail": { ServerDetailPage: () => null },
    "./AccountLinkNoticeModal": { AccountLinkNoticeModal: () => null },
    "./TestModeNoticeModal": { TestModeNoticeModal: () => null },
  };
  overrides["./AuthFallbackPanel"] = loadSource("pages/AuthFallbackPanel.tsx", overrides);
  const module = loadSource(`pages/${page}.tsx`, overrides);
  const props = page === "Servers" ? {} : {
    server: serverList[0], joining: false, progress: null,
    onBack: () => {}, onJoin: () => {}, onFallbackJoin: () => {},
  };
  return renderToStaticMarkup(React.createElement(module[`${page}Page`], props));
}

test("actual list and detail render each palette and the badge without styling production", () => {
  const list = renderPage("Servers");
  assert.equal((list.match(/検証/g) ?? []).length, 3, list);
  for (const server of servers.slice(1)) {
    const style = `background-color:${server.style.palette.background};color:${server.style.palette.foreground};border-color:${server.style.palette.border}`;
    assert.ok(list.includes(style));
    const detail = renderPage("ServerDetail", [server]);
    assert.ok(detail.includes(style));
    assert.ok(detail.includes("検証"));
    assert.ok(detail.includes("起動"));
  }
  const production = renderPage("ServerDetail", [servers[0]]);
  assert.ok(production.includes("<h2"));
  assert.ok(!production.includes("検証"));
  assert.ok(!production.includes("background-color:"));
});

test("actual pages do not emit invalid CSS and escape badge markup", () => {
  const server = {
    ...servers[1],
    style: { badge: "<img src=x onerror=alert(1)>", palette: { ...servers[1].style.palette, border: "url(evil)" } },
  };
  for (const page of ["Servers", "ServerDetail"]) {
    const html = renderPage(page, [server]);
    assert.ok(!html.includes("url(evil)"));
    assert.ok(!html.includes("<img src=x"));
    assert.ok(html.includes("&lt;img"));
  }
});

test("detail refresh clears previously loaded config before an unconfigured response", async () => {
  const changes = [];
  const callbacks = [];
  renderPage("ServerDetail", [servers[1]], changes, callbacks);
  callbacks[0]();
  assert.equal(changes[0].index, 0);
  assert.equal(changes[0].value, null);
  await new Promise(resolve => setImmediate(resolve));
  assert.ok(changes.some(change => change.value === "not_configured"));
});

test("Workspace detail omits fallback settings and never requests fallback status", async () => {
  for (const server of servers.slice(1)) {
    const effects = [];
    const invocations = [];
    const html = renderPage("ServerDetail", [server], [], [], effects, invocations);
    assert.ok(!html.includes("Mojang障害時"));
    for (const label of ["事前登録・再登録", "障害時接続で起動", "状態を更新"]) {
      assert.ok(!html.includes(label));
    }
    effects.forEach(effect => effect());
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(invocations.map(call => call.command), [
      "get_server_config", "get_server_announcements",
    ]);
    assert.ok(invocations.every(call => call.args.serverId === server.id));
    assert.ok(html.includes("起動"));
  }
});

test("production and legacy detail retain fallback settings and status requests", async () => {
  for (const environment of [undefined, null, "production"]) {
    const server = { ...servers[0], environment };
    const effects = [];
    const invocations = [];
    const html = renderPage("ServerDetail", [server], [], [], effects, invocations);
    for (const label of ["Mojang障害時", "事前登録・再登録", "障害時接続で起動", "状態を更新"]) {
      assert.ok(html.includes(label));
    }
    const cleanups = effects.map(effect => effect());
    await new Promise(resolve => setImmediate(resolve));
    cleanups.forEach(cleanup => cleanup?.());
    assert.ok(invocations.some(call =>
      call.command === "get_auth_fallback_status" && call.args.serverId === server.id));
  }
});
