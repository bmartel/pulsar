import { EventEmitter } from "./events";

const decoderWorker = new Worker(new URL("./worker.js", import.meta.url));

export class AudioDecoderWorker extends EventEmitter {
  static jobs = new Map();

  constructor(url) {
    super();
    this.url = url;
    this.samples = null;
    this.metadata = null;
    this.error = null;
    this.activeJobId = null;

    decoderWorker.addEventListener("message", this.handle);
  }

  handle = (event) => {
    const { type, payload: _payload } = event.data;

    if (_payload?.id !== this.activeJobId) return;

    const payload = _payload.payload;

    switch (type) {
      case "metadata":
        this.metadata = payload;
        break;
      case "samples":
        this.samples = payload;
        break;
      case "error":
        this.error = payload;
        break;
    }

    this.emit(type);

    if (["samples", "error", "cancel"].includes(type)) {
      const job = AudioDecoderWorker.jobs.get(this.activeJobId);
      if (job) {
        if (type === "error") {
          job.reject(payload);
        } else {
          job.resolve();
        }
        AudioDecoderWorker.jobs.delete(this.activeJobId);
        this.activeJobId = null;
      }
    }
  };

  createId() {
    return Math.random().toString(36).substr(2, 9);
  }

  decode() {
    return new Promise((resolve, reject) => {
      const id = this.createId();
      AudioDecoderWorker.jobs.set(id, { resolve, reject });
      this.activeJobId = id;

      decoderWorker.postMessage({
        type: "decode",
        payload: { id, url: this.url },
      });
    });
  }

  cancel() {
    if (this.activeJobId) {
      decoderWorker.postMessage({
        type: "cancel",
        payload: { id: this.activeJobId },
      });
    }
  }

  destroy() {
    this.cancel();
    this.samples = null;
    this.metadata = null;
    this.error = null;
    this.url = null;
    decoderWorker.removeEventListener("message", this.handle);
  }
}
