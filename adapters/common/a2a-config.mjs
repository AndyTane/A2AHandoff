// Shared reader for runtime/config.json.
//
// Rust (`crates/handoff-core/src/config.rs`) owns the schema. This module must not
// invent different defaults: the constants below mirror the Rust ones, and every
// Node adapter reads configuration through these helpers.
import {readFileSync} from 'node:fs';
import {join} from 'node:path';

export const CONFIG_DEFAULTS = Object.freeze({
  poll_seconds: 60,
  dispatch_delay_seconds: 10,
  dsh_web_origin: 'http://127.0.0.1:3080',
  dsh_browser_processes: Object.freeze(['msedge', 'chrome']),
  claude_host: 'claude.ai',
  dsh_page_title_pattern: 'DeepSeek Harness',
});

// A missing or broken config never stops a read: defaults are returned instead,
// matching the Rust loader.
export function readRuntimeConfig(product) {
  try {
    const raw = readFileSync(join(product, 'runtime/config.json'), 'utf8').replace(/^\uFEFF/, '');
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

export function dshDataHome(cfg) {
  return typeof cfg?.dsh_data_home === 'string' ? cfg.dsh_data_home.trim() : '';
}

// Returns {scheme, host, port}; an unusable value falls back to the default origin.
export function dshWebOrigin(cfg) {
  const text = (typeof cfg?.dsh_web_origin === 'string' ? cfg.dsh_web_origin : CONFIG_DEFAULTS.dsh_web_origin).trim();
  const match = /^(https?):\/\/([^/?#]+)/i.exec(text);
  if (!match) return parseOrigin(CONFIG_DEFAULTS.dsh_web_origin);
  const scheme = match[1].toLowerCase();
  const authority = match[2].split('@').pop();
  const [host, portText] = authority.includes(':') ? authority.split(/:(?=[^:]*$)/) : [authority, ''];
  const port = portText && /^\d+$/.test(portText) ? Number(portText) : (scheme === 'https' ? 443 : 80);
  const portOk = Number.isInteger(port) && port >= 1 && port <= 65535;
  if (!host || !portOk) return parseOrigin(CONFIG_DEFAULTS.dsh_web_origin);
  return {scheme, host: host.toLowerCase(), port};
}

function parseOrigin(text) {
  const m = /^(https?):\/\/([^:/?#]+)(?::(\d+))?/i.exec(text);
  const scheme = m[1].toLowerCase();
  return {scheme, host: m[2].toLowerCase(), port: m[3] ? Number(m[3]) : (scheme === 'https' ? 443 : 80)};
}

export function dshBrowserProcesses(cfg) {
  const raw = Array.isArray(cfg?.dsh_browser_processes) ? cfg.dsh_browser_processes : [];
  const names = raw.map(v => String(v).trim().toLowerCase()).filter(Boolean);
  return names.length ? names : [...CONFIG_DEFAULTS.dsh_browser_processes];
}

export function claudeHost(cfg) {
  const value = typeof cfg?.claude_host === 'string' ? cfg.claude_host.trim().toLowerCase() : '';
  return value || CONFIG_DEFAULTS.claude_host;
}

export function dshPageTitlePattern(cfg) {
  const value = typeof cfg?.dsh_page_title_pattern === 'string' ? cfg.dsh_page_title_pattern : '';
  return value.trim() ? value : CONFIG_DEFAULTS.dsh_page_title_pattern;
}

// Empty workspace means the extra cross-check is disabled.
export function workspace(cfg) {
  return typeof cfg?.workspace === 'string' ? cfg.workspace.trim() : '';
}

export function autoEnabled(cfg) {
  return cfg?.enabled === true;
}
