//! Bounded hosted identity, entitlement reads and per-caller admission.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::http::HeaderMap;
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::problem::{NativeError, ProblemClass};

// -----------------------------------------------------------------------------
// Policy: Names finite storage, timing and transport bounds.
// -----------------------------------------------------------------------------

/// Fresh eligible cache window in seconds.
const POLICY_FRESH_SECONDS: u64 = 60;

/// Additional known-eligible outage grace in seconds.
const POLICY_STALE_SECONDS: u64 = 300;

/// Total finite outage access window.
const POLICY_OUTAGE_SECONDS: u64 = POLICY_FRESH_SECONDS + POLICY_STALE_SECONDS;

/// Denial refresh interval in seconds.
const POLICY_DENIAL_SECONDS: u64 = 5;

/// Retry and end-to-end lookup deadline in seconds.
const POLICY_LOOKUP_SECONDS: u64 = 1;

/// Concurrent lookup callers, including same-key waiters.
const POLICY_LOOKUP_CALLERS: usize = 8;

/// Maximum account identity length from the restricted source.
const POLICY_ACCOUNT_BYTES: usize = 64;

/// Maximum validated credential length.
const POLICY_KEY_BYTES: usize = 91;

/// Maximum retained entitlement entries.
const POLICY_CACHE_ENTRIES: usize = 10_000;

/// Maximum anonymous limiter identities.
const POLICY_PUBLIC_ENTRIES: usize = 100_000;

/// Maximum paid account limiter identities.
const POLICY_ACCOUNT_ENTRIES: usize = 10_000;

/// Maximum entries inspected in one maintenance pass.
const POLICY_CLEANUP_BATCH: usize = 256;

/// Minimum process-secret length.
const POLICY_SECRET_BYTES: usize = 32;

/// Maximum entitlement response bytes.
const POLICY_ENTITLEMENT_BYTES: usize = 8192;

/// Maximum allowed raw generation input bytes.
const POLICY_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Token accounting time unit.
const POLICY_MINUTE_SECONDS: f64 = 60.0;

/// Cold credential security allowance per minute.
const POLICY_COLD_PER_MINUTE: u32 = 120;

/// Cold credential security burst.
const POLICY_COLD_BURST: u32 = 8;

// -----------------------------------------------------------------------------
// Allowance: Replenishes one modality allowance.
// -----------------------------------------------------------------------------

/// Replenishing allowance for one request modality.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Allowance {
    /// Requests replenished each minute.
    pub per_minute: u32,
    /// Maximum replenished balance.
    pub burst: u32,
}

// -----------------------------------------------------------------------------
// Tier: Groups allowances shared by one caller class.
// -----------------------------------------------------------------------------

/// Caller allowances shared across provider families.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Tier {
    /// Text and embedding allowance.
    pub text: Allowance,
    /// Speech allowance.
    pub speech: Allowance,
    /// Image generation and inline-image allowance.
    pub image: Allowance,
    /// Model discovery allowance.
    pub discovery: Allowance,
    /// Simultaneous generation responses for one identity.
    pub concurrency: usize,
}

impl Tier {
    /// Select one independent modality allowance.
    fn allowance(&self, modality: Modality) -> Allowance {
        let credential = Allowance {
            per_minute: POLICY_COLD_PER_MINUTE,
            burst: POLICY_COLD_BURST,
        };
        match modality {
            Modality::Text => self.text,
            Modality::Speech => self.speech,
            Modality::Image => self.image,
            Modality::Discovery => self.discovery,
            Modality::Credential => credential,
        }
    }
}

// -----------------------------------------------------------------------------
// HostedConfig: Validates explicit deployment policy.
// -----------------------------------------------------------------------------

/// Hosted configuration; secrets are read from the process environment.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HostedConfig {
    /// Restricted Nitro entitlement endpoint.
    pub entitlement_url: String,
    /// HTTPS support page shown only on genuine anonymous allowance failures.
    pub support_url: String,
    /// Immediate ingress peers allowed to present authenticated client identity.
    pub trusted_peers: Vec<IpAddr>,
    /// Anonymous allowances, shared by IP.
    pub public: Tier,
    /// Paid allowances, shared by account.
    pub supporter: Tier,
    /// Global response capacity.
    pub responses: usize,
    /// Response capacity unavailable to public requests.
    pub reserved_responses: usize,
    /// Global active CPU jobs.
    pub cpu_jobs: usize,
    /// CPU slots unavailable to public jobs.
    pub reserved_cpu_jobs: usize,
    /// Global speech response/worker capacity.
    pub speech_jobs: usize,
    /// Speech slots unavailable to public jobs.
    pub reserved_speech_jobs: usize,
    /// Global image response capacity.
    pub image_jobs: usize,
    /// Image slots unavailable to public jobs.
    pub reserved_image_jobs: usize,
    /// Maximum raw body bytes, equally applied to both tiers.
    pub body_bytes: usize,
    /// Aggregate retained input buffer budget.
    pub buffer_bytes: usize,
    /// Buffer capacity unavailable to anonymous callers.
    pub reserved_buffer_bytes: usize,
    /// Maximum encoded output bytes.
    pub output_bytes: usize,
    /// Upload deadline in seconds.
    pub upload_seconds: u64,
    /// Response idle deadline in seconds.
    pub idle_seconds: u64,
    /// Absolute response lifetime in seconds.
    pub response_seconds: u64,
}

