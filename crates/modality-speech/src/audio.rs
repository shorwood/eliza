//! Encoders for the speech formats exposed by provider adapters.

use audio_codec_algorithms::{encode_alaw, encode_ulaw};
use rusty_mp3::{Mp3Encoder as CodecMp3Encoder, Mp3EncoderConfig};

use super::core::AudioFormat;
use super::errors::Error;

// -----------------------------------------------------------------------------
// Mp3Encoder: Owns complete and incremental MPEG Layer III encoding.
// -----------------------------------------------------------------------------

/// Owns the state and emitted-frame invariant of one MP3 stream.
struct Mp3Encoder {
    /// Boxed codec state keeps [`StreamEncoding`] compact.
    codec: Box<CodecMp3Encoder>,
    /// Whether any audio frame has reached the transport.
    has_emitted: bool,
}

impl Mp3Encoder {
    /// Start an empty incremental encoder.
    fn new() -> Self {
        Self {
            codec: Box::new(Self::new_codec()),
            has_emitted: false,
        }
    }

    /// Encode complete signed mono PCM as MPEG Layer III audio.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when the codec rejects PCM or emits no frame.
    fn encode(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, Error> {
        let mut codec = Self::new_codec();
        codec
            .push_pcm_s16(samples, 1, sample_rate)
            .map_err(|source| Error::Mp3Encoding {
                detail: source.to_string(),
            })?;
        codec.finish();
        let bytes = Self::drain(&mut codec);
        (!bytes.is_empty())
            .then_some(bytes)
            .ok_or(Error::Mp3NoFrames)
    }

    /// Configure the underlying fixed-bitrate codec.
    fn new_codec() -> CodecMp3Encoder {
        CodecMp3Encoder::new(Mp3EncoderConfig {
            bitrate_kbps: 64,
            vbr_quality: None,
        })
    }

    /// Drain available frames without treating `Again` as an error.
    fn drain(codec: &mut CodecMp3Encoder) -> Vec<u8> {
        let mut bytes = Vec::new();
        while let Ok(packet) = codec.next_packet() {
            bytes.extend_from_slice(&packet);
        }
        bytes
    }

    /// Push PCM and drain every frame currently available.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when the codec rejects the PCM chunk.
    fn push(&mut self, samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, Error> {
        self.codec
            .push_pcm_s16(samples, 1, sample_rate)
            .map_err(|source| Error::Mp3Encoding {
                detail: source.to_string(),
            })?;
        let bytes = Self::drain(&mut self.codec);
        self.has_emitted |= !bytes.is_empty();
        Ok(bytes)
    }

    /// Flush the stream without appending its late seek-info frame.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when the codec emits no audio frame.
    fn finish(mut self) -> Result<Vec<u8>, Error> {
        self.codec.finish();
        self.codec
            .next_packet()
            .map_err(|source| Error::Mp3Encoding {
                detail: source.to_string(),
            })?;
        let bytes = Self::drain(&mut self.codec);
        self.has_emitted |= !bytes.is_empty();
        self.has_emitted.then_some(bytes).ok_or(Error::Mp3NoFrames)
    }
}

// -----------------------------------------------------------------------------
// WavEncoder: Builds complete and streaming RIFF/WAVE containers.
// -----------------------------------------------------------------------------

/// Serialize signed mono samples as headerless little-endian PCM.
fn encode_pcm(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

/// Encoder for mono 16-bit PCM in a RIFF/WAVE container.
struct WavEncoder {
    /// Samples per second recorded in the container header.
    sample_rate: u32,
}

impl WavEncoder {
    /// Bytes in a mono PCM RIFF/WAVE header.
    const HEADER_BYTES: usize = 44;

    /// Configure an encoder for one fixed sample rate.
    const fn new(sample_rate: u32) -> Self {
        Self { sample_rate }
    }

    /// Build a mono PCM RIFF/WAVE header for a known data length.
    fn header(&self, data_len: u32) -> Vec<u8> {
        let format = 1_u16;
        let channels = 1_u16;
        let mut wav = Vec::with_capacity(Self::HEADER_BYTES);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&data_len.saturating_add(36).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&format.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&self.sample_rate.to_le_bytes());
        wav.extend_from_slice(&(self.sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav
    }

    /// Wrap signed mono PCM in a complete RIFF/WAVE container.
    fn encode(&self, samples: &[i16]) -> Vec<u8> {
        let data = encode_pcm(samples);
        let data_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let mut wav = self.header(data_len);
        wav.extend_from_slice(&data);
        wav
    }

    /// Build a header whose unknown length permits progressive delivery.
    fn streaming_header(&self) -> Vec<u8> {
        self.header(u32::MAX)
    }
}

/// Serialize signed mono samples as headerless big-endian RFC L16.
fn encode_l16(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_be_bytes())
        .collect()
}

/// Encode PCM samples through one G.711 companding function.
fn encode_g711(samples: &[i16], encode: impl Fn(i16) -> u8) -> Vec<u8> {
    let copied = samples.iter().copied();
    copied.map(encode).collect()
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
    ) -> Result<Self, Error> {
        let bytes = match format {
            AudioFormat::Mp3 => Mp3Encoder::encode(samples, sample_rate)?,
            AudioFormat::Wav => WavEncoder::new(sample_rate).encode(samples),
            AudioFormat::Pcm => encode_pcm(samples),
            AudioFormat::L16 => encode_l16(samples),
            AudioFormat::MuLaw => encode_g711(samples, encode_ulaw),
            AudioFormat::ALaw => encode_g711(samples, encode_alaw),
        };
        Ok(Self {
            bytes,
            media_type: format.media_type(sample_rate),
        })
    }
}

// -----------------------------------------------------------------------------
// StreamEncoding: Retains codec-specific incremental state.
// -----------------------------------------------------------------------------

/// Incremental encoder state for one response stream.
enum StreamEncoding {
    /// MPEG Layer III packet encoder and whether it has emitted audio.
    Mp3(
        /// Codec state for this stream.
        Mp3Encoder,
    ),
    /// RIFF/WAVE whose header was emitted before its PCM.
    Wav(
        /// Container configuration for this stream.
        WavEncoder,
    ),
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
            AudioFormat::Mp3 => StreamEncoding::Mp3(Mp3Encoder::new()),
            AudioFormat::Wav => StreamEncoding::Wav(WavEncoder::new(sample_rate)),
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
            StreamEncoding::Mp3(_) => AudioFormat::Mp3,
            StreamEncoding::Wav(_) => AudioFormat::Wav,
            StreamEncoding::Pcm => AudioFormat::Pcm,
            StreamEncoding::L16 => AudioFormat::L16,
            StreamEncoding::MuLaw => AudioFormat::MuLaw,
            StreamEncoding::ALaw => AudioFormat::ALaw,
        };
        format.media_type(self.sample_rate)
    }

