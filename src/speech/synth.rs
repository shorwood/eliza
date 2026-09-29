//! Thin adapter around Flite's embedded retro diphone voice.

use flite_rs::{Engine, Flow};

use super::core::{SpeechRequest, SpeechSegment};
use super::errors::SpeechError;

// -----------------------------------------------------------------------------
// VoiceSettings: Maps provider voice controls onto Flite.
// -----------------------------------------------------------------------------

/// Pitch, speed, and volume derived from provider voice controls.
#[derive(Clone, Copy)]
struct VoiceSettings {
    /// Multiplier applied to Flite's natural pitch.
    pitch: f32,
    /// Provider speaking-rate multiplier, where larger is faster.
    speed: f32,
    /// Integer attenuation used for quiet styles.
    amplitude_divisor: i16,
}

impl VoiceSettings {
    /// Stable pitch families selected from arbitrary provider voice names.
    const PITCHES: [f32; 4] = [0.86, 0.95, 1.05, 1.14];

    /// Compute stable FNV-1a without process-specific random state.
    const fn stable_hash(value: &str) -> u64 {
        let bytes = value.as_bytes();
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut index = 0;
        while index < bytes.len() {
            hash ^= bytes[index] as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            index += 1;
        }
        hash
    }

    /// Apply the compatibility fixture's deliberately small style vocabulary.
    fn apply_style(&mut self, style: &str) {
        let words = style
            .split(|character: char| !character.is_ascii_alphabetic())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        let has = |expected: &str| words.iter().any(|word| word == expected);
        if has("whisper") {
            self.pitch *= 1.08;
            self.speed *= 0.92;
            self.amplitude_divisor = 2;
        }
        if has("calm") || has("soft") {
            self.pitch *= 0.94;
            self.speed *= 0.90;
            self.amplitude_divisor = 2;
        }
        if has("excited") || has("cheerful") {
            self.pitch *= 1.16;
            self.speed *= 1.12;
        }
        if has("high") {
            self.pitch *= 1.20;
        }
        if has("low") || has("deep") {
            self.pitch *= 0.82;
        }
        if has("fast") || has("quick") {
            self.speed *= 1.25;
        }
        if has("slow") {
            self.speed *= 0.78;
        }
        self.speed = self.speed.clamp(0.25, 4.0);
    }

    /// Derive one deterministic Flite configuration from a speech segment.
    fn for_segment(segment: &SpeechSegment) -> Self {
        let hash_byte = Self::stable_hash(&segment.voice).to_le_bytes()[0];
        let mut settings = Self {
            pitch: Self::PITCHES[usize::from(hash_byte) % Self::PITCHES.len()],
            speed: segment.speed,
            amplitude_divisor: 1,
        };
        settings.apply_style(&segment.style);
        settings
    }

    /// Configure a reusable Flite engine for this segment.
    fn apply_to(self, engine: &mut Engine) {
        engine.set_f0_shift(self.pitch);
        engine.set_duration_stretch(self.speed.recip());
    }
}

// -----------------------------------------------------------------------------
// OutputRate: Restricts resampling to Flite's supported integer multiples.
// -----------------------------------------------------------------------------

/// Target sample rate expressed relative to Flite's bundled 8 kHz voice.
#[derive(Clone, Copy)]
struct OutputRate {
    /// Number of output samples written for each Flite sample.
    factor: usize,
    /// Number of target samples representing one second.
    samples_per_second: usize,
}

impl TryFrom<u32> for OutputRate {
    type Error = SpeechError;

    fn try_from(sample_rate: u32) -> Result<Self, Self::Error> {
        match sample_rate {
            8_000 => Ok(Self {
                factor: 1,
                samples_per_second: 8_000,
            }),
            16_000 => Ok(Self {
                factor: 2,
                samples_per_second: 16_000,
            }),
            24_000 => Ok(Self {
                factor: 3,
                samples_per_second: 24_000,
            }),
            sample_rate => Err(SpeechError::UnsupportedSampleRate { sample_rate }),
        }
    }
}

// -----------------------------------------------------------------------------
// SampleStream: Bounds and incrementally emits target-rate PCM.
// -----------------------------------------------------------------------------

/// Target-rate samples emitted together to downstream encoders.
const STREAM_CHUNK_SAMPLES: usize = 2_048;

/// Milliseconds in one second.
const MILLISECONDS_PER_SECOND: usize = 1_000;

/// Bounded resampler that flushes PCM while Flite is still synthesizing.
struct SampleStream<F> {
    /// Samples waiting for the next downstream write.
    pending: Vec<i16>,
    /// Number of target-rate samples accepted so far.
    total: usize,
    /// Maximum permitted target-rate sample count.
    maximum: usize,
    /// Requested integer-multiple output rate.
    rate: OutputRate,
    /// Downstream consumer for complete PCM chunks.
    sink: F,
    /// First limit or sink failure, which also stops Flite.
    failure: Option<SpeechError>,
}

