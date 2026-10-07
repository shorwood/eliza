//! Contract tests for bounded hosted admission and entitlement reads.
use axum::http::{HeaderValue, StatusCode};

use super::*;

// -----------------------------------------------------------------------------
// HostedTestBilling: Serves controlled entitlement reads.
// -----------------------------------------------------------------------------

/// Bounded socket fixture with observable successful, denied and failed reads.
struct HostedTestBilling {
    /// Response selector: approved, denied, or unavailable.
    mode: Arc<std::sync::atomic::AtomicUsize>,
    /// Number of actual billing requests.
    calls: Arc<std::sync::atomic::AtomicUsize>,
    /// Ephemeral internal read endpoint.
    url: String,
    /// Owned server task, aborted on fixture drop.
    task: tokio::task::JoinHandle<()>,
}

impl HostedTestBilling {
    /// Start a local billing endpoint without external services.
    async fn start() -> Self {
        let mode = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let response_mode = Arc::clone(&mode);
        let response_calls = Arc::clone(&calls);
        let router = axum::Router::new().route("/billing/entitlement", axum::routing::post(move || {
            let mode = Arc::clone(&response_mode);
            let calls = Arc::clone(&response_calls);
            async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(10)).await;
                let mode = mode.load(std::sync::atomic::Ordering::Relaxed);
                if mode == 3 { tokio::time::sleep(Duration::from_secs(2)).await; }
                let status = if mode == 2 { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };
                let until = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() + 3_600_000;
                (status, axum::Json(serde_json::json!({"eligible": mode == 0, "accountId": "test-team", "eligibleUntil": until, "refreshSeconds":60, "staleSeconds":300})))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            mode,
            calls,
            url: format!("http://{address}/billing/entitlement"),
            task,
        }
    }
}

impl Drop for HostedTestBilling {
    fn drop(&mut self) {
        self.task.abort();
    }
}

// -----------------------------------------------------------------------------
// HostedTest: Verifies reservations, accounting and finite outage access.
// -----------------------------------------------------------------------------

/// Test credential whose value never needs to appear in logs.
const HOSTED_TEST_KEY: &str =
    "eliza_test_00000000-0000-0000-0000-000000000001_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// Force refresh and set monotonic age without sleeping through the outage window.
fn hosted_test_age(hosted: &Hosted, seconds: u64) {
    let digest = format!("{:x}", Sha256::digest(HOSTED_TEST_KEY.as_bytes()));
    let entry = hosted.cache_entry(&digest).unwrap();
    let mut value = entry.cached.lock().unwrap();
    let cached = value.as_mut().unwrap();
    cached.confirmed = Instant::now()
        .checked_sub(Duration::from_secs(seconds))
        .unwrap();
    cached.fresh_until = Instant::now();
    cached.retry_at = Instant::now();
}

/// Build a disposable hosted policy without initializing an external service.
fn hosted_test_policy() -> Hosted {
    let config: HostedConfig =
        serde_json::from_str(include_str!("../../../config/hosted-staging.json")).unwrap();
    Hosted::new(
        config,
        HostedSecrets {
            ingress_secret: "test-ingress-secret-at-least-32-bytes".into(),
            entitlement_secret: "test-entitlement-secret-at-least-32-bytes".into(),
        },
    )
    .unwrap()
}

/// Concurrent same-key misses coalesce; authoritative denial immediately removes paid access.
#[tokio::test]
async fn hosted_test_coalesces_and_revokes() {
    let billing = HostedTestBilling::start().await;
    let mut hosted = hosted_test_policy();
    hosted.config.entitlement_url.clone_from(&billing.url);
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {HOSTED_TEST_KEY}").parse().unwrap(),
    );
    let ip: IpAddr = "127.0.0.1".parse().unwrap();
    let lookups = (0..8).map(|_| hosted.caller(ip, &headers, Provider::OpenAi));
    let callers = futures_util::future::join_all(lookups).await;
    assert!(
        callers
            .iter()
            .all(|result| result.as_ref().is_ok_and(|caller| caller.is_supporter))
    );
    let calls = billing.calls.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(calls, 1);
    hosted_test_age(&hosted, 60);
    billing.mode.store(1, std::sync::atomic::Ordering::Relaxed);
    let result = hosted.caller(ip, &headers, Provider::OpenAi).await;
    let error = result.unwrap_err();
    let status = error.status();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let calls = billing.calls.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(calls, 2);
}

