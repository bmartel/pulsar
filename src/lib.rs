mod utils;

use crate::utils::set_panic_hook;
use wasm_bindgen::prelude::*;
use std::io::{Read, Seek};
use std::fmt;
use reqwest::Client;
use serde::{Serialize, Deserialize};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSource;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::TimeBase;

fn fmt_time(ts: u64, tb: TimeBase) -> String {
    let time = tb.calc_time(ts);

    let hours = time.seconds / (60 * 60);
    let mins = (time.seconds % (60 * 60)) / 60;
    let secs = f64::from((time.seconds % 60) as u32) + time.frac;

    format!("{}:{:0>2}:{:0>6.3}", hours, mins, secs)
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date)]
    fn now() -> f64;
}

pub fn get_unix_timestamp_millis() -> u128 {
    now() as u128
}

pub enum AudioErrorKind {
    IoError,
    DecodeError,
    SeekError,
    UnsupportedError,
    LimitError,
    ResetRequiredError,
    RequestError,
    UnknownError,
}

impl fmt::Display for AudioErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AudioErrorKind::IoError => write!(f, "IoError"),
            AudioErrorKind::DecodeError => write!(f, "DecodeError"),
            AudioErrorKind::SeekError => write!(f, "SeekError"),
            AudioErrorKind::UnsupportedError => write!(f, "UnsupportedError"),
            AudioErrorKind::LimitError => write!(f, "LimitError"),
            AudioErrorKind::ResetRequiredError => write!(f, "ResetRequiredError"),
            AudioErrorKind::RequestError => write!(f, "RequestError"),
            AudioErrorKind::UnknownError => write!(f, "UnknownError"),
        }
    }
}

pub struct AudioError {
    message: String,
    kind: AudioErrorKind,
}

impl AudioError {
    pub fn new(message: String, kind: AudioErrorKind) -> Self {
        AudioError {
            kind: kind,
            message: message,
        }
    }

    pub fn get_message(&self) -> String {
        format!("{{\"message\":\"{}\",\"kind\":\"{}\"}}", self.message, self.kind.to_string()) 
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // JSON string of the error. No serde_json
        write!(f, "{}", self.get_message())
    }
}

pub struct StreamableFile {
    url: String,
    buffer: Vec<u8>,
    read_position: usize,
}

impl StreamableFile {
    pub fn new(url:String, buffer: Vec<u8>) -> Self {
        StreamableFile {
            url,
            buffer: buffer,
            read_position: 0,
        }
    }

    async fn get_size(&mut self) -> usize {

        // Get the size of the file we are streaming.
        let res = Client::new().head(&self.url)
            .send()
            .await.unwrap();

        let header = res
            .headers().get("Content-Length")
            .unwrap();

        header
            .to_str()
            .unwrap()
            .parse()
            .unwrap()
    }

    async fn init_stream(&mut self) {
        if self.buffer.is_empty() {
            let size = self.get_size().await;

            self.buffer = vec![0; size];
        }
    }
}

impl Read for StreamableFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // If we are reading after the buffer,
        // then return early with 0 written bytes.
        if self.read_position >= self.buffer.len() {
            return Ok(0);
        }

        // This defines the end position of the packet
        // we want to read.
        let read_max = (self.read_position + buf.len()).min(self.buffer.len());

        // These are the bytes that we want to read.
        let bytes = &self.buffer[self.read_position..read_max];
        buf[0..bytes.len()].copy_from_slice(bytes);

        self.read_position += bytes.len();
        Ok(bytes.len())
    }
}

impl Seek for StreamableFile {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let seek_position:usize = match pos {
            std::io::SeekFrom::Start(pos) => pos as usize,
            std::io::SeekFrom::Current(pos) => {
                let pos = self.read_position as i64 + pos;
                pos.try_into().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput, 
                        format!("Invalid seek: {pos}")
                    )
                })?
            },
            std::io::SeekFrom::End(pos) => {
                let pos = self.buffer.len() as i64 + pos;
                pos.try_into().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput, 
                        format!("Invalid seek: {pos}")
                    )
                })?
            },
        };

        if seek_position > self.buffer.len() {
            // log!("Seek position {seek_position} > file size");
            return Ok(self.read_position as u64);
        }

        // log!("Seeking: pos[{seek_position}] type[{pos:?}]");

        self.read_position = seek_position;

        Ok(seek_position as u64)
    }
}