impl<F> SampleStream<F>
where
    F: FnMut(&[i16]) -> Result<(), SpeechError>,
{
    /// Start an empty bounded output stream.
    fn new(maximum: usize, rate: OutputRate, sink: F) -> Self {
        Self {
            pending: Vec::with_capacity(STREAM_CHUNK_SAMPLES),
            total: 0,
            maximum,
            rate,
            sink,
            failure: None,
        }
    }

    /// Reject an append that would exceed the configured output duration.
    fn ensure_room(&mut self, additional: usize) -> Flow {
        if self.total.saturating_add(additional) > self.maximum {
            self.failure = Some(SpeechError::OutputTooLong);
            Flow::Stop
        } else {
            Flow::Continue
        }
    }

    /// Flush one transport-sized PCM chunk.
    fn flush(&mut self) -> Flow {
        // Empty codec writes add no information and can confuse frame encoders.
        if self.pending.is_empty() {
            return Flow::Continue;
        }
        match (self.sink)(&self.pending) {
            Ok(()) => {
                self.pending.clear();
                Flow::Continue
            }
            Err(error) => {
                self.failure = Some(error);
                Flow::Stop
            }
        }
    }

    /// Append one target-rate sample and flush full chunks immediately.
    fn push(&mut self, sample: i16) -> Flow {
        self.pending.push(sample);
        self.total += 1;
        if self.pending.len() == STREAM_CHUNK_SAMPLES {
            self.flush()
        } else {
            Flow::Continue
        }
    }

    /// Append one sample repeatedly for integer-rate conversion or silence.
    fn push_repeated(&mut self, sample: i16, count: usize) -> Flow {
        for _ in 0..count {
            // A disconnected downstream consumer stops synthesis immediately.
            if self.push(sample) == Flow::Stop {
                return Flow::Stop;
            }
        }
        Flow::Continue
    }

    /// Append Flite samples at the requested integer-multiple sample rate.
    fn append_audio(&mut self, source: &[i16], amplitude_divisor: i16) -> Flow {
        let additional = source.len().saturating_mul(self.rate.factor);

        // Stop Flite before accepting audio beyond the response-duration cap.
        if self.ensure_room(additional) == Flow::Stop {
            return Flow::Stop;
        }
        for sample in source {
            let scaled = *sample / amplitude_divisor;

            // Propagate a sink failure without synthesizing the remaining periods.
            if self.push_repeated(scaled, self.rate.factor) == Flow::Stop {
                return Flow::Stop;
            }
        }
        Flow::Continue
    }

    /// Append silence between independently voiced segments.
    fn append_pause(&mut self, milliseconds: u16) -> Flow {
        let count = self
            .rate
            .samples_per_second
            .saturating_mul(usize::from(milliseconds))
            / MILLISECONDS_PER_SECOND;

        // Segment pauses count toward the same response-duration cap as speech.
        if self.ensure_room(count) == Flow::Stop {
            return Flow::Stop;
        }
        self.push_repeated(0, count)
    }

    /// Flush the tail and return the exact emitted sample count.
    ///
    /// # Errors
    ///
    /// Returns the first duration-limit or downstream sink failure.
    fn finish(mut self) -> Result<usize, SpeechError> {
        if self.failure.is_none() {
            self.flush();
        }
        self.failure.map_or(Ok(self.total), Err)
    }
}

/// Stream all segments through Flite's bundled diphone voice.
///
/// # Errors
///
/// Returns [`SpeechError::OutputTooLong`] when output exceeds the supplied sample limit, or an
/// encoder failure when the downstream sink rejects PCM.
pub(super) fn synthesize_streaming(
    request: &SpeechRequest,
    maximum_samples: usize,
    sink: impl FnMut(&[i16]) -> Result<(), SpeechError>,
) -> Result<usize, SpeechError> {
    let rate = request.sample_rate.try_into()?;
    let mut engine = Engine::new();
    let mut output = SampleStream::new(maximum_samples, rate, sink);
    for segment in &request.segments {
        let settings = VoiceSettings::for_segment(segment);
        settings.apply_to(&mut engine);
        engine.synthesize_streaming(&segment.text, |period| {
            output.append_audio(period, settings.amplitude_divisor)
        });
        if output.failure.is_some() || output.append_pause(segment.pause_after_ms) == Flow::Stop {
            break;
        }
    }
    output.finish()
}

