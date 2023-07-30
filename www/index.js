import { memory } from "pulsar/pulsar_bg.wasm";
import { AudioDecoder } from "pulsar";

// const decoder = AudioDecoder.new(
// );

// decoder
//   .decode()
//   .then(() => {
//     const err = decoder.get_last_error();
//     console.log(err);
//     const samplesPtr = decoder.get_samples();
//     const samples = new Float32Array(
//       memory.buffer,
//       samplesPtr,
//       decoder.get_samples_len()
//     );
//     console.log(samples);
//   })
//   .catch((err) => {
//     console.error(JSON.parse(err));
//   });
