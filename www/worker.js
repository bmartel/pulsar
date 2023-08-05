const activeJobs = new Map();

const formatError = (err) => {
  if (!err) {
    return null;
  }

  if (err instanceof Error) {
    return err.message;
  }

  if (err.startsWith("{") && err.endsWith("}")) {
    return JSON.parse(err);
  }

  return err;
};

const sendMessage = (type, payload) => {
  postMessage({ type, payload });
};

const sendError = ({ id, err }) => {
  sendMessage("error", { id, payload: formatError(err) });
};

const decode = ({ id, url }) => {
  return (() => {
    let cancelled = false;
    let metadata = false;

    const run = async () => {
      const memory = await import("pulsar/pulsar_bg.wasm").then(
        (wasm) => wasm.memory
      );
      const AudioDecoder = await import("pulsar").then(
        (wasm) => wasm.AudioDecoder
      );

      const decoder = AudioDecoder.new(url);

      decoder
        .decode((data, channels, duration) => {
          if (!metadata && channels && duration) {
            metadata = true;
            const payload = {
              channels,
              sample_rate: data,
              duration: Number(duration),
            };
            sendMessage("metadata", { id, payload });
          }
        })
        .then(() => {
          if (cancelled) {
            return;
          }
          const samplesPtr = decoder.get_samples();
          const samples = new Float32Array(
            memory.buffer,
            samplesPtr,
            decoder.get_samples_len()
          );
          sendMessage("samples", { id, payload: samples });
          activeJobs.delete(id);
        })
        .catch((err) => {
          sendError({ id, err });
          activeJobs.delete(id);
        });
    };

    run();

    return {
      cancel: () => {
        cancelled = true;
        // decoder.cancel();
        sendMessage("cancel", { id });
        activeJobs.delete(id);
      },
    };
  })();
};

onmessage = (event) => {
  const { type, payload } = event.data;
  switch (type) {
    case "decode":
      activeJobs.set(payload.id, decode(payload));
      break;
    case "cancel":
      const job = activeJobs.get(payload.id);
      if (job) {
        job.cancel();
      }
      break;
    default:
      console.error("unknown message type", type);
  }
};
