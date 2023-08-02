import { memory } from "pulsar/pulsar_bg.wasm";
import { AudioDecoder } from "pulsar";
import WaveSurfer from "wavesurfer.js";

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

const slider = document.createElement("input");
slider.type = "range";
slider.setAttribute("min", 10);
slider.setAttribute("max", 1000);
slider.setAttribute("value", 100);

app.appendChild(document.createElement("h2")).textContent = "Zoom";
app.appendChild(slider);
app.appendChild(document.createElement("h2")).textContent =
  "WaveSurfer w/ Pulsar";
app.appendChild(document.createElement("div")).id = "waveform1";

app.appendChild(document.createElement("h2")).textContent = "WaveSurfer direct";
app.appendChild(document.createElement("div")).id = "waveform2";

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
  .decode((msg) => {
    console.log(msg);
  })
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

    const w1 = WaveSurfer.create({
      container: waveform1,
      waveColor: "rgb(180, 180, 180)",
      url: sampleOgg,
      peaks: [samples],
    });

    const w2 = WaveSurfer.create({
      container: waveform2,
      waveColor: "rgb(180, 180, 180)",
      url: sampleOgg,
      // peaks: [samples],
    });

    slider.addEventListener("input", (e) => {
      const minPxPerSec = e.target.valueAsNumber;
      w1.zoom(minPxPerSec);
      w2.zoom(minPxPerSec);
    });
  })
  .catch((err) => {
    console.error(formatError(err));
  });