impl HostedConfig {
    /// Validate policy before binding the listener.
    ///
    /// # Errors
    /// Returns a safe configuration error for invalid bounds or endpoints.
    pub fn validate(&self) -> Result<(), &'static str> {
        for (total, reserved) in [
            (self.responses, self.reserved_responses),
            (self.cpu_jobs, self.reserved_cpu_jobs),
            (self.speech_jobs, self.reserved_speech_jobs),
            (self.image_jobs, self.reserved_image_jobs),
            (self.buffer_bytes, self.reserved_buffer_bytes),
        ] {
            // Reject an unsafe deployment policy before binding the listener.
            if reserved == 0 || reserved >= total || total > Semaphore::MAX_PERMITS {
                return Err("hosted totals must exceed positive reserved capacity");
            }
        }

        // Reject before accepting work or changing authoritative access.
        if self.body_bytes == 0
            || self.body_bytes > POLICY_BODY_BYTES
            || self.reserved_buffer_bytes < self.body_bytes
            || self.buffer_bytes - self.reserved_buffer_bytes < self.body_bytes
        {
            return Err("invalid hosted request, buffer, timeout or ingress bounds");
        }

        // Require finite transport bounds and at least one trusted peer.
        let deadlines = [
            self.upload_seconds,
            self.idle_seconds,
            self.response_seconds,
        ];

        // Require finite deadlines and an authenticated ingress configuration.
        let invalid_deadline = deadlines.iter().any(|seconds| {
            *seconds == 0
                || Instant::now()
                    .checked_add(Duration::from_secs(*seconds))
                    .is_none()
        });

        // Reject unsupported clock bounds before any listener or watchdog is created.
        if invalid_deadline || self.output_bytes == 0 || self.trusted_peers.is_empty() {
            return Err("invalid hosted request, buffer, timeout or ingress bounds");
        }

        // Validate caller limits independently of transport limits.
        for tier in [&self.public, &self.supporter] {
            // Reject before accepting work or changing authoritative access.
            if tier.concurrency == 0 || tier.concurrency > self.responses {
                return Err("invalid hosted caller concurrency");
            }

            // Every modality needs a positive refill rate and burst.
            if [tier.text, tier.speech, tier.image, tier.discovery]
                .iter()
                .any(|a| a.per_minute == 0 || a.burst == 0)
            {
                return Err("hosted allowances must be positive");
            }
        }

        // Permit plaintext only for the loopback development billing source.
        for (value, loopback_allowed) in [(&self.entitlement_url, true), (&self.support_url, false)]
        {
            let url = reqwest::Url::parse(value).map_err(|_| "invalid hosted URL")?;
            let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));

            // Reject before accepting work or changing authoritative access.
            if (url.scheme() != "https" && !(loopback_allowed && local && url.scheme() == "http"))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("hosted URLs require HTTPS without credentials, query or fragment");
            }
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Provider: Selects native SDK credential headers.
// -----------------------------------------------------------------------------

/// Native provider response/header selection.
#[derive(Debug, Clone, Copy)]
pub enum Provider {
    /// `OpenAI` and Gemini `OpenAI`-compatible routes.
    OpenAi,
    /// Anthropic routes.
    Anthropic,
    /// Native Gemini routes.
    Gemini,
    /// Ollama routes.
    Ollama,
}

// -----------------------------------------------------------------------------
// Modality: Selects the allowance before generation.
// -----------------------------------------------------------------------------

/// Work allowance selected before expensive execution.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Modality {
    /// Chat and embeddings.
    Text,
    /// Speech synthesis.
    Speech,
    /// Images, including inline images.
    Image,
    /// Model catalogs.
    Discovery,
    /// Cold credential resolution security allowance; never includes an offer.
    Credential,
}

// -----------------------------------------------------------------------------
// AccessTier: Selects protected or public capacity.
// -----------------------------------------------------------------------------

/// Caller class selected only by verified entitlement resolution.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AccessTier {
    /// Anonymous IP allowance.
    Public,
    /// Confirmed account allowance.
    Supporter,
}

impl AccessTier {
    /// Classify a resolved caller by its confirmed entitlement.
    const fn for_caller(caller: &Caller) -> Self {
        if caller.is_supporter {
            Self::Supporter
        } else {
            Self::Public
        }
    }

    /// Test the explicit paid-capacity alternative.
    #[must_use]
    pub const fn is_supporter(self) -> bool {
        matches!(self, Self::Supporter)
    }
}

// -----------------------------------------------------------------------------
// Entitlement: Decodes the restricted billing response.
// -----------------------------------------------------------------------------

/// Typed restricted billing response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entitlement {
    /// Whether billing confirmed a current paid or grace period.
    #[serde(rename = "eligible")]
    is_eligible: bool,
    /// Stable account identity; absent for unknown keys.
    account_id: Option<String>,
    /// Absolute paid, grace or key expiry in Unix milliseconds.
    eligible_until: Option<u64>,
    /// Maximum fresh cache age advertised by billing.
    refresh_seconds: u64,
    /// Additional outage grace advertised by billing.
    stale_seconds: u64,
}

// -----------------------------------------------------------------------------
// Cached: Retains finite freshness and outage clocks.
// -----------------------------------------------------------------------------

/// Last authoritative read and finite refresh/outage deadlines.
#[derive(Clone)]
struct Cached {
    /// Last authoritative entitlement read.
    value: Entitlement,
    /// Monotonic time of the last successful read.
    confirmed: Instant,
    /// Deadline for the next required refresh.
    fresh_until: Instant,
    /// Short backoff after a failed refresh.
    retry_at: Instant,
}

