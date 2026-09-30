//! Conversion from provider wire contracts into provider-neutral types.

/// Consumes a provider contract and validates its canonical representation.
pub trait Lower {
    /// Provider-neutral representation produced by this conversion.
    type Canonical;

    /// Typed provider error raised when the contract cannot be represented.
    type Error;

    /// Validate and convert this provider contract.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] when the source contract cannot be represented
    /// by [`Self::Canonical`].
    fn lower(self) -> Result<Self::Canonical, Self::Error>;
}