/// Failed reads retain only prior eligible state and never reset its last-success clock.
#[tokio::test]
async fn hosted_test_failed_reads_have_finite_grace() {
    let billing = HostedTestBilling::start().await;
    let mut hosted = hosted_test_policy();
    hosted.config.entitlement_url.clone_from(&billing.url);
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {HOSTED_TEST_KEY}").parse().unwrap(),
    );
    let ip: IpAddr = "127.0.0.1".parse().unwrap();
    let initial = hosted.caller(ip, &headers, Provider::OpenAi).await.unwrap();
    assert!(initial.is_supporter);
    billing.mode.store(2, std::sync::atomic::Ordering::Relaxed);
    hosted_test_age(&hosted, POLICY_OUTAGE_SECONDS - 1);
    let stale = hosted.caller(ip, &headers, Provider::OpenAi).await.unwrap();
    assert!(stale.is_supporter);
    hosted_test_age(&hosted, POLICY_OUTAGE_SECONDS + 1);
    let expired = hosted.caller(ip, &headers, Provider::OpenAi).await;
    let error = expired.unwrap_err();
    let status = error.status();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let mut restarted = hosted_test_policy();
    restarted.config.entitlement_url.clone_from(&billing.url);
    let unknown = restarted.caller(ip, &headers, Provider::OpenAi).await;
    let error = unknown.unwrap_err();
    let status = error.status();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

/// Deadline cancellation retains readable confirmed state for every coalesced caller.
#[tokio::test]
async fn hosted_test_deadline_keeps_finite_grace() {
    let billing = HostedTestBilling::start().await;
    let mut hosted = hosted_test_policy();
    hosted.config.entitlement_url.clone_from(&billing.url);
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {HOSTED_TEST_KEY}").parse().unwrap(),
    );
    let ip: IpAddr = "127.0.0.1".parse().unwrap();
    assert!(
        hosted
            .caller(ip, &headers, Provider::OpenAi)
            .await
            .unwrap()
            .is_supporter
    );
    billing.mode.store(3, std::sync::atomic::Ordering::Relaxed);
    hosted_test_age(&hosted, 60);
    let lookups = (0..8).map(|_| hosted.caller(ip, &headers, Provider::OpenAi));
    let callers = futures_util::future::join_all(lookups).await;
    assert!(
        callers
            .iter()
            .all(|result| result.as_ref().is_ok_and(|caller| caller.is_supporter))
    );
    let calls = billing.calls.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(calls, 2);
}

