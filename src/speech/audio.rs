//! Tiny encoders for the speech formats exposed by provider adapters.

use audio_codec_algorithms::{encode_alaw, encode_ulaw};
use rusty_mp3::{Mp3Encoder, Mp3EncoderConfig};

use super::core::{AudioFormat, SpeechError};

// -----------------------------------------------------------------------------
// Encode: Produces the supported audio containers and byte encodings.
// -----------------------------------------------------------------------------

/// Serialize signed mono samples as headerless little-endian PCM.
fn encode_pcm(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect()
}

/// Serialize signed mono samples as headerless big-endian RFC L16.
fn encode_l16(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|sample| sample.to_be_bytes())
        .collect()
}

/// Wrap signed mono PCM in a RIFF/WAVE container.
fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data = encode_pcm(samples);
    let data_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let format = 1_u16;
    let channels = 1_u16;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
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
    wav.extend_from_slice(&data);
    wav
}

/// Encode signed mono PCM as MPEG Layer III audio.
///
/// # Errors
///
/// Returns an encoding failure when the codec rejects PCM or emits no frame.
fn encode_mp3(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, SpeechError> {
    let mut encoder = Mp3Encoder::new(Mp3EncoderConfig {
        bitrate_kbps: 64,
        vbr_quality: None,
    });
    encoder
        .push_pcm_s16(samples, 1, sample_rate)
        .map_err(|error| SpeechError::Encoding {
            message: error.to_string(),
        })?;
    encoder.finish();
    let mut bytes = Vec::new();
    while let Ok(packet) = encoder.next_packet() {
        bytes.extend_from_slice(&packet);
    }
    (!bytes.is_empty())
        .then_some(bytes)
        .ok_or_else(|| SpeechError::Encoding {
            message: "MP3 encoder produced no frames".to_owned(),
        })
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
                bytes: encode_mp3(samples, sample_rate)?,
                media_type: "audio/mpeg".to_owned(),
            },
            AudioFormat::Wav => Self {
                bytes: encode_wav(samples, sample_rate),
                media_type: "audio/wav".to_owned(),
            },
            AudioFormat::Pcm => Self {
                bytes: encode_pcm(samples),
                media_type: format!("audio/pcm;rate={sample_rate}"),
            },
            AudioFormat::L16 => Self {
                bytes: encode_l16(samples),
                media_type: format!("audio/L16;rate={sample_rate}"),
            },
            AudioFormat::MuLaw => Self {
                bytes: samples.iter().copied().map(encode_ulaw).collect(),
                media_type: format!("audio/mulaw;rate={sample_rate}"),
            },
            AudioFormat::ALaw => Self {
                bytes: samples.iter().copied().map(encode_alaw).collect(),
                media_type: format!("audio/alaw;rate={sample_rate}"),
            },
        };
        Ok(encoded)
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
        let wav = encode_wav(&[0, 1, -1], 24_000);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(wav.len(), 50);
    }

    #[test]
    fn l16_uses_network_byte_order() {
        assert_eq!(encode_l16(&[0x1234, -2]), [0x12, 0x34, 0xff, 0xfe]);
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
    fn mp3_starts_with_a_frame_sync() {
        let samples = vec![0; 1_200];
        let encoded = encode_mp3(&samples, 24_000).unwrap();
        assert_eq!(encoded[0], 0xff);
        assert_eq!(encoded[1] & 0xe0, 0xe0);
    }
}
