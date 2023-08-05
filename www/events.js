export class EventEmitter {
  constructor() {
    this.listeners = new Map();
  }

  on(event, listener) {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }

    this.listeners.get(event).add(listener);
  }

  off(event, listener) {
    if (!this.listeners.has(event)) return;

    this.listeners.get(event).delete(listener);
  }

  emit(event, ...args) {
    if (!this.listeners.has(event)) return;

    for (const listener of this.listeners.get(event)) {
      listener(...args);
    }
  }

  clear() {
    this.listeners.clear();
  }
}
