'use strict';
// Tiny IndexedDB wrapper: projects (JSON) and recorded audio (Float32 PCM).
const DB = (() => {
  let dbp = null;
  function open() {
    if (!dbp) {
      dbp = new Promise((res, rej) => {
        const r = indexedDB.open('pocketstudio', 1);
        r.onupgradeneeded = () => {
          const db = r.result;
          db.createObjectStore('projects', { keyPath: 'id' });
          db.createObjectStore('audio');
        };
        r.onsuccess = () => res(r.result);
        r.onerror = () => rej(r.error);
      });
    }
    return dbp;
  }
  async function tx(store, mode, fn) {
    const db = await open();
    return new Promise((res, rej) => {
      const t = db.transaction(store, mode);
      const req = fn(t.objectStore(store));
      let out;
      if (req) req.onsuccess = () => { out = req.result; };
      t.oncomplete = () => res(out);
      t.onerror = () => rej(t.error);
      t.onabort = () => rej(t.error);
    });
  }
  return {
    getProject: id => tx('projects', 'readonly', s => s.get(id)),
    putProject: p => tx('projects', 'readwrite', s => s.put(p)),
    deleteProject: id => tx('projects', 'readwrite', s => s.delete(id)),
    listProjects: () => tx('projects', 'readonly', s => s.getAll()),
    getAudio: id => tx('audio', 'readonly', s => s.get(id)),
    putAudio: (id, data) => tx('audio', 'readwrite', s => s.put(data, id)),
    deleteAudio: id => tx('audio', 'readwrite', s => s.delete(id)),
  };
})();

const Settings = {
  get(k, d) { try { const v = localStorage.getItem('ps.' + k); return v == null ? d : JSON.parse(v); } catch (e) { return d; } },
  set(k, v) { try { localStorage.setItem('ps.' + k, JSON.stringify(v)); } catch (e) { /* ignore */ } },
};