// -----------------------------------------------------------------------------
// CacheEntry: Coalesces lookups for one key hash.
// -----------------------------------------------------------------------------

/// One coalesced entitlement identity.
struct CacheEntry {
    /// Last confirmed state remains readable while a refresh is in flight.
    cached: Mutex<Option<Cached>>,
    /// Serializes refreshes without hiding already-confirmed eligibility.
    refresh: tokio::sync::Mutex<()>,
}

impl CacheEntry {
    // -----------------------------------------------------------------------------

    // IsExpired: Tests behavior.
    // -----------------------------------------------------------------------------

    /// Treat active lookups and locked state as ineligible for eviction.
    fn is_expired(self: &Arc<Self>) -> bool {
        Arc::strong_count(self) == 1
            && self.cached.try_lock().is_ok_and(|value| {
                value.as_ref().is_none_or(|cached| {
                    let seconds = if cached.value.is_eligible {
                        POLICY_OUTAGE_SECONDS
                    } else {
                        POLICY_DENIAL_SECONDS
                    };
                    cached.confirmed.elapsed() > Duration::from_secs(seconds)
                })
            })
    }
}

// -----------------------------------------------------------------------------
// CacheState: Bounds entitlement storage and maintenance.
// -----------------------------------------------------------------------------

/// Bounded entries and bounded-cost cleanup rotation.
struct CacheState {
    /// Bounded retained identity entries.
    entries: HashMap<String, Arc<CacheEntry>>,
    /// One cleanup queue entry per retained identity.
    maintenance: VecDeque<String>,
}

impl CacheState {
    // -----------------------------------------------------------------------------

    // Cleanup: Tests behavior.
    // -----------------------------------------------------------------------------

