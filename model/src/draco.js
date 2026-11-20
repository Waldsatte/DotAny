// Google's Draco decoder, as three.js ships it, only the part glTF needs. It travels inside the program and is loaded
// only when a model asks for it. Decoding only: nothing is ever written back compressed.
import { DRACOLoader } from 'three/examples/jsm/loaders/DRACOLoader.js';
import { DRACO_WRAPPER, DRACO_WASM } from './draco-bin.js';

// a worker that can't start says so instead of leaving the file waiting forever
export class PackedDraco extends DRACOLoader {
  _loadLibrary(url, type) {
    return Promise.resolve(
      type === 'text' ? DRACO_WRAPPER : Uint8Array.from(atob(DRACO_WASM), (c) => c.charCodeAt(0)).buffer
    );
  }
  _getWorker(id, cost) {
    return WebAssembly.compile(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]))
      .catch(() => {
        throw new Error('draco worker: no wasm');
      })
      .then(() => super._getWorker(id, cost))
      .then((w) => {
        w.onerror = (e) => {
          for (const k of Object.keys(w._callbacks))
            w._callbacks[k].reject(new Error('draco worker: ' + (e.message || 'failed')));
        };
        return w;
      });
  }
}