/// Anonymous saturation cannot consume the protected slot.
#[test]
fn hosted_test_reservations_are_hard_and_release_on_drop() {
    let capacity = Capacity::try_from(CapacityBudget {
        total: 2,
        reserved: 1,
    })
    .unwrap();
    let public = capacity.acquire(AccessTier::Public, 1).unwrap();
    assert_eq!(
        capacity
            .acquire(AccessTier::Public, 1)
            .err()
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let supporter = capacity.acquire(AccessTier::Supporter, 1).unwrap();
    assert!(capacity.acquire(AccessTier::Supporter, 1).is_err());
    drop(public);
    drop(supporter);
    assert!(capacity.acquire(AccessTier::Public, 1).is_ok());
}

/// All keys resolved to an account share the same bucket and no offer.
#[test]
fn hosted_test_account_allowance_and_concurrency_are_shared() {
    let mut hosted = hosted_test_policy();
    hosted.config.supporter.text.burst = 1;
    let caller = Caller {
        identity: "account:team".into(),
        is_supporter: true,
    };
    drop(hosted.admit(&caller, Modality::Text).unwrap());
    let rejected = hosted.admit(&caller, Modality::Text).err().unwrap();
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(!rejected.message().contains("$5"));
    let mut headers = HeaderMap::new();
    rejected.write_error_code(&mut headers);
    assert!(headers.contains_key("retry-after"));
}

/// Idle cleanup cannot replenish an active or recently depleted identity.
#[test]
fn hosted_test_cleanup_does_not_reset_allowance() {
    let mut hosted = hosted_test_policy();
    hosted.config.public.text.burst = 1;
    let caller = Caller {
        identity: "ip:192.0.2.1".into(),
        is_supporter: false,
    };
    let lease = hosted.admit(&caller, Modality::Text).unwrap();
    hosted.cleanup();
    assert!(hosted.admit(&caller, Modality::Text).is_err());
    drop(lease);
    hosted.cleanup();
    assert!(hosted.admit(&caller, Modality::Text).is_err());
}

/// Spoofed ordinary forwarding and wrong ingress credentials cannot choose identity.
#[test]
fn hosted_test_only_verified_ingress_can_supply_identity() {
    let hosted = hosted_test_policy();
    let peer = "127.0.0.1".parse().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("192.0.2.2"));
    assert!(hosted.client_ip(peer, &headers).is_err());
    headers.insert(
        "x-eliza-ingress-token",
        HeaderValue::from_static("test-ingress-secret-at-least-32-bytes"),
    );
    headers.insert(
        "x-eliza-client-ip",
        HeaderValue::from_static("2001:db8:1:2::1234"),
    );
    assert_eq!(
        hosted.client_ip(peer, &headers).unwrap().to_string(),
        "2001:db8:1:2::"
    );
    assert!(
        hosted
            .client_ip("192.0.2.9".parse().unwrap(), &headers)
            .is_err()
    );
}

/// Placeholder SDK credentials never create distinct limiter identities or lookups.
#[tokio::test]
async fn hosted_test_placeholders_and_malformed_keys_do_not_contact_billing() {
    let hosted = hosted_test_policy();
    let ip = "192.0.2.1".parse().unwrap();
    let mut headers = HeaderMap::new();
    for key in ["Bearer ignored", "Bearer random-other-key"] {
        headers.insert("authorization", HeaderValue::from_str(key).unwrap());
        let caller = hosted.caller(ip, &headers, Provider::OpenAi).await.unwrap();
        assert_eq!(caller.identity, "ip:192.0.2.1");
        assert!(!caller.is_supporter);
    }
    headers.insert(
        "authorization",
        HeaderValue::from_static("Bearer eliza_test_bad"),
    );
    assert_eq!(
        hosted
            .caller(ip, &headers, Provider::OpenAi)
            .await
            .err()
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cache = hosted.cache.lock().unwrap();
    let count = cache.entries.len();
    assert_eq!(count, 0);
}

/// Outage grace cannot outlive last confirmation, denial or the paid/key expiry.
#[test]
fn hosted_test_outage_clock_never_extends_eligibility() {
    let mut cached = Cached {
        value: Entitlement {
            is_eligible: true,
            account_id: Some("team".into()),
            eligible_until: Some(u64::MAX),
            refresh_seconds: 60,
            stale_seconds: 300,
        },
        confirmed: Instant::now()
            .checked_sub(Duration::from_secs(359))
            .unwrap(),
        fresh_until: Instant::now()
            .checked_sub(Duration::from_secs(299))
            .unwrap(),
        retry_at: Instant::now(),
    };
    assert!(Caller::try_from(&cached).is_ok());
    cached.confirmed -= Duration::from_secs(2);
    assert_eq!(
        Caller::try_from(&cached).err().unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    cached.value.is_eligible = false;
    assert_eq!(
        Caller::try_from(&cached).err().unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    cached.value.is_eligible = true;
    cached.value.eligible_until = Some(1);
    assert_eq!(
        Caller::try_from(&cached).err().unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
}
