import WaveSurfer from "wavesurfer.js";
import { AudioDecoderWorker } from "./decoder";

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

// app.appendChild(document.createElement("h2")).textContent = "WaveSurfer direct";
// app.appendChild(document.createElement("div")).id = "waveform2";

const decoder = new AudioDecoderWorker(sampleOgg);

decoder.on("metadata", () => {
  console.log("metadata", decoder.metadata);
});

let w1 = null;
// let w2 = null;

decoder
  .decode()
  .then(() => {
    w1 = WaveSurfer.create({
      container: waveform1,
      waveColor: "rgb(180, 180, 180)",
      url: sampleOgg,
      peaks: [decoder.samples],
    });

    slider.addEventListener("input", (e) => {
      const minPxPerSec = e.target.valueAsNumber;
      w1.zoom(minPxPerSec);
      // w2.zoom(minPxPerSec);
    });
  })
  .catch((err) => {
    console.error(err.message);
  });

// setTimeout(() => {
//   w2 = WaveSurfer.create({
//     container: waveform2,
//     waveColor: "rgb(180, 180, 180)",
//     url: sampleOgg,
//     // peaks: [samples],
//   });
// }, 1);
