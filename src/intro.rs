use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use qoi::{Channels, Decoder as QoiDecoder};
use rayon::prelude::*;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};
use thiserror::Error;

use crate::logging::{LogOptions, log_as};

const INTRO_FPS: f64 = 30.0; // plan to make this variable across different intros if needed via a manifest.json impl in the asset folder

pub struct IntroPlayer {
    frames: Vec<IntroFrame>,
    frame_duration: Duration,
    started_at: Option<Instant>,
    pending_audio: Option<Vec<u8>>,
    _audio_sink: Option<MixerDeviceSink>,
    audio_player: Option<Player>,
}

impl IntroPlayer {
    /// loads intro data by folder name
    pub fn load_from_asset(asset_name: &str) -> Result<Self, IntroError> {
        let log = log_as(Some("INTROPLAYER"), LogOptions::default());

        let intro_root = resolve_intro_asset_path(asset_name)?;
        let frames_dir = intro_root.join("frames");
        let audio_path = intro_root.join("audio.wav");

        let started = Instant::now();

        log(&format!("decoding \"{}\"", frames_dir.display()));

        let frames = decode_qoi_frames_from_dir(&frames_dir)?;
        let audio = read_optional_audio(&audio_path)?;

        if frames.is_empty() {
            return Err(IntroError::NoFramesDecoded);
        }

        log(&format!(
            "decoded {} frames of \"{}\" in {:?}",
            frames.len(),
            asset_name,
            started.elapsed()
        ));

        Ok(Self {
            frames,
            frame_duration: Duration::from_secs_f64(1.0 / INTRO_FPS),
            started_at: None,
            pending_audio: audio,
            _audio_sink: None,
            audio_player: None,
        })
    }

    /// start playing the intro audio
    pub fn start(&mut self) -> Result<(), IntroError> {
        let (sink, player) = start_audio(self.pending_audio.take())?;

        self._audio_sink = sink;
        self.audio_player = player;
        self.started_at = Some(Instant::now());

        Ok(())
    }

    pub fn is_started(&self) -> bool {
        self.started_at.is_some()
    }

    /// loads intro data by a specific path
    pub fn load_from_path(path: &Path) -> Result<Self, IntroError> {
        let asset_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| IntroError::InvalidAssetName(path.to_path_buf()))?;

        Self::load_from_asset(asset_name)
    }

    pub fn first_frame(&self) -> Option<&IntroFrame> {
        self.frames.first()
    }

    /// get playback frame for the current playback time
    pub fn frame_to_present(&self) -> Option<(usize, &IntroFrame)> {
        if self.frames.is_empty() {
            return None;
        }

        let frame_index = self
            .current_frame_index()
            .unwrap_or(self.frames.len().saturating_sub(1));

        self.frames
            .get(frame_index)
            .map(|frame| (frame_index, frame))
    }

    pub fn is_finished(&self) -> bool {
        let Some(started_at) = self.started_at else {
            return false;
        };

        let total_duration = self.frame_duration.mul_f64(self.frames.len() as f64);

        started_at.elapsed() >= total_duration
    }

    pub fn stop_audio(&self) {
        if let Some(player) = &self.audio_player {
            player.stop();
        }
    }

    fn current_frame_index(&self) -> Option<usize> {
        let started_at = self.started_at?;

        if self.frames.is_empty() {
            return None;
        }

        let elapsed = started_at.elapsed();
        let frame_index = (elapsed.as_secs_f64() / self.frame_duration.as_secs_f64()) as usize;

        (frame_index < self.frames.len()).then_some(frame_index)
    }
}

#[derive(Debug, Clone)]
pub struct IntroFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Error, Debug)]
pub enum IntroError {
    #[error("{0}")]
    Io(#[from] io::Error),

    #[error("{0}")]
    ImageDecode(String),

    #[error("{0}")]
    AudioPlayback(String),

    #[error("invalid intro asset name from path: {0}")]
    InvalidAssetName(PathBuf),

    #[error("intro asset directory does not exist: {0}")]
    MissingIntroRoot(PathBuf),

    #[error("intro frames directory does not exist: {0}")]
    MissingFramesDir(PathBuf),

    #[error("no intro frames were decoded")]
    NoFramesDecoded,
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

// note(prim): would it be worth it to have a cache of sorts??
//             it would reduce load times down to a simple fs call or two,
//             but idk how it would work rn...
fn decode_qoi_frames_from_dir(frames_dir: &Path) -> Result<Vec<IntroFrame>, IntroError> {
    let log = log_as(Some("INTROPLAYER"), LogOptions::default());

    if !frames_dir.is_dir() {
        return Err(IntroError::MissingFramesDir(frames_dir.to_path_buf()));
    }

    let mut frame_paths = fs::read_dir(frames_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("qoi"))
        })
        .collect::<Vec<_>>();

    frame_paths.sort();

    frame_paths
        .par_iter()
        .map(|path| decode_qoi_frame(path))
        .collect()
}

fn decode_qoi_frame(path: &Path) -> Result<IntroFrame, IntroError> {
    let bytes = fs::read(path)?;

    let mut decoder = QoiDecoder::new(&bytes)
        .map_err(|err| {
            IntroError::ImageDecode(format!(
                "failed to create QOI decoder for {}: {err}",
                path.display()
            ))
        })?
        .with_channels(Channels::Rgba);

    let header = *decoder.header();
    let mut rgba = vec![0; decoder.required_buf_len()];

    decoder.decode_to_buf(&mut rgba).map_err(|err| {
        IntroError::ImageDecode(format!(
            "failed to decode QOI frame {}: {err}",
            path.display()
        ))
    })?;

    let expected_len = header.width as usize * header.height as usize * 4;

    if rgba.len() != expected_len {
        return Err(IntroError::ImageDecode(format!(
            "decoded QOI frame {} produced {} bytes, expected {} bytes for {}x{} RGBA",
            path.display(),
            rgba.len(),
            expected_len,
            header.width,
            header.height
        )));
    }

    Ok(IntroFrame {
        width: header.width,
        height: header.height,
        rgba,
    })
}

fn read_optional_audio(path: &Path) -> Result<Option<Vec<u8>>, IntroError> {
    if !path.exists() {
        return Ok(None);
    }

    Ok(Some(fs::read(path)?))
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