/// Collect streamed synthesis for complete-response formats.
///
/// # Errors
///
/// Returns the same bounded synthesis failures as [`synthesize_streaming`].
pub(super) fn synthesize(
    request: &SpeechRequest,
    maximum_samples: usize,
) -> Result<Vec<i16>, SpeechError> {
    let mut samples = Vec::new();
    synthesize_streaming(request, maximum_samples, |chunk| {
        samples.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(samples)
}

// -----------------------------------------------------------------------------
// Tests: Locks down Flite integration and provider controls.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Complete segment settings used by synthesis tests.
    #[derive(Clone, Copy)]
    struct TestProfile {
        /// Provider voice name.
        voice: &'static str,
        /// Provider style instructions.
        style: &'static str,
        /// Speaking-rate multiplier.
        speed: f32,
    }

    impl TestProfile {
        /// Maximum sample count used by ordinary test renders.
        const MAXIMUM_SAMPLES: usize = 1_000_000;

        /// Deliberately tiny bound used to exercise rejection.
        const TINY_SAMPLE_LIMIT: usize = 100;

        /// Native Flite output rate.
        const NATIVE_SAMPLE_RATE: u32 = 8_000;

        /// Highest provider output rate.
        const HIGH_SAMPLE_RATE: u32 = 24_000;

        /// Unstyled baseline request.
        const ORDINARY: Self = Self {
            voice: "Kore",
            style: "",
            speed: 1.0,
        };

        /// Distinct voice and style request.
        const STYLED: Self = Self {
            voice: "Puck",
            style: "whisper high",
            speed: 1.0,
        };

        /// Faster baseline request.
        const FAST: Self = Self {
            voice: "Kore",
            style: "",
            speed: 2.0,
        };

        /// Deliberately slow request.
        const SLOW: Self = Self {
            voice: "Kore",
            style: "slow",
            speed: 0.25,
        };

        /// Render this profile at one provider sample rate.
        ///
        /// # Errors
        ///
        /// Returns the synthesis error under test.
        fn render(self, sample_rate: u32, maximum_samples: usize) -> Result<Vec<i16>, SpeechError> {
            synthesize(
                &SpeechRequest {
                    segments: vec![SpeechSegment {
                        text: "You seem to be quite positive.".to_owned(),
                        voice: self.voice.to_owned(),
                        style: self.style.to_owned(),
                        speed: self.speed,
                        pause_after_ms: 0,
                    }],
                    sample_rate,
                },
                maximum_samples,
            )
        }
    }

    /// Flite output remains reproducible and non-silent.
    ///
    /// # Errors
    ///
    /// Returns the synthesis error under test.
    ///
    /// # Panics
    ///
    /// Panics when deterministic or audible output regresses.
    #[test]
    fn synthesis_is_deterministic_and_audible() -> Result<(), SpeechError> {
        let first = TestProfile::ORDINARY
            .render(TestProfile::HIGH_SAMPLE_RATE, TestProfile::MAXIMUM_SAMPLES)?;
        let second = TestProfile::ORDINARY
            .render(TestProfile::HIGH_SAMPLE_RATE, TestProfile::MAXIMUM_SAMPLES)?;
        assert_eq!(first, second);
        assert!(first.iter().any(|sample| *sample != 0));
        Ok(())
    }

    /// Provider voice, style, speed, and output rate remain observable.
    ///
    /// # Errors
    ///
    /// Returns the synthesis error under test.
    ///
    /// # Panics
    ///
    /// Panics when a provider control no longer changes the output.
    #[test]
    fn provider_controls_change_output() -> Result<(), SpeechError> {
        let ordinary = TestProfile::ORDINARY.render(
            TestProfile::NATIVE_SAMPLE_RATE,
            TestProfile::MAXIMUM_SAMPLES,
        )?;
        let styled = TestProfile::STYLED.render(
            TestProfile::NATIVE_SAMPLE_RATE,
            TestProfile::MAXIMUM_SAMPLES,
        )?;
        let fast = TestProfile::FAST.render(
            TestProfile::NATIVE_SAMPLE_RATE,
            TestProfile::MAXIMUM_SAMPLES,
        )?;
        let upsampled = TestProfile::ORDINARY.render(
            TestProfile::HIGH_SAMPLE_RATE,
            TestProfile::MAXIMUM_SAMPLES * 3,
        )?;
        assert_ne!(ordinary, styled);
        assert!(fast.len() < ordinary.len());
        assert_eq!(upsampled.len(), ordinary.len() * 3);
        Ok(())
    }

    /// The existing rendered-duration bound still rejects oversized output.
    ///
    /// # Panics
    ///
    /// Panics when synthesis ignores the output bound.
    #[test]
    fn enforces_rendered_duration_cap() {
        let result = TestProfile::SLOW.render(
            TestProfile::HIGH_SAMPLE_RATE,
            TestProfile::TINY_SAMPLE_LIMIT,
        );
        assert!(matches!(result, Err(SpeechError::OutputTooLong)));
    }
}