    /// Emit any format prefix required before the first PCM chunk.
    pub(super) fn begin(&self) -> Option<Vec<u8>> {
        match &self.encoding {
            StreamEncoding::Wav(encoder) => Some(encoder.streaming_header()),
            _ => None,
        }
    }

    /// Encode one PCM chunk without waiting for the rest of the utterance.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when the MP3 encoder rejects input.
    pub(super) fn push(&mut self, samples: &[i16]) -> Result<Vec<u8>, Error> {
        match &mut self.encoding {
            StreamEncoding::Mp3(encoder) => encoder.push(samples, self.sample_rate),
            StreamEncoding::Wav(_) | StreamEncoding::Pcm => Ok(encode_pcm(samples)),
            StreamEncoding::L16 => Ok(encode_l16(samples)),
            StreamEncoding::MuLaw => Ok(encode_g711(samples, encode_ulaw)),
            StreamEncoding::ALaw => Ok(encode_g711(samples, encode_alaw)),
        }
    }

    /// Flush codec tail bytes after the final PCM chunk.
    ///
    /// # Errors
    ///
    /// Returns an encoding failure when MP3 produced no audio frames.
    pub(super) fn finish(self) -> Result<Vec<u8>, Error> {
        match self.encoding {
            StreamEncoding::Mp3(encoder) => encoder.finish(),
            StreamEncoding::Wav(_)
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
        let wav = WavEncoder::new(24_000).encode(&[0, 1, -1]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(wav.len(), 50);
    }

    #[test]
    fn streamed_wav_has_open_ended_lengths() {
        let header = StreamingEncoder::new(AudioFormat::Wav, 24_000)
            .begin()
            .unwrap();
        assert_eq!(header.len(), WavEncoder::HEADER_BYTES);
        assert_eq!(
            u32::from_le_bytes(header[4..8].try_into().unwrap()),
            u32::MAX
        );
        assert_eq!(
            u32::from_le_bytes(header[40..44].try_into().unwrap()),
            u32::MAX
        );
    }

    #[test]
    fn l16_uses_network_byte_order() {
        assert_eq!(encode_l16(&[0x1234, -2]), [0x12, 0x34, 0xff, 0xfe]);
    }

    #[test]
    fn formats_report_provider_media_types() {
        assert_eq!(AudioFormat::Mp3.media_type(24_000), "audio/mpeg");
        assert_eq!(AudioFormat::Wav.media_type(24_000), "audio/wav");
        assert_eq!(AudioFormat::Pcm.media_type(24_000), "audio/pcm;rate=24000");
        assert_eq!(AudioFormat::L16.media_type(24_000), "audio/L16;rate=24000");
        assert_eq!(
            AudioFormat::MuLaw.media_type(8_000),
            "audio/mulaw;rate=8000"
        );
        assert_eq!(AudioFormat::ALaw.media_type(8_000), "audio/alaw;rate=8000");
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
