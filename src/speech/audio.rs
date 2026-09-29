//! Tiny encoders for the speech formats exposed by provider adapters.

use audio_codec_algorithms::{encode_alaw, encode_ulaw};
use rusty_mp3::{Mp3Encoder, Mp3EncoderConfig};

use super::core::AudioFormat;
use super::errors::SpeechError;

// -----------------------------------------------------------------------------
// Audio: Encodes buffered PCM and derives its provider metadata.
// -----------------------------------------------------------------------------

/// Bytes in a mono PCM RIFF/WAVE header.
const AUDIO_WAV_HEADER_BYTES: usize = 44;

/// Serialize signed mono samples as headerless little-endian PCM.
fn audio_encode_pcm(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

/// Serialize signed mono samples as headerless big-endian RFC L16.
fn audio_encode_l16(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_be_bytes())
        .collect()
}

/// Build a mono PCM RIFF/WAVE header for a known or streaming data length.
fn audio_wav_header(sample_rate: u32, data_len: Option<u32>) -> Vec<u8> {
    let data_len = data_len.unwrap_or(u32::MAX);
    let format = 1_u16;
    let channels = 1_u16;
    let mut wav = Vec::with_capacity(AUDIO_WAV_HEADER_BYTES);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&data_len.saturating_add(36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&format.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav
}

/// Wrap signed mono PCM in a RIFF/WAVE container.
fn audio_encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data = audio_encode_pcm(samples);
    let data_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let mut wav = audio_wav_header(sample_rate, Some(data_len));
    wav.extend_from_slice(&data);
    wav
}

