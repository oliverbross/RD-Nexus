import * as logbookStore from './services/logbookStore.js';

const embedded = window.self !== window.top && new URLSearchParams(window.location.search).get('nexus') === '1';
window.__NEXUS_EMBEDDED__ = embedded;

if (embedded) {
  document.documentElement.dataset.nexusEmbed = 'true';
  let resizeFrame = 0;
  const publishViewport = () => {
    resizeFrame = 0;
    document.documentElement.style.setProperty('--nexus-embed-vh', `${window.innerHeight}px`);
    document.documentElement.style.setProperty('--nexus-embed-vw', `${window.innerWidth}px`);
  };
  const queueViewport = () => {
    if (resizeFrame) cancelAnimationFrame(resizeFrame);
    resizeFrame = requestAnimationFrame(publishViewport);
  };
  publishViewport();
  window.addEventListener('resize', queueViewport, { passive: true });
  window.visualViewport?.addEventListener('resize', queueViewport, { passive: true });
  window.addEventListener('load', queueViewport, { once: true });
  new MutationObserver(() => {
    if (
      !document.documentElement.style.getPropertyValue('--nexus-embed-vh') ||
      !document.documentElement.style.getPropertyValue('--nexus-embed-vw')
    ) queueViewport();
  }).observe(document.documentElement, { attributes: true, attributeFilter: ['style'] });
}

let clusterSpots = [];
let station = { callsign: '', grid: '' };
let rotator = { azimuth: null, live: false };
let wsjtx = { decodes: [], client: null };
let bridgeAlert = null;
let logSync = { revision: 0, nativeRevision: 0, total: 0, rows: [] };
let mirrorQueue = Promise.resolve();

const response = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' },
  });

const send = (type, payload = {}) => {
  if (embedded) window.parent.postMessage({ source: 'openhamclock', type, payload }, '*');
};

const queueMirrorCommit = (messageRevision, work) => {
  mirrorQueue = mirrorQueue.then(async () => {
    try {
      const result = await work();
      send('logbook:commit', { messageRevision, ...result });
    } catch (error) {
      send('logbook:commit', { messageRevision, status: 'failed', error: String(error) });
    }
  });
};

const jsonOr = async (res, fallback) => {
  try {
    return res.ok ? await res.clone().json() : fallback;
  } catch {
    return fallback;
  }
};

const mergeBy = (upstream, nexus, key) => {
  const merged = new Map();
  for (const value of [...(Array.isArray(upstream) ? upstream : []), ...(Array.isArray(nexus) ? nexus : [])]) {
    merged.set(key(value), value);
  }
  return [...merged.values()];
};

