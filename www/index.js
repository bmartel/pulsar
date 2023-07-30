import { memory } from "pulsar/pulsar_bg.wasm";
import { AudioDecoder } from "pulsar";

const sampleOgg =
  "https://raw.githubusercontent.com/bmartel/pulsar/main/www/assets/sample_10MB_OGG.ogg";
const sampleMp3 =
  "https://raw.githubusercontent.com/bmartel/pulsar/main/www/assets/sample_1OMB_MP3.mp3";
const sampleWav =
  "https://raw.githubusercontent.com/bmartel/pulsar/main/www/assets/sample_10MB_WAV.wav";

const decoder = AudioDecoder.new(sampleOgg);

decoder
  .decode()
  .then(() => {
    const err = decoder.get_last_error();
    console.log(err);
    const samplesPtr = decoder.get_samples();
    const samples = new Float32Array(
      memory.buffer,
      samplesPtr,
      decoder.get_samples_len()
    );
    console.log(samples);
  })
  .catch((err) => {
    console.error(JSON.parse(err));
  });