/// Encode signed mono PCM as MPEG Layer III audio.
///
/// # Errors
///
/// Returns an encoding failure when the codec rejects PCM or emits no frame.
fn audio_encode_mp3(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, SpeechError> {
    let mut encoder = Mp3Encoder::new(Mp3EncoderConfig {
        bitrate_kbps: 64,
        vbr_quality: None,
    });
    encoder
        .push_pcm_s16(samples, 1, sample_rate)
        .map_err(|source| SpeechError::Mp3Encoding { source })?;
    encoder.finish();
    let mut bytes = Vec::new();
    while let Ok(packet) = encoder.next_packet() {
        bytes.extend_from_slice(&packet);
    }
    (!bytes.is_empty())
        .then_some(bytes)
        .ok_or(SpeechError::Mp3NoFrames)
}

/// Return the provider-facing MIME type for one encoding and sample rate.
fn audio_media_type(format: AudioFormat, sample_rate: u32) -> String {
    match format {
        AudioFormat::Mp3 => "audio/mpeg".to_owned(),
        AudioFormat::Wav => "audio/wav".to_owned(),
        AudioFormat::Pcm => format!("audio/pcm;rate={sample_rate}"),
        AudioFormat::L16 => format!("audio/L16;rate={sample_rate}"),
        AudioFormat::MuLaw => format!("audio/mulaw;rate={sample_rate}"),
        AudioFormat::ALaw => format!("audio/alaw;rate={sample_rate}"),
    }
}

// -----------------------------------------------------------------------------
// EncodedAudio: Pairs encoded bytes with provider response metadata.
// -----------------------------------------------------------------------------

/// Encoded bytes paired with their provider-safe MIME type.
pub(super) struct EncodedAudio {
    /// Complete encoded payload.
    pub(super) bytes: Vec<u8>,
    /// MIME type including the sample rate where relevant.
    pub(super) media_type: String,
}

impl EncodedAudio {
    /// Encode signed mono samples into a requested container or companding format.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when the selected codec cannot produce output.
    pub(super) fn encode(
        samples: &[i16],
        sample_rate: u32,
        format: AudioFormat,
    ) -> Result<Self, SpeechError> {
        let encoded = match format {
            AudioFormat::Mp3 => Self {
                bytes: audio_encode_mp3(samples, sample_rate)?,
                media_type: audio_media_type(format, sample_rate),
            },
            AudioFormat::Wav => Self {
                bytes: audio_encode_wav(samples, sample_rate),
                media_type: audio_media_type(format, sample_rate),
            },
            AudioFormat::Pcm => Self {
                bytes: audio_encode_pcm(samples),
                media_type: audio_media_type(format, sample_rate),
            },
            AudioFormat::L16 => Self {
                bytes: audio_encode_l16(samples),
                media_type: audio_media_type(format, sample_rate),
            },
            AudioFormat::MuLaw => Self {
                bytes: samples.iter().copied().map(encode_ulaw).collect(),
                media_type: audio_media_type(format, sample_rate),
            },
            AudioFormat::ALaw => Self {
                bytes: samples.iter().copied().map(encode_alaw).collect(),
                media_type: audio_media_type(format, sample_rate),
            },
        };
        Ok(encoded)
    }
}

// -----------------------------------------------------------------------------
// StreamEncoding: Retains codec-specific incremental state.
// -----------------------------------------------------------------------------

/// Incremental encoder state for one response stream.
enum StreamEncoding {
    /// MPEG Layer III packet encoder and whether it has emitted audio.
    Mp3 {
        /// Boxed state keeps the format enum compact.
        encoder: Box<Mp3Encoder>,
        /// Whether any frame has reached the transport.
        emitted: bool,
    },
    /// RIFF/WAVE whose header was emitted before its PCM.
    Wav,
    /// Headerless little-endian PCM.
    Pcm,
    /// Headerless big-endian L16 PCM.
    L16,
    /// G.711 mu-law.
    MuLaw,
    /// G.711 A-law.
    ALaw,
}

// -----------------------------------------------------------------------------
// Mp3: Feeds and drains the packet encoder incrementally.
// -----------------------------------------------------------------------------

/// Drain every MP3 frame currently available without treating `Again` as an error.
fn mp3_drain(encoder: &mut Mp3Encoder) -> Vec<u8> {
    let mut bytes = Vec::new();
    while let Ok(packet) = encoder.next_packet() {
        bytes.extend_from_slice(&packet);
    }
    bytes
}

/// Push PCM through an incremental MP3 encoder and drain available frames.
///
/// # Errors
///
/// Returns an encoding failure when the codec rejects the PCM chunk.
fn mp3_push(
    encoder: &mut Mp3Encoder,
    emitted: &mut bool,
    samples: &[i16],
    sample_rate: u32,
) -> Result<Vec<u8>, SpeechError> {
    encoder
        .push_pcm_s16(samples, 1, sample_rate)
        .map_err(|source| SpeechError::Mp3Encoding { source })?;
    let bytes = mp3_drain(encoder);
    *emitted |= !bytes.is_empty();
    Ok(bytes)
}

/// Flush a live MP3 encoder without appending its late seek-info frame.
///
/// # Errors
///
/// Returns an encoding failure when the codec emits no audio frame.
fn mp3_finish(mut encoder: Mp3Encoder, mut emitted: bool) -> Result<Vec<u8>, SpeechError> {
    encoder.finish();
    encoder
        .next_packet()
        .map_err(|source| SpeechError::Mp3Encoding { source })?;
    let bytes = mp3_drain(&mut encoder);
    emitted |= !bytes.is_empty();
    emitted.then_some(bytes).ok_or(SpeechError::Mp3NoFrames)
}

// -----------------------------------------------------------------------------
// StreamingEncoder: Releases encoded bytes while PCM is still arriving.
// -----------------------------------------------------------------------------

/// Stateful encoder that releases bytes as PCM becomes available.
pub(super) struct StreamingEncoder {
    /// Selected codec state.
    encoding: StreamEncoding,
    /// Input sample rate fixed for the stream lifetime.
    sample_rate: u32,
}

impl StreamingEncoder {
    /// Create an encoder for one fixed format and sample rate.
    pub(super) fn new(format: AudioFormat, sample_rate: u32) -> Self {
        let encoding = match format {
            AudioFormat::Mp3 => StreamEncoding::Mp3 {
                encoder: Box::new(Mp3Encoder::new(Mp3EncoderConfig {
                    bitrate_kbps: 64,
                    vbr_quality: None,
                })),
                emitted: false,
            },
            AudioFormat::Wav => StreamEncoding::Wav,
            AudioFormat::Pcm => StreamEncoding::Pcm,
            AudioFormat::L16 => StreamEncoding::L16,
            AudioFormat::MuLaw => StreamEncoding::MuLaw,
            AudioFormat::ALaw => StreamEncoding::ALaw,
        };
        Self {
            encoding,
            sample_rate,
        }
    }

    /// Return the provider-facing MIME type for this encoder.
    pub(super) fn media_type(&self) -> String {
        let format = match self.encoding {
            StreamEncoding::Mp3 { .. } => AudioFormat::Mp3,
            StreamEncoding::Wav => AudioFormat::Wav,
            StreamEncoding::Pcm => AudioFormat::Pcm,
            StreamEncoding::L16 => AudioFormat::L16,
            StreamEncoding::MuLaw => AudioFormat::MuLaw,
            StreamEncoding::ALaw => AudioFormat::ALaw,
        };
        audio_media_type(format, self.sample_rate)
    }

    /// Emit any format prefix required before the first PCM chunk.
    pub(super) fn begin(&self) -> Option<Vec<u8>> {
        matches!(self.encoding, StreamEncoding::Wav)
            .then(|| audio_wav_header(self.sample_rate, None))
    }

    /// Encode one PCM chunk without waiting for the rest of the utterance.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when the MP3 encoder rejects input.
    pub(super) fn push(&mut self, samples: &[i16]) -> Result<Vec<u8>, SpeechError> {
        match &mut self.encoding {
            StreamEncoding::Mp3 { encoder, emitted } => {
                mp3_push(encoder, emitted, samples, self.sample_rate)
            }
            StreamEncoding::Wav | StreamEncoding::Pcm => Ok(audio_encode_pcm(samples)),
            StreamEncoding::L16 => Ok(audio_encode_l16(samples)),
            StreamEncoding::MuLaw => Ok(samples.iter().copied().map(encode_ulaw).collect()),
            StreamEncoding::ALaw => Ok(samples.iter().copied().map(encode_alaw).collect()),
        }
    }

    /// Flush codec tail bytes after the final PCM chunk.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when MP3 produced no audio frames.
    pub(super) fn finish(self) -> Result<Vec<u8>, SpeechError> {
        match self.encoding {
            StreamEncoding::Mp3 { encoder, emitted } => mp3_finish(*encoder, emitted),
            StreamEncoding::Wav
            | StreamEncoding::Pcm
            | StreamEncoding::L16
            | StreamEncoding::MuLaw
            | StreamEncoding::ALaw => Ok(Vec::new()),
        }
    }
}

// -----------------------------------------------------------------------------
// Tests: Locks down container and codec invariants.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![expect(
        clippy::missing_panics_doc,
        reason = "asserting codec tests intentionally panic on regressions"
    )]
    use super::*;

    #[test]
    fn wav_has_consistent_header() {
        let wav = audio_encode_wav(&[0, 1, -1], 24_000);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(wav.len(), 50);
    }

    #[test]
    fn l16_uses_network_byte_order() {
        assert_eq!(audio_encode_l16(&[0x1234, -2]), [0x12, 0x34, 0xff, 0xfe]);
    }

    #[test]
    fn g711_matches_standard_vectors() {
        let samples = [0, -4, 1_000, -1_000, i16::MAX, i16::MIN];
        assert_eq!(
            samples.map(encode_ulaw),
            [0xff, 0x7f, 0xce, 0x4e, 0x80, 0x00]
        );
        assert_eq!(
            samples.map(encode_alaw),
            [0xd5, 0x55, 0xfa, 0x7a, 0xaa, 0x2a]
        );
    }

    #[test]
    fn streamed_mp3_decodes_to_non_silent_mono_audio() {
        let samples = (0..4_800)
            .map(|index| if index % 32 < 16 { 12_000 } else { -12_000 })
            .collect::<Vec<_>>();
        let mut encoder = StreamingEncoder::new(AudioFormat::Mp3, 24_000);
        let mut mp3_bytes = Vec::new();
        for chunk in samples.chunks(257) {
            mp3_bytes.extend(encoder.push(chunk).unwrap());
        }
        mp3_bytes.extend(encoder.finish().unwrap());

        let mut decoder = rusty_mp3::Mp3Decoder::new();
        decoder.push(&mp3_bytes);
        decoder.flush();
        let mut pcm_samples = Vec::new();
        while let Ok(frame) = decoder.next_frame() {
            assert_eq!(frame.sample_rate, 24_000);
            assert_eq!(frame.channels, 1);
            pcm_samples.extend(frame.samples);
        }
        assert!(!pcm_samples.is_empty());
        assert!(pcm_samples.iter().any(|sample| sample.abs() > 0.01));
    }
}
