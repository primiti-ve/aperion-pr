use std::collections::VecDeque;
use std::fs;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use thiserror::Error;

use crate::logging::{LogOptions, log_as};

const INTRO_VIDEO_FILENAME: &str = "intro.mp4";
const FRAME_BUFFER_CAPACITY: usize = 4;

pub struct IntroPlayer {
    width: u32,
    height: u32,
    frame_duration: Duration,
    total_duration: Duration,
    started_at: Option<Instant>,
    pending_audio: Option<Vec<u8>>,
    _audio_sink: Option<MixerDeviceSink>,
    audio_player: Option<Player>,
    current_frame: Option<DecodedFrame>,
    pending_frames: VecDeque<DecodedFrame>,
    decode_events: Receiver<DecodeEvent>,
    decode_finished: bool,
}

// eventually, we need to either bundle ffmpeg with the build via the build script or find some other way to decode these videos...
// oh how I envy Bink video right now
impl IntroPlayer {
    pub fn load_from_asset(asset_name: &str) -> Result<Self, IntroError> {
        let intro_root = resolve_intro_asset_path(asset_name)?;
        Self::load_from_path(&intro_root)
    }

    pub fn load_from_path(path: &Path) -> Result<Self, IntroError> {
        let log = log_as(Some("INTROPLAYER"), LogOptions::default());
        let video_path = resolve_intro_video_path(path)?;

        let started = Instant::now();
        log(&format!("probing \"{}\"", video_path.display()));

        let metadata = probe_intro_video(&video_path)?;
        let pending_audio = extract_optional_audio(&video_path, metadata.has_audio)?;
        let (decode_events, first_frame) = start_video_decoder(
            &video_path,
            metadata.width,
            metadata.height,
            FRAME_BUFFER_CAPACITY,
        )?;

        log(&format!(
            "prepared {}x{} intro at {:.3} fps in {:?}",
            metadata.width,
            metadata.height,
            1.0 / metadata.frame_duration.as_secs_f64(),
            started.elapsed()
        ));

        Ok(Self {
            width: metadata.width,
            height: metadata.height,
            frame_duration: metadata.frame_duration,
            total_duration: metadata.duration,
            started_at: None,
            pending_audio,
            _audio_sink: None,
            audio_player: None,
            current_frame: Some(first_frame),
            pending_frames: VecDeque::with_capacity(FRAME_BUFFER_CAPACITY),
            decode_events,
            decode_finished: false,
        })
    }

