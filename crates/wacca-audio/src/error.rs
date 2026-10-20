use std::path::PathBuf;

/// Everything that can go wrong in this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{what}: expected magic {expected:?} at 0x{offset:x}")]
    BadMagic {
        what: &'static str,
        expected: &'static str,
        offset: usize,
    },
    #[error("{what}: data truncated (need {need} bytes at 0x{offset:x}, have {have})")]
    Truncated {
        what: &'static str,
        offset: usize,
        need: usize,
        have: usize,
    },
    #[error("@UTF: unsupported column flags 0x{0:02x}")]
    UnsupportedColumn(u8),
    #[error("AFS2: unsupported field size {0}")]
    UnsupportedFieldSize(u16),
    #[error("@UTF table {table}: missing column {column}")]
    MissingColumn { table: String, column: String },
    #[error("@UTF table {table}: column {column} row {row} has an unexpected type")]
    ColumnType {
        table: String,
        column: String,
        row: usize,
    },
    #[error("@UTF table {table}: row {row} out of range")]
    RowOutOfRange { table: String, row: usize },
    #[error("cue {0:?} not found in the ACB")]
    CueNotFound(String),
    #[error("cue {0:?} has no playable waveform")]
    CueSilent(String),
    #[error("cue {cue:?}: waveform is not an HCA in a stream AWB (sound effects: use SeBank)")]
    UnsupportedSource { cue: String },
    #[error("waveform {index}: {reason} not supported")]
    UnsupportedWaveform { index: u16, reason: &'static str },
    #[error("ADX decode: {0}")]
    Adx(String),
    #[error(
        "sound effect is {got} Hz but the output runs at {expected} Hz (resample it first: Sfx::resampled)"
    )]
    SampleRate { expected: u32, got: u32 },
    #[error("sound-effect queue is full")]
    SfxQueueFull,
    #[error("stream AWB port {0} not listed in the ACB")]
    UnknownAwbPort(u16),
    #[error("AWB {awb}: id {id} not found")]
    AwbIdNotFound { awb: String, id: u32 },
    #[error("HCA decode: {0}")]
    Hca(String),
    #[error("audio device: {0}")]
    Device(String),
    #[error("{path}: {source}")]
    File {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub(crate) fn file(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::File { path, source }
    }
}