    /// Evict only idle entries whose finite retained result has expired.
    fn cleanup(&mut self) {
        for _ in 0..POLICY_CLEANUP_BATCH {
            let Some(key) = self.maintenance.pop_front() else {
                break;
            };
            let expired = self.entries.get(&key).is_some_and(CacheEntry::is_expired);
            if expired {
                self.entries.remove(&key);
            } else {
                self.maintenance.push_back(key);
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Bucket: Accounts for monotonic fractional refill.
// -----------------------------------------------------------------------------

/// Fractional monotonic token balance.
struct Bucket {
    /// Current fractional token balance.
    tokens: f64,
    /// Last monotonic refill time.
    updated: Instant,
}

impl Bucket {
    /// Charge one monotonic token or report the rounded-up refill delay.
    ///
    /// # Errors
    /// Returns the minimum whole seconds before one token becomes available.
    fn charge(&mut self, rate: Allowance) -> Result<(), u64> {
        let now = Instant::now();
        let refill = now.duration_since(self.updated).as_secs_f64() * f64::from(rate.per_minute)
            / POLICY_MINUTE_SECONDS;
        self.tokens = (self.tokens + refill).min(f64::from(rate.burst));
        self.updated = now;

        // Refuse depleted balances without resetting their refill clock.
        if self.tokens < 1.0 {
            let retry =
                ((1.0 - self.tokens) * POLICY_MINUTE_SECONDS / f64::from(rate.per_minute)).ceil();
            return Err(Duration::from_secs_f64(retry).as_secs().max(1));
        }
        self.tokens -= 1.0;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Identity: Retains caller accounting through active work.
// -----------------------------------------------------------------------------

/// One caller accounting record retained through active responses.
struct Identity {
    /// Independent allowances for each modality.
    buckets: HashMap<Modality, Bucket>,
    /// Active responses held by this identity.
    inflight: usize,
    /// Last successful charge or active request.
    touched: Instant,
    /// Whether the identity has a confirmed paid entitlement.
    is_supporter: bool,
}

impl Identity {
    /// Charge one modality without changing response concurrency.
    ///
    /// # Errors
    /// Returns seconds until a depleted allowance refills.
    fn charge(&mut self, modality: Modality, rate: Allowance) -> Result<(), u64> {
        let now = Instant::now();
        let bucket = self.buckets.entry(modality).or_insert(Bucket {
            tokens: f64::from(rate.burst),
            updated: now,
        });
        bucket.charge(rate)?;
        self.touched = now;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// LimiterState: Bounds identities and maintenance costs.
// -----------------------------------------------------------------------------

/// Bounded accounting and constant-time admission counts.
struct LimiterState {
    /// Bounded retained identity entries.
    entries: HashMap<String, Identity>,
    /// One cleanup queue entry per retained identity.
    maintenance: VecDeque<String>,
    /// Anonymous identities currently retained.
    public_count: usize,
    /// Account identities currently retained.
    supporter_count: usize,
}

impl LimiterState {
    /// Remove one expired identity while retaining exact capacity counts.
    fn remove(&mut self, key: &str) {
        // Preserve accounting when no entry exists.
        let Some(entry) = self.entries.remove(key) else {
            return;
        };
        if entry.is_supporter {
            self.supporter_count -= 1;
        } else {
            self.public_count -= 1;
        }
    }

    /// Allocate one bounded identity without evicting depleted balances.
    ///
    /// # Errors
    /// Returns shared overload when identity capacity is exhausted.
    fn ensure(&mut self, caller: &Caller) -> Result<(), NativeError> {
        // Existing identities retain their original accounting.
        if self.entries.contains_key(&caller.identity) {
            return Ok(());
        }
        let (count, maximum) = if caller.is_supporter {
            (&mut self.supporter_count, POLICY_ACCOUNT_ENTRIES)
        } else {
            (&mut self.public_count, POLICY_PUBLIC_ENTRIES)
        };

        // Saturation is shared overload, never a paid upgrade opportunity.
        if *count >= maximum {
            return Err(NativeError::unavailable());
        }
        *count += 1;
        self.maintenance.push_back(caller.identity.clone());

        // Store empty balances; the first charge initializes the chosen modality.
        let entry = Identity {
            buckets: HashMap::new(),
            inflight: 0,
            touched: Instant::now(),
            is_supporter: caller.is_supporter,
        };

        // Retain the caller's initialized accounting until it fully refills.
        self.entries.insert(caller.identity.clone(), entry);
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// IdentityLease: Releases caller concurrency on body completion.
// -----------------------------------------------------------------------------

/// Per-identity response accounting, released exactly once on drop.
pub struct IdentityLease {
    /// Shared bounded rate and active-response accounting.
    identities: Arc<Mutex<LimiterState>>,
    /// Nonsecret IP or account identity owned by this response.
    identity: String,
}

impl Drop for IdentityLease {
    fn drop(&mut self) {
        // A poisoned limiter cannot safely change its accounting.
        let Ok(mut identities) = self.identities.lock() else {
            return;
        };

        // Maintenance can already have removed inactive accounting.
        let Some(entry) = identities.entries.get_mut(&self.identity) else {
            return;
        };
        entry.inflight = entry.inflight.saturating_sub(1);
    }
}

// -----------------------------------------------------------------------------
// Caller: Projects trusted IP or confirmed account identity.
// -----------------------------------------------------------------------------

/// Resolved caller identity; no raw key or key hash is retained.
#[derive(Debug, Clone)]
pub struct Caller {
    /// Stable limiter identity, shared by an account's keys.
    identity: String,
    /// Whether a paid entitlement has been confirmed.
    pub is_supporter: bool,
}

impl Caller {
    /// Select the capacity class from confirmed account state.
    #[must_use]
    pub const fn tier(&self) -> AccessTier {
        AccessTier::for_caller(self)
    }
}

impl TryFrom<&Cached> for Caller {
    type Error = NativeError;

    fn try_from(cached: &Cached) -> Result<Self, Self::Error> {
        // Never extend cached access past an authoritative denial or deadline.
        if !cached.value.is_eligible {
            return Err(NativeError::unauthorized());
        }
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| NativeError::unavailable())?
            .as_millis();

        // Never extend cached access past an authoritative denial or deadline.
        if cached
            .value
            .eligible_until
            .is_none_or(|until| u128::from(until) <= millis)
        {
            return Err(NativeError::unauthorized());
        }

        // Never extend cached access past an authoritative denial or deadline.
        let outage = POLICY_FRESH_SECONDS + cached.value.stale_seconds.min(POLICY_STALE_SECONDS);

        // A failed refresh must never extend the finite outage deadline.
        if cached.confirmed.elapsed() > Duration::from_secs(outage) {
            return Err(NativeError::unavailable());
        }

        // Project only a bounded nonsecret account identity.
        let account = cached
            .value
            .account_id
            .as_ref()
            .filter(|v| !v.is_empty() && v.len() <= POLICY_ACCOUNT_BYTES)
            .ok_or_else(NativeError::unavailable)?;

        // Share every key belonging to this confirmed account.
        Ok(Caller {
            identity: format!("account:{account}"),
            is_supporter: true,
        })
    }
}

// -----------------------------------------------------------------------------
// CapacityBudget: Names total and reserved admission roles.
// -----------------------------------------------------------------------------

/// Named total and reserved roles for one capacity gate.
#[derive(Debug, Clone, Copy)]
pub struct CapacityBudget {
    /// Total available units.
    pub total: usize,
    /// Units unavailable to public traffic.
    pub reserved: usize,
}

// -----------------------------------------------------------------------------
// CapacityLease: Retains capacity through actual completion.
// -----------------------------------------------------------------------------

/// Owned capacity retained until the actual operation or body ends.
#[derive(Debug)]
pub struct CapacityLease {
    /// Retains total capacity until actual completion.
    _total: OwnedSemaphorePermit,
    /// Retains the public portion when required.
    _public: Option<OwnedSemaphorePermit>,
}

// -----------------------------------------------------------------------------
// Capacity: Claims reserved capacity without a queue.
// -----------------------------------------------------------------------------

/// Total capacity plus a public cap; reserved work is never lent out.
#[derive(Debug)]
pub struct Capacity {
    /// Total active capacity across both tiers.
    total: Arc<Semaphore>,
    /// Anonymous capacity excluding the reservation.
    public: Arc<Semaphore>,
}

impl TryFrom<CapacityBudget> for Capacity {
    type Error = &'static str;

    fn try_from(budget: CapacityBudget) -> Result<Self, Self::Error> {
        let CapacityBudget { total, reserved } = budget;

        // Reject invalid budgets before subtraction or semaphore allocation.
        if reserved == 0 || reserved >= total || total > Semaphore::MAX_PERMITS {
            return Err("invalid capacity budget");
        }
        Ok(Self {
            total: Arc::new(Semaphore::new(total)),
            public: Arc::new(Semaphore::new(total - reserved)),
        })
    }
}

impl Capacity {
    /// Claim free capacity immediately, with no implicit wait queue.
    ///
    /// # Errors
    /// Returns shared overload when either required pool is occupied.
    pub fn acquire(&self, tier: AccessTier, units: u32) -> Result<CapacityLease, NativeError> {
        let public = if tier.is_supporter() {
            None
        } else {
            Some(
                Arc::clone(&self.public)
                    .try_acquire_many_owned(units)
                    .map_err(|_| NativeError::unavailable())?,
            )
        };
        let total = Arc::clone(&self.total)
            .try_acquire_many_owned(units)
            .map_err(|_| NativeError::unavailable())?;
        Ok(CapacityLease {
            _total: total,
            _public: public,
        })
    }
}

// -----------------------------------------------------------------------------
// HostedSecrets: Separates ingress and billing credentials.
// -----------------------------------------------------------------------------

/// Explicitly named process credentials, never loaded from the JSON policy.
pub struct HostedSecrets {
    /// Immediate ingress authentication token.
    pub ingress_secret: String,
    /// Internal Nitro read token.
    pub entitlement_secret: String,
}

// -----------------------------------------------------------------------------
// HasElizaToken: Recognizes malformed ELIZA credentials.
// -----------------------------------------------------------------------------

/// Recognize an ELIZA credential even when its bearer prefix is malformed.
fn has_eliza_token(value: &str) -> bool {
    value.contains("eliza_")
}

// -----------------------------------------------------------------------------
// Canonical: Groups anonymous network identities.
// -----------------------------------------------------------------------------

/// Preserve IPv4 mapping; otherwise group a public IPv6 /64.
fn canonical_ipv6(v6: Ipv6Addr) -> IpAddr {
    let segments = v6.segments();
    v6.to_ipv4_mapped().map_or_else(
        || {
            IpAddr::V6(Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                0,
                0,
                0,
                0,
            ))
        },
        IpAddr::V4,
    )
}

/// Group anonymous IPv6 addresses and normalize mapped IPv4 identities.
fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => canonical_ipv6(v6),
        v4 @ IpAddr::V4(_) => v4,
    }
}

// -----------------------------------------------------------------------------
// Hosted: Shares admission and entitlement policy.
// -----------------------------------------------------------------------------

/// Shared hosted policy for one origin process.
pub struct Hosted {
    /// Validated, nonsecret policy.
    config: HostedConfig,
    /// Separate ingress authentication credential, never serialized.
    ingress_secret: String,
    /// Restricted Nitro read credential, never serialized.
    entitlement_secret: String,
    /// Reused bounded asynchronous entitlement client.
    client: reqwest::Client,
    /// Bounded key-hash cache; no raw credential values.
    cache: Mutex<CacheState>,
    /// Bounds cold resolution callers, including coalesced waiters.
    lookups: Arc<Semaphore>,
    /// Shared bounded rate and active-response accounting.
    identities: Arc<Mutex<LimiterState>>,
    /// Response reservation independent of worker lifetimes.
    pub responses: Capacity,
    /// Aggregate active CPU work.
    pub cpu: Capacity,
    /// Speech admission shared across provider families.
    pub speech: Capacity,
    /// Image admission, including inline-image chat.
    pub image: Capacity,
    /// Aggregate retained input buffer capacity.
    pub buffers: Capacity,
}

impl Hosted {
    /// Construct reusable clients and bounded state before accepting traffic.
    ///
    /// # Errors
    /// Returns sanitized validation/client initialization errors.
    #[expect(
        rlib::missing_code_phase_comments,
        reason = "the gate initializer declaratively maps validated named policy fields"
    )]
    pub fn new(config: HostedConfig, secrets: HostedSecrets) -> Result<Self, &'static str> {
        let HostedSecrets {
            ingress_secret,
            entitlement_secret,
        } = secrets;
        config.validate()?;

        // Reject an unsafe deployment policy before binding the listener.
        if ingress_secret.len() < POLICY_SECRET_BYTES
            || entitlement_secret.len() < POLICY_SECRET_BYTES
        {
            return Err("hosted secrets require at least 32 bytes");
        }

        // Reuse a deadline-bounded client without redirecting its secret.
        let client_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(POLICY_LOOKUP_SECONDS))
            .redirect(reqwest::redirect::Policy::none());
        let client = client_builder
            .build()
            .map_err(|_| "cannot initialize entitlement client")?;

        // Initialize capacity gates from already validated policy.
        Ok(Self {
            responses: Capacity::try_from(CapacityBudget {
                total: config.responses,
                reserved: config.reserved_responses,
            })?,
            cpu: Capacity::try_from(CapacityBudget {
                total: config.cpu_jobs,
                reserved: config.reserved_cpu_jobs,
            })?,
            speech: Capacity::try_from(CapacityBudget {
                total: config.speech_jobs,
                reserved: config.reserved_speech_jobs,
            })?,
            image: Capacity::try_from(CapacityBudget {
                total: config.image_jobs,
                reserved: config.reserved_image_jobs,
            })?,
            buffers: Capacity::try_from(CapacityBudget {
                total: config.buffer_bytes,
                reserved: config.reserved_buffer_bytes,
            })?,
            config,
            client,
            ingress_secret,
            entitlement_secret,
            cache: Mutex::new(CacheState {
                entries: HashMap::new(),
                maintenance: VecDeque::new(),
            }),
            lookups: Arc::new(Semaphore::new(POLICY_LOOKUP_CALLERS)),
            identities: Arc::new(Mutex::new(LimiterState {
                entries: HashMap::new(),
                maintenance: VecDeque::new(),
                public_count: 0,
                supporter_count: 0,
            })),
        })
    }

    /// Validate UUID separators and the URL-safe secret after prefix/length checks.
    ///
    /// # Errors
    /// Rejects malformed trusted-prefix credentials.
    fn validate_key(token: &str) -> Result<(), NativeError> {
        let bytes = token.as_bytes();
        let uuid_valid = bytes[11..47].iter().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });

        // Validate the separator and URL-safe secret after checking all slice bounds.
        if !uuid_valid
            || bytes[47] != b'_'
            || !bytes[48..]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(NativeError::unauthorized());
        }
        Ok(())
    }

    /// Read one unambiguous native SDK credential.
    ///
    /// # Errors
    /// Rejects malformed or conflicting ELIZA credentials.
    fn credential(headers: &HeaderMap, provider: Provider) -> Result<Option<&str>, NativeError> {
        // Reject before accepting work or changing authoritative access.
        if ["authorization", "x-api-key", "x-goog-api-key"]
            .into_iter()
            .filter(|name| headers.contains_key(*name))
            .count()
            > 1
        {
            return Err(NativeError::unauthorized());
        }

        // Select the provider's native header without downgrading other ELIZA keys.
        let header = match provider {
            Provider::OpenAi | Provider::Ollama => "authorization",
            Provider::Anthropic => "x-api-key",
            Provider::Gemini => "x-goog-api-key",
        };

        // Reject before accepting work or changing authoritative access.
        if headers.get_all(header).iter().count() > 1
            || headers.iter().any(|(name, value)| {
                ["authorization", "x-api-key", "x-goog-api-key"].contains(&name.as_str())
                    && name != header
                    && value.to_str().is_ok_and(has_eliza_token)
            })
        {
            return Err(NativeError::unauthorized());
        }

        // Decode only the provider's single native credential value.
        let value = headers
            .get(header)
            .map(|v| v.to_str().map_err(|_| NativeError::unauthorized()))
            .transpose()?
            .unwrap_or_default();

        // Reject before accepting work or changing authoritative access.
        if header == "authorization" && has_eliza_token(value) && !value.starts_with("Bearer ") {
            return Err(NativeError::unauthorized());
        }
        let token = if header == "authorization" {
            value.strip_prefix("Bearer ").unwrap_or(value)
        } else {
            value
        };

        // Reuse already verified state without another billing read.
        if !token.starts_with("eliza_") {
            return Ok(None);
        }

        // Reject before accepting work or changing authoritative access.
        if !token.starts_with("eliza_test_") || token.len() != POLICY_KEY_BYTES {
            return Err(NativeError::unauthorized());
        }

        // Validate the key layout only after checking its exact prefix and length.
        Self::validate_key(token)?;
        Ok(Some(token))
    }

    /// Read the validated immutable policy.
    #[must_use]
    pub const fn config(&self) -> &HostedConfig {
        &self.config
    }

    /// Verify ingress before trusting the single sanitized identity header.
    ///
    /// # Errors
    /// Rejects untrusted peers, duplicate/missing identity and wrong ingress credentials.
    pub fn client_ip(&self, peer: IpAddr, headers: &HeaderMap) -> Result<IpAddr, NativeError> {
        // Verify the secret separately from the untrusted identity header.
        let supplied = headers
            .get("x-eliza-ingress-token")
            .ok_or_else(NativeError::unauthorized)?;
        let supplied = supplied.to_str().map_err(|_| NativeError::unauthorized())?;

        // Compare equal-length secrets without a content-dependent early exit.
        let supplied_bytes = supplied.as_bytes();
        let matches = supplied.len() == self.ingress_secret.len()
            && supplied_bytes
                .iter()
                .zip(self.ingress_secret.as_bytes())
                .fold(0u8, |d, (a, b)| d | (a ^ b))
                == 0;

        // Reject an unsafe deployment policy before binding the listener.
        if !self.config.trusted_peers.contains(&peer)
            || !matches
            || headers.get_all("x-eliza-ingress-token").iter().count() != 1
            || headers.get_all("x-eliza-client-ip").iter().count() != 1
        {
            return Err(NativeError::unauthorized());
        }

        // Read the one sanitized IP only after ingress authentication.
        let supplied = headers
            .get("x-eliza-client-ip")
            .ok_or_else(NativeError::unauthorized)?;
        let supplied = supplied.to_str().map_err(|_| NativeError::unauthorized())?;
        let parsed = supplied.parse().map_err(|_| NativeError::unauthorized())?;
        Ok(canonical_ip(parsed))
    }

    /// Remove a bounded batch of idle fully-refillable identities.
    pub fn cleanup(&self) {
        // A poisoned limiter cannot safely remove retained balances.
        let Ok(mut identities) = self.identities.lock() else {
            return;
        };
        for _ in 0..POLICY_CLEANUP_BATCH {
            let Some(key) = identities.maintenance.pop_front() else {
                break;
            };
            let expired = identities.entries.get(&key).is_some_and(|entry| {
                let tier = if entry.is_supporter {
                    &self.config.supporter
                } else {
                    &self.config.public
                };
                let refill = [
                    tier.text,
                    tier.speech,
                    tier.image,
                    tier.discovery,
                    Allowance {
                        per_minute: POLICY_COLD_PER_MINUTE,
                        burst: POLICY_COLD_BURST,
                    },
                ]
                .into_iter()
                .map(|a| f64::from(a.burst) * POLICY_MINUTE_SECONDS / f64::from(a.per_minute))
                .fold(0.0, f64::max);
                entry.inflight == 0
                    && entry.touched.elapsed().as_secs_f64() >= refill.max(POLICY_MINUTE_SECONDS)
            });
            if expired {
                identities.remove(&key);
            } else {
                identities.maintenance.push_back(key);
            }
        }
    }

    /// Claim one cache entry with bounded eviction work.
    ///
    /// # Errors
    /// Returns overload when the bounded cache cannot retain another identity.
    fn cache_entry(&self, digest: &str) -> Result<Arc<CacheEntry>, NativeError> {
        let mut cache = self.cache.lock().map_err(|_| NativeError::unavailable())?;

        // Reuse an existing coalescing entry without changing its clocks.
        if let Some(entry) = cache.entries.get(digest) {
            return Ok(Arc::clone(entry));
        }

        // Rotate at most one bounded batch before refusing new retained state.
        if cache.entries.len() >= POLICY_CACHE_ENTRIES {
            cache.cleanup();
        }

        // Never evict a live approval merely to accommodate a new key.
        if cache.entries.len() >= POLICY_CACHE_ENTRIES {
            return Err(NativeError::unavailable());
        }

        // Retain one bounded entry with independent read and refresh ownership.
        let entry = Arc::new(CacheEntry {
            cached: Mutex::new(None),
            refresh: tokio::sync::Mutex::new(()),
        });
        cache.entries.insert(digest.to_owned(), Arc::clone(&entry));
        cache.maintenance.push_back(digest.to_owned());
        Ok(entry)
    }

    /// Read a bounded typed internal billing response.
    ///
    /// # Errors
    /// Returns an opaque failure for malformed or unavailable billing responses.
    async fn read_entitlement(&self, digest: &str) -> Result<Entitlement, ()> {
        // Authenticate the bounded internal lookup, never the generation provider.
        let request = self
            .client
            .post(&self.config.entitlement_url)
            .bearer_auth(&self.entitlement_secret)
            .body(digest.to_owned());
        let response = request.send().await.map_err(|_| ())?;

        // Reject before accepting work or changing authoritative access.
        if !response.status().is_success() {
            return Err(());
        }

        // Bound decoded response bytes before JSON deserialization.
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(frame) = stream.next().await {
            let frame = frame.map_err(|_| ())?;

            // Reject before accepting work or changing authoritative access.
            if bytes.len() + frame.len() > POLICY_ENTITLEMENT_BYTES {
                return Err(());
            }
            bytes.extend_from_slice(&frame);
        }

        // Require sufficient authoritative data before caching paid access.
        let value: Entitlement = serde_json::from_slice(&bytes).map_err(|_| ())?;

        // Reject before accepting work or changing authoritative access.
        if value.is_eligible
            && (value
                .account_id
                .as_ref()
                .is_none_or(|v| v.is_empty() || v.len() > POLICY_ACCOUNT_BYTES)
                || value.eligible_until.is_none())
        {
            return Err(());
        }
        Ok(value)
    }

    /// Offer support only for genuine anonymous generation allowances.
    fn limit_error(&self, caller: &Caller, modality: Modality, seconds: u64) -> NativeError {
        let message = if modality == Modality::Credential {
            "Credential lookup allowance exhausted; retry shortly.".into()
        } else if modality == Modality::Discovery {
            "Discovery allowance temporarily exhausted; retry shortly.".into()
        } else if caller.is_supporter {
            "Your account allowance is temporarily exhausted; retry shortly.".into()
        } else {
            format!(
                "Public allowance temporarily exhausted; retry shortly. Support ELIZA for $3/month: {}",
                self.config.support_url
            )
        };
        NativeError::admission(ProblemClass::RateLimit, message, Some(seconds))
    }

    /// Charge additional inline-image work without counting a second HTTP response.
    ///
    /// # Errors
    /// Returns the caller's image allowance failure or shared bounded-state overload.
    pub fn charge(&self, caller: &Caller, modality: Modality) -> Result<(), NativeError> {
        let mut identities = self
            .identities
            .lock()
            .map_err(|_| NativeError::unavailable())?;
        identities.ensure(caller)?;
        let entry = identities
            .entries
            .get_mut(&caller.identity)
            .ok_or_else(NativeError::unavailable)?;
        let tier = if caller.is_supporter {
            &self.config.supporter
        } else {
            &self.config.public
        };
        entry
            .charge(modality, tier.allowance(modality))
            .map_err(|seconds| self.limit_error(caller, modality, seconds))
    }

    /// Claim only caller response concurrency before reading an upload.
    ///
    /// # Errors
    /// Returns a genuine personal concurrency failure or bounded-state overload.
    pub fn begin(&self, caller: &Caller, modality: Modality) -> Result<IdentityLease, NativeError> {
        let mut identities = self
            .identities
            .lock()
            .map_err(|_| NativeError::unavailable())?;
        identities.ensure(caller)?;

        // Retain this caller's existing state while claiming response ownership.
        let entry = identities
            .entries
            .get_mut(&caller.identity)
            .ok_or_else(NativeError::unavailable)?;

        // Select the concurrency limit for this resolved caller class.
        let tier = if caller.is_supporter {
            &self.config.supporter
        } else {
            &self.config.public
        };

        // Concurrency counts actual HTTP responses, including admitted uploads.
        if entry.inflight >= tier.concurrency {
            return Err(self.limit_error(caller, modality, 1));
        }
        entry.inflight += 1;
        entry.touched = Instant::now();
        Ok(IdentityLease {
            identities: Arc::clone(&self.identities),
            identity: caller.identity.clone(),
        })
    }

    /// Own response admission with one modality charge.
    ///
    /// # Errors
    /// Returns personal concurrency, rate or bounded-state failures.
    pub fn admit(&self, caller: &Caller, modality: Modality) -> Result<IdentityLease, NativeError> {
        let lease = self.begin(caller, modality)?;
        self.charge(caller, modality)?;
        Ok(lease)
    }

    /// Resolve one key under the end-to-end deadline.
    ///
    /// # Errors
    /// Returns authentication, bounded-state or lookup failures.
    async fn resolve_caller(
        &self,
        ip: IpAddr,
        headers: &HeaderMap,
        provider: Provider,
    ) -> Result<Caller, NativeError> {
        // SDK placeholders stay anonymous and never allocate entitlement cache entries.
        let Some(token) = Self::credential(headers, provider)? else {
            return Ok(Caller {
                identity: format!("ip:{ip}"),
                is_supporter: false,
            });
        };

        // Hash the bounded key before retaining any identity or calling billing.
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        let entry = self.cache_entry(&digest)?;
        let snapshot = entry
            .cached
            .lock()
            .map_err(|_| NativeError::unavailable())?
            .clone();

        // Reuse fresh state while another caller may be refreshing the same key.
        if let Some(cached) = snapshot
            .as_ref()
            .filter(|value| Instant::now() < value.fresh_until)
        {
            return Caller::try_from(cached);
        }
        let security_caller = Caller {
            identity: format!("ip:{ip}"),
            is_supporter: false,
        };

        // Cold misses consume the IP security allowance; cached refreshes do not.
        if snapshot.is_none() {
            let security = self.admit(&security_caller, Modality::Credential)?;
            drop(security);
        }

        // Bound lookup callers including same-key coalescing waiters.
        let _lookup = Arc::clone(&self.lookups)
            .try_acquire_owned()
            .map_err(|_| NativeError::unavailable())?;

        // Coalesce the lookup and recheck after any preceding caller completes.
        let _refresh = entry.refresh.lock().await;
        let mut value = entry
            .cached
            .lock()
            .map_err(|_| NativeError::unavailable())?
            .clone();
        let now = Instant::now();

        // Reuse already verified state without another billing read.
        if let Some(cached) = value
            .as_ref()
            .filter(|c| now < c.fresh_until || now < c.retry_at)
        {
            return Caller::try_from(cached);
        }
        if let Ok(entitlement) = self.read_entitlement(&digest).await {
            let fresh = if entitlement.is_eligible {
                entitlement.refresh_seconds.min(POLICY_FRESH_SECONDS)
            } else {
                POLICY_DENIAL_SECONDS
            };
            let cached = Cached {
                value: entitlement,
                confirmed: Instant::now(),
                fresh_until: Instant::now() + Duration::from_secs(fresh),
                retry_at: Instant::now(),
            };
            let caller = Caller::try_from(&cached);
            *entry
                .cached
                .lock()
                .map_err(|_| NativeError::unavailable())? = Some(cached);
            caller
        } else if let Some(cached) = value.as_mut() {
            cached.retry_at = Instant::now() + Duration::from_secs(POLICY_LOOKUP_SECONDS);
            let caller = Caller::try_from(&*cached);
            *entry
                .cached
                .lock()
                .map_err(|_| NativeError::unavailable())? = value;
            caller
        } else {
            Err(NativeError::unavailable())
        }
    }

    /// Recover only a last-confirmed result after lookup saturation or deadline.
    ///
    /// # Errors
    /// Unknown state remains unavailable; denied or expired state remains unauthorized.
    fn cached_caller(
        &self,
        headers: &HeaderMap,
        provider: Provider,
    ) -> Result<Caller, NativeError> {
        let token = Self::credential(headers, provider)?.ok_or_else(NativeError::unavailable)?;
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        let entry = {
            let cache = self.cache.lock().map_err(|_| NativeError::unavailable())?;
            cache
                .entries
                .get(&digest)
                .cloned()
                .ok_or_else(NativeError::unavailable)?
        };
        let mut value = entry
            .cached
            .lock()
            .map_err(|_| NativeError::unavailable())?;
        let cached = value.as_mut().ok_or_else(NativeError::unavailable)?;
        cached.retry_at = Instant::now() + Duration::from_secs(POLICY_LOOKUP_SECONDS);
        Caller::try_from(&*cached)
    }

    /// Resolve a provider credential without letting arbitrary SDK placeholders create identities.
    ///
    /// # Errors
    /// Returns native authentication or retryable lookup failures.
    pub async fn caller(
        &self,
        ip: IpAddr,
        headers: &HeaderMap,
        provider: Provider,
    ) -> Result<Caller, NativeError> {
        let result = tokio::time::timeout(
            Duration::from_secs(POLICY_LOOKUP_SECONDS),
            self.resolve_caller(ip, headers, provider),
        )
        .await;
        match result {
            Ok(Err(error)) if error.status().is_server_error() => {
                self.cached_caller(headers, provider)
            }
            Ok(result) => result,
            Err(_) => self.cached_caller(headers, provider),
        }
    }
}

impl std::fmt::Debug for Hosted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hosted(<redacted>)")
    }
}