    pub fn start(&mut self) -> Result<(), IntroError> {
        if self.started_at.is_some() {
            return Ok(());
        }

        let (sink, player) = start_audio(self.pending_audio.take())?;

        self._audio_sink = sink;
        self.audio_player = player;
        self.started_at = Some(Instant::now());

        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn first_frame(&self) -> Option<&IntroFrame> {
        self.current_frame.as_ref().map(|frame| &frame.frame)
    }

    pub fn frame_to_present(&mut self) -> Result<Option<(usize, &IntroFrame)>, IntroError> {
        self.drain_decode_events()?;

        let frame_index = self.current_frame_index();

        while let Some(next_frame) = self.pending_frames.front() {
            if next_frame.index > frame_index {
                break;
            }

            self.current_frame = self.pending_frames.pop_front();
        }

        Ok(self
            .current_frame
            .as_ref()
            .map(|frame| (frame.index, &frame.frame)))
    }

    pub fn is_finished(&self) -> bool {
        let Some(started_at) = self.started_at else {
            return false;
        };

        started_at.elapsed() >= self.total_duration
    }

    pub fn stop_audio(&self) {
        if let Some(player) = &self.audio_player {
            player.stop();
        }
    }

    fn current_frame_index(&self) -> usize {
        let Some(started_at) = self.started_at else {
            return 0;
        };

        let elapsed = started_at.elapsed().min(self.total_duration);
        (elapsed.as_secs_f64() / self.frame_duration.as_secs_f64()) as usize
    }

    fn drain_decode_events(&mut self) -> Result<(), IntroError> {
        loop {
            match self.decode_events.try_recv() {
                Ok(DecodeEvent::Frame(frame)) => self.pending_frames.push_back(frame),
                Ok(DecodeEvent::Finished) => self.decode_finished = true,
                Ok(DecodeEvent::Error(message)) => {
                    return Err(IntroError::VideoDecode(message));
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return if self.decode_finished {
                        Ok(())
                    } else {
                        Err(IntroError::VideoDecode(
                            "video decoder disconnected unexpectedly".to_string(),
                        ))
                    };
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct IntroFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone)]
struct DecodedFrame {
    index: usize,
    frame: IntroFrame,
}

enum DecodeEvent {
    Frame(DecodedFrame),
    Finished,
    Error(String),
}

struct IntroVideoMetadata {
    width: u32,
    height: u32,
    duration: Duration,
    frame_duration: Duration,
    has_audio: bool,
}

#[derive(Error, Debug)]
pub enum IntroError {
    #[error("{0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    AudioPlayback(String),

    #[error("invalid intro asset name from path: {0}")]
    InvalidAssetName(PathBuf),

    #[error("intro asset directory does not exist: {0}")]
    MissingIntroRoot(PathBuf),

    #[error("intro video file does not exist: {0}")]
    MissingVideoFile(PathBuf),

    #[error("{0}")]
    VideoProbe(String),

    #[error("{0}")]
    VideoDecode(String),

    #[error("ffmpeg returned no frames for {0}")]
    NoFramesDecoded(PathBuf),
}

fn resolve_intro_asset_path(asset_name: &str) -> Result<PathBuf, IntroError> {
    let relative = Path::new("assets").join("intros").join(asset_name);

    let from_cwd = std::env::current_dir()?.join(&relative);
    if from_cwd.exists() {
        return Ok(from_cwd);
    }

    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "could not find exe directory"))?
        .to_path_buf();

    let from_exe = exe_dir.join(&relative);
    if from_exe.exists() {
        return Ok(from_exe);
    }

    Err(IntroError::MissingIntroRoot(from_cwd))
}

fn resolve_intro_video_path(path: &Path) -> Result<PathBuf, IntroError> {
    let intro_root = if path.is_dir() {
        path.to_path_buf()
    } else {
        let asset_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| IntroError::InvalidAssetName(path.to_path_buf()))?;

        resolve_intro_asset_path(asset_name)?
    };

    let video_path = intro_root.join(INTRO_VIDEO_FILENAME);
    if video_path.is_file() {
        Ok(video_path)
    } else {
        Err(IntroError::MissingVideoFile(video_path))
    }
}

fn probe_intro_video(path: &Path) -> Result<IntroVideoMetadata, IntroError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,avg_frame_rate",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .output()
        .map_err(|err| {
            IntroError::VideoProbe(format!(
                "failed to run ffprobe for {}: {err}",
                path.display()
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IntroError::VideoProbe(format!(
            "ffprobe failed for {}: {}",
            path.display(),
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut width = None;
    let mut height = None;
    let mut avg_frame_rate = None;
    let mut duration = None;

    for line in stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };

        match key.trim() {
            "width" => width = value.trim().parse::<u32>().ok(),
            "height" => height = value.trim().parse::<u32>().ok(),
            "avg_frame_rate" => avg_frame_rate = parse_frame_rate(value.trim()),
            "duration" => duration = value.trim().parse::<f64>().ok(),
            _ => {}
        }
    }

    let width = width.ok_or_else(|| {
        IntroError::VideoProbe(format!("ffprobe did not report width for {}", path.display()))
    })?;
    let height = height.ok_or_else(|| {
        IntroError::VideoProbe(format!("ffprobe did not report height for {}", path.display()))
    })?;
    let frame_rate = avg_frame_rate.ok_or_else(|| {
        IntroError::VideoProbe(format!(
            "ffprobe did not report a valid frame rate for {}",
            path.display()
        ))
    })?;
    let duration = duration.ok_or_else(|| {
        IntroError::VideoProbe(format!(
            "ffprobe did not report duration for {}",
            path.display()
        ))
    })?;

    let has_audio = probe_audio_stream(path)?;

    Ok(IntroVideoMetadata {
        width,
        height,
        duration: Duration::from_secs_f64(duration),
        frame_duration: Duration::from_secs_f64(1.0 / frame_rate),
        has_audio,
    })
}

fn parse_frame_rate(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;

    (denominator > 0.0).then_some(numerator / denominator)
}

fn probe_audio_stream(path: &Path) -> Result<bool, IntroError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .map_err(|err| {
            IntroError::VideoProbe(format!(
                "failed to inspect audio stream for {}: {err}",
                path.display()
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IntroError::VideoProbe(format!(
            "ffprobe audio probe failed for {}: {}",
            path.display(),
            stderr.trim()
        )));
    }

    Ok(!output.stdout.is_empty())
}

fn extract_optional_audio(path: &Path, has_audio: bool) -> Result<Option<Vec<u8>>, IntroError> {
    if !has_audio {
        return Ok(None);
    }

    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "a:0", "-vn", "-acodec", "pcm_s16le", "-f", "wav", "pipe:1"])
        .output()
        .map_err(|err| {
            IntroError::VideoDecode(format!(
                "failed to extract audio from {}: {err}",
                path.display()
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IntroError::VideoDecode(format!(
            "ffmpeg audio extraction failed for {}: {}",
            path.display(),
            stderr.trim()
        )));
    }

    if output.stdout.is_empty() {
        Ok(None)
    } else {
        Ok(Some(output.stdout))
    }
}

fn start_video_decoder(
    path: &Path,
    width: u32,
    height: u32,
    buffer_capacity: usize,
) -> Result<(Receiver<DecodeEvent>, DecodedFrame), IntroError> {
    let frame_size = width as usize * height as usize * 4;
    let path_buf = path.to_path_buf();
    let (sender, receiver) = mpsc::sync_channel(buffer_capacity);

    thread::spawn(move || {
        let mut child = match Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&path_buf)
            .args(["-an", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                let _ = sender.send(DecodeEvent::Error(format!(
                    "failed to start ffmpeg decoder for {}: {err}",
                    path_buf.display()
                )));
                return;
            }
        };

        let Some(mut stdout) = child.stdout.take() else {
            let _ = sender.send(DecodeEvent::Error(format!(
                "ffmpeg did not provide a stdout pipe for {}",
                path_buf.display()
            )));
            let _ = child.kill();
            let _ = child.wait();
            return;
        };

        let mut frame_index = 0usize;

        loop {
            let mut rgba = vec![0; frame_size];

            match stdout.read_exact(&mut rgba) {
                Ok(()) => {
                    let event = DecodeEvent::Frame(DecodedFrame {
                        index: frame_index,
                        frame: IntroFrame {
                            width,
                            height,
                            rgba,
                        },
                    });

                    if sender.send(event).is_err() {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }

                    frame_index += 1;
                }
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                    if frame_index == 0 {
                        let _ = sender.send(DecodeEvent::Error(format!(
                            "ffmpeg produced no frames for {}",
                            path_buf.display()
                        )));
                    } else {
                        let _ = sender.send(DecodeEvent::Finished);
                    }

                    let _ = child.wait();
                    return;
                }
                Err(err) => {
                    let _ = sender.send(DecodeEvent::Error(format!(
                        "failed while decoding video frames from {}: {err}",
                        path_buf.display()
                    )));
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
    });

    match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(DecodeEvent::Frame(frame)) => Ok((receiver, frame)),
        Ok(DecodeEvent::Error(message)) => Err(IntroError::VideoDecode(message)),
        Ok(DecodeEvent::Finished) => Err(IntroError::NoFramesDecoded(path.to_path_buf())),
        Err(err) => Err(IntroError::VideoDecode(format!(
            "timed out waiting for the first decoded frame from {}: {err}",
            path.display()
        ))),
    }
}

fn start_audio(
    audio: Option<Vec<u8>>,
) -> Result<(Option<MixerDeviceSink>, Option<Player>), IntroError> {
    let Some(audio) = audio else {
        return Ok((None, None));
    };

    let sink_handle = DeviceSinkBuilder::open_default_sink().map_err(|err| {
        IntroError::AudioPlayback(format!("could not open default audio output: {err}"))
    })?;

    let player = Player::connect_new(&sink_handle.mixer());

    let decoder = Decoder::try_from(Cursor::new(audio))
        .map_err(|err| IntroError::AudioPlayback(format!("could not decode intro audio: {err}")))?;

    player.append(decoder);
    player.play();

    Ok((Some(sink_handle), Some(player)))
}