if (embedded) {
  const nativeFetch = window.fetch.bind(window);
  window.fetch = async (input, init = {}) => {
    const raw = typeof input === 'string' ? input : input?.url;
    const url = new URL(raw, window.location.href);
    if (url.origin === window.location.origin && url.pathname === '/api/dxcluster/paths') {
      const native = await nativeFetch(input, init).catch(() => null);
      const upstream = native ? await jsonOr(native, []) : [];
      return response(mergeBy(upstream, clusterSpots, (spot) => `${spot.dxCall || spot.call}|${spot.freq}|${spot.spotter}`));
    }
    if (url.origin === window.location.origin && url.pathname === '/api/wsjtx/decodes') {
      const since = Number(url.searchParams.get('since') || 0);
      const native = await nativeFetch(input, init).catch(() => null);
      const upstream = native ? await jsonOr(native, {}) : {};
      const nexusDecodes = since ? wsjtx.decodes.filter((decode) => decode.timestamp > since) : wsjtx.decodes;
      return response({ ...upstream,
        decodes: mergeBy(upstream.decodes, nexusDecodes, (decode) => decode.id || `${decode.timestamp}|${decode.message}`),
        timestamp: Date.now(),
      });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/wsjtx') {
      const native = await nativeFetch(input, init).catch(() => null);
      const upstream = native ? await jsonOr(native, {}) : {};
      const clients = { ...(upstream.clients || {}), ...(wsjtx.client ? { 'RD-Nexus': wsjtx.client } : {}) };
      const decodes = mergeBy(upstream.decodes, wsjtx.decodes, (decode) => decode.id || `${decode.timestamp}|${decode.message}`);
      return response({ ...upstream,
        enabled: upstream.enabled !== false || Boolean(wsjtx.client),
        clients,
        decodes,
        qsos: upstream.qsos || [],
        wspr: upstream.wspr || [],
        stats: {
          ...(upstream.stats || {}),
          totalDecodes: decodes.length,
          totalQsos: upstream.stats?.totalQsos || 0,
          totalWspr: upstream.stats?.totalWspr || 0,
          activeClients: Object.keys(clients).length,
        },
      });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/dxcluster/spots') {
      const native = await nativeFetch(input, init).catch(() => null);
      const upstream = native ? await jsonOr(native, []) : [];
      const nexus = clusterSpots.map((spot) => ({
          call: spot.dxCall,
          freq: spot.freq,
          comment: spot.comment,
          time: spot.time,
          spotter: spot.spotter,
          timestamp: spot.timestamp,
          source: 'Nexus',
        }));
      return response(mergeBy(upstream, nexus, (spot) => `${spot.call || spot.dxCall}|${spot.freq}|${spot.spotter}`));
    }
    if (url.origin === window.location.origin && url.pathname === '/api/dxcluster/spot' && init.method === 'POST') {
      send('cluster:post', JSON.parse(init.body || '{}'));
      const native = await nativeFetch(input, init).catch(() => null);
      return native?.ok ? native : response({ ok: true, source: 'nexus', upstreamStatus: native?.status || 0 });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/rotator/status') {
      const native = await nativeFetch(input, init).catch(() => null);
      const upstream = native ? await jsonOr(native, {}) : {};
      if (upstream.source && upstream.source !== 'none') return response(upstream, native.status);
      return response({ ...upstream, source: 'nexus', ...rotator, lastSeen: Date.now(), staleMs: 5000, error: null });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/rotator/turn' && init.method === 'POST') {
      const body = JSON.parse(init.body || '{}');
      const status = await nativeFetch('/api/rotator/status').then((res) => jsonOr(res, {})).catch(() => ({}));
      if (status.source && status.source !== 'none') return nativeFetch(input, init);
      send('rotator:turn', { azimuth: body.azimuth });
      return response({ ok: true, target: body.azimuth, source: 'nexus' });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/rotator/stop' && init.method === 'POST') {
      const status = await nativeFetch('/api/rotator/status').then((res) => jsonOr(res, {})).catch(() => ({}));
      if (status.source && status.source !== 'none') return nativeFetch(input, init);
      send('rotator:stop');
      return response({ ok: true, source: 'nexus' });
    }
    return nativeFetch(input, init);
  };

  window.addEventListener('message', (event) => {
    if (event.source !== window.parent || event.data?.source !== 'nexus') return;
    const { type, payload } = event.data;
    if (type === 'state') {
      station = payload.station || station;
      clusterSpots = Array.isArray(payload.spots) ? payload.spots : clusterSpots;
      rotator = payload.rotator || rotator;
      wsjtx = payload.wsjtx || wsjtx;
      if (payload.rig) window.dispatchEvent(new CustomEvent('nexus-rig-state', { detail: payload.rig }));
    } else if (type === 'logbook:begin' && Number.isInteger(payload?.revision) && payload.revision > logSync.revision) {
      logSync = { revision: payload.revision, nativeRevision: Number(payload.nativeRevision) || 0, total: Number(payload.total) || 0, rows: [] };
    } else if (type === 'logbook:chunk' && payload?.revision === logSync.revision && payload.offset === logSync.rows.length && Array.isArray(payload.rows)) {
      logSync.rows.push(...payload.rows);
    } else if (type === 'logbook:end' && payload?.revision === logSync.revision && logSync.rows.length === logSync.total) {
      const snapshot = logSync;
      queueMirrorCommit(snapshot.revision, () => logbookStore.mergeFromNexus(snapshot.rows, snapshot.nativeRevision));
    } else if (type === 'logbook:delta' && Array.isArray(payload?.upserts) && Array.isArray(payload?.deleted)) {
      queueMirrorCommit(payload.messageRevision, () => logbookStore.applyNexusDelta(payload));
    } else if (type === 'mutation-result' && payload?.ok === false) {
      if (!bridgeAlert) {
        bridgeAlert = document.createElement('div');
        bridgeAlert.setAttribute('role', 'alert');
        bridgeAlert.style.cssText = 'position:fixed;z-index:2147483647;left:12px;right:12px;bottom:12px;padding:10px 14px;border:2px solid #dc2626;border-radius:8px;background:#450a0a;color:#fff;font:600 14px system-ui';
        document.body.appendChild(bridgeAlert);
      }
      bridgeAlert.textContent = `Nexus integration: ${payload.error || 'operation failed'}`;
    }
  });

  window.addEventListener('DOMContentLoaded', () => send('ready', { station }));
}

export { embedded, send };