unsafe impl Send for StreamableFile {}
unsafe impl Sync for StreamableFile {}

impl MediaSource for StreamableFile {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.buffer.len() as u64)
    }
}


#[derive(Serialize, Deserialize)]
pub struct DecodingResult {
    pub data: Option<Vec<f32>>,
    pub error: Option<String>,
}

async fn fetch_audio_file(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    let res = reqwest::get(url).await?;
    let bytes = res.bytes().await?;
    
    Ok(bytes.to_vec())
}

#[wasm_bindgen]
pub struct AudioDecoder {
    url: String,
    samples: Vec<f32>,
    duration: u64,
    time_base: TimeBase,
    sample_rate: u32,
    channels: usize,
    samples_len: usize,
    estimate_samples_len: usize,
    error: Option<String>,
    gapless: bool,
}

#[wasm_bindgen]
impl AudioDecoder {
    pub fn new(url: &str, gapless: Option<bool>) -> AudioDecoder {
        set_panic_hook();

        AudioDecoder {
            url: url.to_string(),
            samples: Vec::new(),
            error: None,
            duration: 0,
            time_base: TimeBase::default(),
            sample_rate: 0,
            channels: 0,
            samples_len: 0,
            estimate_samples_len: 0,
            gapless: gapless.unwrap_or(true),
        }
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn get_channels(&self) -> usize {
        self.channels
    }

    // Returns the duration in seconds based on the time base.
    pub fn get_duration(&self) -> u64 {
        self.time_base.calc_time(self.duration).seconds
    }

    pub fn get_duration_str(&self) -> String {
        fmt_time(self.duration, self.time_base)
    }

    pub fn get_samples_len(&self) -> usize {
        self.samples_len
    }

    pub fn get_progress(&self, current_samples_len: usize) -> f32 {
        (current_samples_len as f32 / self.estimate_samples_len as f32) * 100.0
    }

    pub fn get_samples(&self) -> *const f32 {
        self.samples.as_ptr()
    }

    pub fn get_last_error(&self) -> Option<String> {
        self.error.clone()
    }

    async fn decode_audio(&mut self, bytes: Option<Vec<u8>>, callback: &js_sys::Function) -> Result<Vec<f32>, AudioError> {
        let should_init = bytes.is_none();
        let mut file = StreamableFile::new(self.url.to_string(), bytes.unwrap_or_default());
        if should_init {
            file.init_stream().await;
        }
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut format_opts: FormatOptions = Default::default();
        format_opts.enable_gapless = self.gapless;
        let metadata_opts: MetadataOptions = Default::default();
        let decoder_opts: DecoderOptions = Default::default();
        let mut hint = Hint::new();

        let probe = match symphonia::default::get_probe().format(&mut hint, stream, &format_opts, &metadata_opts) {
            Ok(probe) => probe,
            Err(e) => {
                let msg = e.to_string();

                match e {
                    Error::IoError(_) => {
                        return Err(AudioError::new(msg, AudioErrorKind::IoError))
                    },
                    Error::Unsupported(_) => {
                        return Err(AudioError::new(msg, AudioErrorKind::UnsupportedError))
                    },
                    _ => {
                        return Err(AudioError::new(msg, AudioErrorKind::UnknownError))
                    }
                }
            }
        };
        let mut format = probe.format;

        let track = format.default_track().unwrap();
        let track_id = track.id;
        if let Some(duration) = track.codec_params.n_frames.map(|frames| track.codec_params.start_ts + frames) {
            self.duration = duration;
        }
        if let Some(time_base) = track.codec_params.time_base {
            self.time_base = time_base;
        }
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts).unwrap();
        let mut sample_count = 0;
        let mut sample_buf = None;
        let mut samples = Vec::new();
        let mut err: Option<AudioError> = None;
        let this = JsValue::NULL;

        loop {
            // Get the next packet from the format reader.
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(e) => {
                    let msg = e.to_string();
                    if msg == "end of stream" {
                        break;
                    }
                    match e {
                        Error::IoError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::IoError));
                            break;
                        },
                        Error::DecodeError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::DecodeError));
                            break;
                        },
                        Error::SeekError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::SeekError));
                            break;
                        },
                        Error::Unsupported(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::UnsupportedError));
                            break;
                        },
                        Error::LimitError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::LimitError));
                            break;
                        },
                        Error::ResetRequired => {
                            err = Some(AudioError::new(msg, AudioErrorKind::ResetRequiredError));
                            break;
                        },
                        _ => {
                            err = Some(AudioError::new(msg, AudioErrorKind::UnknownError));
                            break;
                        }
                    }
                },
            };

            // If the packet does not belong to the selected track, skip it.
            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(audio_buf) => {
                    // The decoded audio samples may now be accessed via the audio buffer if per-channel
                    // slices of samples in their native decoded format is desired. Use-cases where
                    // the samples need to be accessed in an interleaved order or converted into
                    // another sample format, or a byte buffer is required, are covered by copying the
                    // audio buffer into a sample buffer or raw sample buffer, respectively. In the
                    // example below, we will copy the audio buffer into a sample buffer in an
                    // interleaved order while also converting to a f32 sample format.

                    // If this is the *first* decoded packet, create a sample buffer matching the
                    // decoded audio buffer format.
                    if sample_buf.is_none() {
                        // Get the audio buffer specification.
                        let spec = *audio_buf.spec();

                        // Get the capacity of the decoded buffer. Note: This is capacity, not length!
                        let capacity = audio_buf.capacity() as u64;

                        // Create the f32 sample buffer.
                        sample_buf = Some(SampleBuffer::<f32>::new(capacity, spec));

                        self.sample_rate = spec.rate;
                        self.channels = spec.channels.count();
                        // Estimate the number of samples in the total decoded audio buffer.
                        // Note: This is an estimate, not an exact value!
                        self.estimate_samples_len = (self.channels as u64 * self.sample_rate as u64 * self.get_duration() as u64) as usize;
                    }

                    // Copy the decoded audio buffer into the sample buffer in an interleaved format.
                    if let Some(buf) = &mut sample_buf {
                        buf.copy_interleaved_ref(audio_buf);

                        // The samples may now be access via the `samples()` function.
                        sample_count += buf.samples().len();
                        samples.extend_from_slice(buf.samples());
                    }

                    let progress = self.get_progress(sample_count);

                    if progress == 0.0 {
                        let sample_rate = JsValue::from(self.sample_rate);
                        let channels = JsValue::from(self.channels);
                        let duration = JsValue::from(self.get_duration());
                        let _ = callback.call3(&this, &sample_rate, &channels, &duration);
                    } else {
                        let value = JsValue::from(progress);
                        let _ = callback.call1(&this, &value);
                    }
                },
                Err(e) => {
                    let msg = e.to_string();
                    if msg == "end of stream" {
                        break;
                    }
                    match e {
                        Error::IoError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::IoError));
                            break;
                        },
                        Error::DecodeError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::DecodeError));
                            break;
                        },
                        Error::SeekError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::SeekError));
                            break;
                        },
                        Error::Unsupported(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::UnsupportedError));
                            break;
                        },
                        Error::LimitError(_) => {
                            err = Some(AudioError::new(msg, AudioErrorKind::LimitError));
                            break;
                        },
                        Error::ResetRequired => {
                            err = Some(AudioError::new(msg, AudioErrorKind::ResetRequiredError));
                            break;
                        },
                        _ => {
                            err = Some(AudioError::new(msg, AudioErrorKind::UnknownError));
                            break;
                        }
                    }
                },
            }
        }

        if err.is_some() {
            return Err(err.unwrap());
        }

        self.samples_len = sample_count;
        log!("\rDecoded {} samples", self.samples_len);
        Ok(samples)
    }

    pub async fn decode(&mut self, callback: &js_sys::Function) -> Result<(), JsValue> {
        let bytes = match fetch_audio_file(&self.url).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let err = AudioError::new(e.to_string(), AudioErrorKind::RequestError);
                self.error = Some(err.get_message());
                return Err(JsValue::from_str(&self.error.clone().unwrap()));
            }
        };

        let result = self.decode_audio(Some(bytes), callback).await;

        match result {
            Ok(data) => self.samples = data,
            Err(e) => {
                self.error = Some(e.get_message());
            },
        };

        if self.error.is_some() {
            Err(JsValue::from_str(&self.error.clone().unwrap()))
        } else {
            Ok(())
        }
    }
}
