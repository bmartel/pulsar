import { memory } from "pulsar/pulsar_bg.wasm";
import { AudioDecoder } from "pulsar";

const sampleOgg =
  "https://raw.githubusercontent.com/bmartel/pulsar/main/www/assets/sample_10MB_OGG.ogg";
const sampleMp3 =
  "https://raw.githubusercontent.com/bmartel/pulsar/main/www/assets/sample_1OMB_MP3.mp3";
const sampleWav =
  "https://raw.githubusercontent.com/bmartel/pulsar/main/www/assets/sample_10MB_WAV.wav";

app.appendChild(document.createElement("h1")).textContent = "Pulsar";
const audio = document.createElement("audio");
audio.src = sampleOgg;
audio.controls = true;
app.appendChild(audio);

const decoder = AudioDecoder.new(sampleOgg);

const formatTime = (ms) => {
  const date = new Date(null);
  date.setSeconds(Number(ms));

  return date.toISOString().substr(11, 8);
};

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

decoder
  .decode()
  .then(() => {
    const err = decoder.get_last_error();
    const samplesPtr = decoder.get_samples();
    const samples = new Float32Array(
      memory.buffer,
      samplesPtr,
      decoder.get_samples_len()
    );
    console.log("error?", err);
    console.log("channels", decoder.get_channels());
    console.log("sample_rate", decoder.get_sample_rate());
    console.log(
      "duration",
      decoder.get_duration_str(),
      formatTime(decoder.get_duration())
    );
    console.log("samples", samples);
  })
  .catch((err) => {
    console.error(formatError(err));
  });
