//! Thin adapter around Flite's embedded retro diphone voice.

use flite_rs::Engine;

use super::core::{SpeechError, SpeechRequest, SpeechSegment};

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
// SampleBuffer: Owns bounded target-rate PCM assembly.
// -----------------------------------------------------------------------------

/// PCM output whose every append is checked against the duration bound.
struct SampleBuffer {
    /// Assembled target-rate samples.
    samples: Vec<i16>,
    /// Maximum permitted target-rate sample count.
    maximum: usize,
}

impl SampleBuffer {
    /// Start an empty bounded output buffer.
    const fn new(maximum: usize) -> Self {
        Self {
            samples: Vec::new(),
            maximum,
        }
    }

    /// Reject an append that would exceed the configured output duration.
    ///
    /// # Errors
    ///
    /// Returns [`SpeechError::OutputTooLong`] when the addition crosses the limit.
    fn ensure_room(&self, additional: usize) -> Result<(), SpeechError> {
        if self.samples.len().saturating_add(additional) > self.maximum {
            Err(SpeechError::OutputTooLong)
        } else {
            Ok(())
        }
    }

    /// Append Flite samples at the requested integer-multiple sample rate.
    ///
    /// # Errors
    ///
    /// Returns [`SpeechError::OutputTooLong`] when the resampled audio crosses the limit.
    fn append_audio(
        &mut self,
        source: &[i16],
        rate: OutputRate,
        amplitude_divisor: i16,
    ) -> Result<(), SpeechError> {
        let additional = source.len().saturating_mul(rate.factor);
        self.ensure_room(additional)?;
        self.samples.reserve(additional);
        for sample in source {
            let scaled = *sample / amplitude_divisor;
            self.samples
                .extend(std::iter::repeat_n(scaled, rate.factor));
        }
        Ok(())
    }

    /// Append silence between independently voiced segments.
    ///
    /// # Errors
    ///
    /// Returns [`SpeechError::OutputTooLong`] when the pause crosses the limit.
    fn append_pause(&mut self, milliseconds: u16, rate: OutputRate) -> Result<(), SpeechError> {
        let count = rate
            .samples_per_second
            .saturating_mul(usize::from(milliseconds))
            / 1_000;
        self.ensure_room(count)?;
        self.samples.resize(self.samples.len() + count, 0);
        Ok(())
    }

    /// Return the completed PCM buffer.
    fn finish(self) -> Vec<i16> {
        self.samples
    }
}

// -----------------------------------------------------------------------------
// Synthesize: Renders provider-neutral segments with Flite.
// -----------------------------------------------------------------------------

/// Render all segments through Flite's bundled diphone voice.
///
/// # Errors
///
/// Returns [`SpeechError::OutputTooLong`] when output exceeds the supplied sample limit, or an
/// unsupported-rate error when called without provider-neutral validation.
pub(super) fn synthesize(
    request: &SpeechRequest,
    maximum_samples: usize,
) -> Result<Vec<i16>, SpeechError> {
    let rate = request.sample_rate.try_into()?;
    let mut engine = Engine::new();
    let mut output = SampleBuffer::new(maximum_samples);
    for segment in &request.segments {
        let settings = VoiceSettings::for_segment(segment);
        settings.apply_to(&mut engine);
        let audio = engine.synthesize(&segment.text);
        output.append_audio(&audio.samples, rate, settings.amplitude_divisor)?;
        output.append_pause(segment.pause_after_ms, rate)?;
    }
    Ok(output.finish())
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
