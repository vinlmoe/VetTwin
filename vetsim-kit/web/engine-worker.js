// Calculs longs du moteur hors du fil de l'interface (voir BackgroundEngine dans engine.js).
// Message reçu : {id, method, args} ; réponse : {id, result} ou {id, error}.
import { Controller } from './engine.js';

let ctl = null;
self.onmessage = async (ev) => {
  const { id, method, args } = ev.data;
  try {
    ctl ??= await Controller.create();
    if (!['treatment', 'evaluate', 'heart', 'auscultate'].includes(method)) throw new Error(`méthode inconnue « ${method} »`);
    self.postMessage({ id, result: ctl[method](...args) });
  } catch (err) {
    self.postMessage({ id, error: String(err && err.message ? err.message : err) });
  }
};
