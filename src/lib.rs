mod utils;

use crate::utils::set_panic_hook;
use wasm_bindgen::prelude::*;
use std::io::{Read, Seek};
use std::sync::atomic::AtomicBool;
use std::thread;
use std::fmt;
use std::error::Error as StdError;
use std::sync::mpsc::{channel, Receiver, Sender};
use rangemap::RangeSet;
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
use tokio::runtime::Builder;

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
    pub fn new(e: Error, kind: AudioErrorKind) -> Self {
        AudioError {
            kind: kind,
            message: e.to_string(),
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

// Used in cpal_output.rs to mute the stream when buffering.
pub static IS_STREAM_BUFFERING: AtomicBool = AtomicBool::new(false);

const CHUNK_SIZE:usize = 1024 * 128;
const FETCH_OFFSET:usize = CHUNK_SIZE / 2;

pub struct StreamableFile
{
    url: String,
    buffer: Vec<u8>,
    read_position: usize,
    downloaded: RangeSet<usize>,
    requested: RangeSet<usize>,
    receivers: Vec<(u128, Receiver<(usize, Vec<u8>)>)>
}

impl StreamableFile
{
    pub fn new(url:String, bytes: Option<Vec<u8>>) -> Self
    {
        let buffer = bytes.unwrap_or_default();
        let downloaded = if buffer.is_empty() {
            RangeSet::new()
        } else {
            let mut rs = RangeSet::<usize>::new();
            rs.insert(0..buffer.len());
            rs
        };

        StreamableFile
        {
            url,
            buffer: buffer,
            read_position: 0,
            downloaded: downloaded,
            requested: RangeSet::new(),
            receivers: Vec::new()
        }
    }

    async fn init_stream(&mut self) {
        if self.buffer.is_empty() {
            // Get the size of the file we are streaming.
            let res = Client::new().head(&self.url)
                .send()
                .await.unwrap();

            let header = res
                .headers().get("Content-Length")
                .unwrap();

            let size:usize = header
                .to_str()
                .unwrap()
                .parse()
                .unwrap();

            log!("{size}");

            self.buffer = vec![0; size];
        }
    }

    /// Gets the next chunk in the sequence.
    /// 
    /// Returns the received bytes by sending them via `tx`.
    async fn read_chunk(tx:Sender<(usize, Vec<u8>)>, url:String, start:usize, file_size:usize)
    {
        let end = (start + CHUNK_SIZE).min(file_size);

        let chunk = Client::new().get(url)
            .header("Range", format!("bytes={start}-{end}"))
            .send().await.unwrap().bytes().await.unwrap().to_vec();
        
        tx.send((start, chunk)).unwrap();
    }

    /// Polls all receivers.
    /// 
    /// If there is data to receive, then write it to the buffer.
    /// 
    /// Changes made are commited to `downloaded`.
    fn try_write_chunk(&mut self, should_buffer:bool)
    {
        let mut completed_downloads = Vec::new();

        for (id, rx) in &self.receivers
        {
            // Block on the first chunk or when buffering.
            // Buffering fixes the issue with seeking on MP3 (no blocking on data).
            let result = if self.downloaded.is_empty() || should_buffer {
                rx.recv().ok()
            } else { rx.try_recv().ok() };

            match result
            {
                None => (),
                Some((position, chunk)) => {
                    // Write the data.
                    let end = (position + chunk.len()).min(self.buffer.len());

                    if position != end {
                        self.buffer[position..end].copy_from_slice(chunk.as_slice());
                        self.downloaded.insert(position..end);
                    }

                    // Clean up.
                    completed_downloads.push(*id);
                }
            }
        }

        // Remove completed receivers.
        self.receivers.retain(|(id, _)| !completed_downloads.contains(&id));
    }

    /// Determines if a chunk should be downloaded by getting
    /// the downloaded range that contains `self.read_position`.
    /// 
    /// Returns `true` and the start index of the chunk
    /// if one should be downloaded.
    fn should_get_chunk(&self, buf_len:usize) -> (bool, usize)
    {
        let closest_range = self.downloaded.get(&self.read_position);

        if closest_range.is_none() {
            return (true, self.read_position);
        }

        let closest_range = closest_range.unwrap();
        
        // Make sure that the same chunk isn't being downloaded again.
        // This may happen because the next `read` call happens
        // before the chunk has finished downloading. In that case,
        // it is unnecessary to request another chunk.
        let is_already_downloading = self.requested.contains(&(self.read_position + CHUNK_SIZE));

        let should_get_chunk = self.read_position + buf_len >= closest_range.end - FETCH_OFFSET
            && !is_already_downloading
            && closest_range.end != self.buffer.len();
        
        (should_get_chunk, closest_range.end)
    }
}

impl Read for StreamableFile
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>
    {
        // If we are reading after the buffer,
        // then return early with 0 written bytes.
        if self.read_position >= self.buffer.len() {
            return Ok(0);
        }

        // This defines the end position of the packet
        // we want to read.
        let read_max = (self.read_position + buf.len()).min(self.buffer.len());

        // If the position we are reading at is close
        // to the last downloaded chunk, then fetch more.
        let (should_get_chunk, chunk_write_pos) = self.should_get_chunk(buf.len());
        
        log!("Read: read_pos[{}] read_max[{read_max}] buf[{}] write_pos[{chunk_write_pos}] download[{should_get_chunk}]", self.read_position, buf.len());
        if should_get_chunk
        {
            self.requested.insert(chunk_write_pos..chunk_write_pos + CHUNK_SIZE + 1);

            let url = self.url.clone();
            let file_size = self.buffer.len();
            let (tx, rx) = channel();

            let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                .unwrap().as_millis();
            self.receivers.push((id, rx));

            // warning: unused implementer f `Future` that must be used
            // note: futures do nothing unless you `.await` or poll them
            thread::spawn(move || {
                let rt = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(Self::read_chunk(tx, url, chunk_write_pos, file_size));
            });
        }

        // Write any new bytes.
        let should_buffer = !self.downloaded.contains(&self.read_position);
        IS_STREAM_BUFFERING.store(should_buffer, std::sync::atomic::Ordering::SeqCst);
        self.try_write_chunk(should_buffer);

        // These are the bytes that we want to read.
        let bytes = &self.buffer[self.read_position..read_max];
        buf[0..bytes.len()].copy_from_slice(bytes);

        self.read_position += bytes.len();
        Ok(bytes.len())
    }
}

impl Seek for StreamableFile
{
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64>
    {
        let seek_position:usize = match pos
        {
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
            log!("Seek position {seek_position} > file size");
            return Ok(self.read_position as u64);
        }

        log!("Seeking: pos[{seek_position}] type[{pos:?}]");

        self.read_position = seek_position;

        Ok(seek_position as u64)
    }
}

unsafe impl Send for StreamableFile {}
unsafe impl Sync for StreamableFile {}

impl MediaSource for StreamableFile
{
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
    sample_rate: u32,
    channels: usize,
    samples_len: usize,
    error: Option<String>,
}

#[wasm_bindgen]
impl AudioDecoder {
    pub fn new(url: &str) -> AudioDecoder {
        set_panic_hook();

        AudioDecoder {
            url: url.to_string(),
            samples: Vec::new(),
            error: None,
            duration: 0,
            sample_rate: 0,
            channels: 0,
            samples_len: 0,
        }
    }

    pub fn get_sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn get_channels(&self) -> usize {
        self.channels
    }

    pub fn get_duration(&self) -> u64 {
        self.duration
    }

    pub fn get_samples_len(&self) -> usize {
        self.samples_len
    }

    pub fn get_samples(&self) -> *const f32 {
        self.samples.as_ptr()
    }

    pub fn get_last_error(&self) -> Option<String> {
        self.error.clone()
    }

    async fn decode_audio(&mut self) -> Result<Vec<f32>, AudioError> {
        let bytes = fetch_audio_file(&self.url).await.unwrap();
        let stream = MediaSourceStream::new(Box::new(StreamableFile::new(self.url.to_string(), Some(bytes))), Default::default());
        let format_opts: FormatOptions = Default::default();
        let metadata_opts: MetadataOptions = Default::default();
        let decoder_opts: DecoderOptions = Default::default();
        let mut hint = Hint::new();

        let probe = symphonia::default::get_probe().format(&mut hint, stream, &format_opts, &metadata_opts).unwrap();
        let mut format = probe.format;

        let track = format.default_track().unwrap();
        let track_id = track.id;
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts).unwrap();
        let mut sample_count = 0;
        let mut sample_buf = None;
        let mut samples = Vec::new();
        let mut err: Option<AudioError> = None;

        loop {
            // Get the next packet from the format reader.
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(e) => {
                    if e.to_string() == "end of stream" {
                        break;
                    }
                    match e {
                        Error::IoError(e) => {
                            err = Some(AudioError::new(Error::IoError(e), AudioErrorKind::IoError));
                            break;
                        },
                        Error::DecodeError(e) => {
                            err = Some(AudioError::new(Error::DecodeError(e), AudioErrorKind::DecodeError));
                            break;
                        },
                        Error::SeekError(e) => {
                            err = Some(AudioError::new(Error::SeekError(e), AudioErrorKind::SeekError));
                            break;
                        },
                        Error::Unsupported(e) => {
                            err = Some(AudioError::new(Error::Unsupported(e), AudioErrorKind::UnsupportedError));
                            break;
                        },
                        Error::LimitError(e) => {
                            err = Some(AudioError::new(Error::LimitError(e), AudioErrorKind::LimitError));
                            break;
                        },
                        Error::ResetRequired => {
                            err = Some(AudioError::new(Error::from(e), AudioErrorKind::ResetRequiredError));
                            break;
                        },
                        _ => {
                            err = Some(AudioError::new(Error::from(e), AudioErrorKind::UnknownError));
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
                        let duration = audio_buf.capacity() as u64;

                        // Create the f32 sample buffer.
                        sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));

                        self.sample_rate = spec.rate;
                        self.duration = duration;
                        self.channels = spec.channels.count();
                    }

                    // Copy the decoded audio buffer into the sample buffer in an interleaved format.
                    if let Some(buf) = &mut sample_buf {
                        buf.copy_interleaved_ref(audio_buf);

                        // The samples may now be access via the `samples()` function.
                        sample_count += buf.samples().len();
                        samples.extend_from_slice(buf.samples());
                    }
                },
                Err(e) => {
                    if e.to_string() == "end of stream" {
                        break;
                    }
                    match e {
                        Error::IoError(e) => {
                            err = Some(AudioError::new(Error::IoError(e), AudioErrorKind::IoError));
                            break;
                        },
                        Error::DecodeError(e) => {
                            err = Some(AudioError::new(Error::DecodeError(e), AudioErrorKind::DecodeError));
                            break;
                        },
                        Error::SeekError(e) => {
                            err = Some(AudioError::new(Error::SeekError(e), AudioErrorKind::SeekError));
                            break;
                        },
                        Error::Unsupported(e) => {
                            err = Some(AudioError::new(Error::Unsupported(e), AudioErrorKind::UnsupportedError));
                            break;
                        },
                        Error::LimitError(e) => {
                            err = Some(AudioError::new(Error::LimitError(e), AudioErrorKind::LimitError));
                            break;
                        },
                        Error::ResetRequired => {
                            err = Some(AudioError::new(Error::from(e), AudioErrorKind::ResetRequiredError));
                            break;
                        },
                        _ => {
                            err = Some(AudioError::new(Error::from(e), AudioErrorKind::UnknownError));
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

    pub async fn decode(&mut self) -> Result<(), JsValue> {
        let result = self.decode_audio().await;

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
