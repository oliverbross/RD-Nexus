import * as logbookStore from './services/logbookStore.js';

const embedded = window.self !== window.top && new URLSearchParams(window.location.search).get('nexus') === '1';
window.__NEXUS_EMBEDDED__ = embedded;

let clusterSpots = [];
let station = { callsign: '', grid: '' };
let rotator = { azimuth: null, live: false };
let wsjtx = { decodes: [], client: null };

const response = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' },
  });

const send = (type, payload = {}) => {
  if (embedded) window.parent.postMessage({ source: 'openhamclock', type, payload }, '*');
};

if (embedded) {
  const nativeFetch = window.fetch.bind(window);
  window.fetch = async (input, init = {}) => {
    const raw = typeof input === 'string' ? input : input?.url;
    const url = new URL(raw, window.location.href);
    if (url.origin === window.location.origin && url.pathname === '/api/dxcluster/paths') {
      return response(clusterSpots);
    }
    if (url.origin === window.location.origin && url.pathname === '/api/wsjtx/decodes') {
      const since = Number(url.searchParams.get('since') || 0);
      return response({
        decodes: since ? wsjtx.decodes.filter((decode) => decode.timestamp > since) : wsjtx.decodes,
        timestamp: Date.now(),
      });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/wsjtx') {
      const clients = wsjtx.client ? { 'RD-Nexus': wsjtx.client } : {};
      return response({
        enabled: true,
        port: 0,
        relayEnabled: false,
        relayConnected: false,
        clients,
        decodes: wsjtx.decodes,
        qsos: [],
        wspr: [],
        stats: {
          totalDecodes: wsjtx.decodes.length,
          totalQsos: 0,
          totalWspr: 0,
          activeClients: Object.keys(clients).length,
        },
      });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/dxcluster/spots') {
      return response(
        clusterSpots.map((spot) => ({
          call: spot.dxCall,
          freq: spot.freq,
          comment: spot.comment,
          time: spot.time,
          spotter: spot.spotter,
          timestamp: spot.timestamp,
        })),
      );
    }
    if (url.origin === window.location.origin && url.pathname === '/api/dxcluster/spot' && init.method === 'POST') {
      send('cluster:post', JSON.parse(init.body || '{}'));
      return response({ ok: true, source: 'nexus' });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/rotator/status') {
      return response({ source: 'nexus', ...rotator, lastSeen: Date.now(), staleMs: 5000, error: null });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/rotator/turn' && init.method === 'POST') {
      const body = JSON.parse(init.body || '{}');
      send('rotator:turn', { azimuth: body.azimuth });
      return response({ ok: true, target: body.azimuth });
    }
    if (url.origin === window.location.origin && url.pathname === '/api/rotator/stop' && init.method === 'POST') {
      send('rotator:stop');
      return response({ ok: true });
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
      if (Array.isArray(payload.logbook)) void logbookStore.replaceFromNexus(payload.logbook);
      if (payload.rig) window.dispatchEvent(new CustomEvent('nexus-rig-state', { detail: payload.rig }));
    }
  });

  window.addEventListener('DOMContentLoaded', () => send('ready', { station }));
}

export { embedded, send };
