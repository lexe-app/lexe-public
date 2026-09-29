//! End-to-end flows: the REQUESTER builds a request, the WALLET responds, the
//! response is delivered, and the REQUESTER accepts it.

use std::{
    collections::BTreeSet,
    fs,
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
    routing::post,
};
use lexe_api::server::{LayerConfig, extract::LxQuery};
use lexe_byte_array::ByteArray;
use lexe_common::{env::DeployEnv, net, time::TimestampMs};
use lexe_connect::{
    LEXE_MAILBOX_URL,
    http::LexeConnectClient,
    mailbox::{Address, MailboxStore, PutError},
    request::{
        CredentialRequest, CredentialRequestParams, Delivery, OneTimeSecret,
        RequestError,
    },
    requester::{AcceptError, PendingRequest},
    response::{CredentialResponse, CredentialResult, ErrorCode, Grant},
    wallet::{DeliveryAction, Outcome, RequesterDisplay},
};
use lexe_crypto::rng::SysRng;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use serde::Deserialize;
use tracing::info_span;

/// A real base64 `ClientCredentials` blob, as the WALLET would issue. Read at
/// runtime so `test_data/` can stay out of the published crate.
static CREDENTIAL: LazyLock<String> = LazyLock::new(|| {
    fs::read_to_string("test_data/client_credentials.b64").unwrap()
});

fn params(delivery: Delivery) -> CredentialRequestParams {
    CredentialRequestParams {
        delivery,
        account: Some("@janedoe".into()),
        metadata: Some("order-42".into()),
        scopes: ["read_info", "receive"].map(String::from).into(),
        permissions: BTreeSet::new(),
        label: Some("BillSplit".into()),
        expires_at: None,
    }
}

fn approved() -> Outcome {
    Outcome::Approved {
        credential: CREDENTIAL.clone(),
        expires_at: None,
    }
}

fn assert_granted(response: &CredentialResponse) {
    match &response.result {
        CredentialResult::Granted(Grant { credential, .. }) =>
            assert_eq!(credential, &*CREDENTIAL),
        other => panic!("Expected a grant, got {other:?}"),
    }
    assert_eq!(response.account.as_deref(), Some("@janedoe"));
    assert_eq!(response.metadata.as_deref(), Some("order-42"));
}

/// A local server standing in for both a REQUESTER's `post_url` and a
/// mailbox. Tests point deliveries at it with [`TestServer::localize`].
struct TestServer {
    base: String,
    captured: Captured,
    _task: LxTask<()>,
}

/// The last `post_url` delivery received: its content type and body.
type Captured = Arc<Mutex<Option<(String, Vec<u8>)>>>;

struct AppState {
    captured: Captured,
    mailbox: MailboxStore,
}

#[derive(Deserialize)]
struct AddressQuery {
    address: Address,
}

impl TestServer {
    async fn spawn() -> Self {
        let captured = Captured::default();
        let state = Arc::new(AppState {
            captured: captured.clone(),
            mailbox: MailboxStore::new().with_ttl(Duration::from_secs(60)),
        });
        let app = Router::new()
            .route("/lexe", post(Self::capture))
            .route("/mailbox", post(Self::mailbox_put).get(Self::mailbox_get))
            .with_state(state);

        const SPAN_NAME: &str = "(lexe-connect-test-server)";
        let (task, base) = lexe_api::server::spawn_server_task(
            net::LOCALHOST_WITH_EPHEMERAL_PORT,
            app,
            LayerConfig::default(),
            None,
            SPAN_NAME.into(),
            info_span!(parent: None, SPAN_NAME),
            NotifyOnce::new(),
        )
        .unwrap();

        Self {
            base,
            captured,
            _task: task,
        }
    }

    /// Points an `https://<host>/<path>` url at this server instead.
    fn localize(&self, url: &str) -> String {
        let after_scheme = &url[url.find("://").unwrap() + 3..];
        let path = &after_scheme[after_scheme.find('/').unwrap()..];
        format!("{}{path}", self.base)
    }

    /// Takes the last `post_url` delivery: its content type and body.
    fn take_captured(&self) -> (String, Vec<u8>) {
        self.captured.lock().unwrap().take().unwrap()
    }

    async fn capture(
        State(state): State<Arc<AppState>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> StatusCode {
        let content_type = headers[CONTENT_TYPE].to_str().unwrap().to_owned();
        *state.captured.lock().unwrap() = Some((content_type, body.to_vec()));
        StatusCode::OK
    }

    async fn mailbox_put(
        State(state): State<Arc<AppState>>,
        LxQuery(query): LxQuery<AddressQuery>,
        body: Bytes,
    ) -> StatusCode {
        match state
            .mailbox
            .put(query.address, body.to_vec(), Instant::now())
        {
            Ok(()) => StatusCode::OK,
            Err(PutError::Occupied) => StatusCode::CONFLICT,
            Err(PutError::TooLarge) => StatusCode::PAYLOAD_TOO_LARGE,
            Err(PutError::Full) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    async fn mailbox_get(
        State(state): State<Arc<AppState>>,
        LxQuery(query): LxQuery<AddressQuery>,
    ) -> axum::response::Response {
        match state.mailbox.get(&query.address, Instant::now()) {
            Some(blob) => (StatusCode::OK, blob).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
}

// --- Tests --- //

#[test]
fn redirect_flow() {
    let mut rng = SysRng::new();
    let delivery = Delivery::Redirect("https://billsplit.com/cb?e=1".into());
    let pending = PendingRequest::new(&mut rng, params(delivery)).unwrap();

    let request =
        CredentialRequest::parse(&pending.connection_string()).unwrap();
    assert_eq!(&request, pending.request());
    assert_eq!(
        request.requester_display(),
        RequesterDisplay::Verified {
            domain: "billsplit.com".into()
        }
    );

    let DeliveryAction::Redirect(url) =
        request.respond(&mut rng, approved()).unwrap()
    else {
        panic!("Expected a redirect");
    };
    assert!(
        url.starts_with("https://billsplit.com/cb?e=1&response="),
        "{url}"
    );
    assert_granted(&pending.accept_redirect(&url).unwrap());

    let DeliveryAction::Redirect(url) =
        request.respond(&mut rng, Outcome::Rejected).unwrap()
    else {
        panic!("Expected a redirect");
    };
    let response = pending.accept_redirect(&url).unwrap();
    match response.result {
        CredentialResult::Error(err) =>
            assert_eq!(err.code, ErrorCode::UserRejected),
        other => panic!("Expected an error, got {other:?}"),
    }
}

#[tokio::test]
async fn post_flows() {
    let mut rng = SysRng::new();
    let server = TestServer::spawn().await;
    let client = LexeConnectClient::new(DeployEnv::Dev).unwrap();

    for plaintext in [true, false] {
        let delivery = Delivery::Post("https://paygate.com/lexe?e=1".into());
        let pending = if plaintext {
            PendingRequest::new_plaintext(&mut rng, params(delivery))
        } else {
            PendingRequest::new(&mut rng, params(delivery))
        }
        .unwrap();

        let request =
            CredentialRequest::parse(&pending.connection_string()).unwrap();
        assert_eq!(
            request.requester_display(),
            RequesterDisplay::Verified {
                domain: "paygate.com".into()
            }
        );

        let DeliveryAction::Http(mut delivery) =
            request.respond(&mut rng, approved()).unwrap()
        else {
            panic!("Expected an http delivery");
        };
        assert_eq!(delivery.url, "https://paygate.com/lexe?e=1");
        let expected_content_type = if plaintext {
            "application/json"
        } else {
            "application/octet-stream"
        };
        assert_eq!(delivery.content_type, expected_content_type);

        delivery.url = server.localize(&delivery.url);
        client.deliver(&delivery).await.unwrap();
        let (content_type, body) = server.take_captured();
        assert_eq!(content_type, expected_content_type);
        assert_granted(&pending.accept_body(&body).unwrap());
    }
}

#[tokio::test]
async fn mailbox_flow() {
    let mut rng = SysRng::new();
    let server = TestServer::spawn().await;
    let client = LexeConnectClient::new(DeployEnv::Dev).unwrap();

    let delivery = Delivery::Mailbox("https://mailbox.test/mailbox".into());
    let pending = PendingRequest::new(&mut rng, params(delivery)).unwrap();
    let (mailbox_url, address) = pending.mailbox().unwrap();
    let local_mailbox_url = server.localize(mailbox_url);

    let request =
        CredentialRequest::parse(&pending.connection_string()).unwrap();
    assert_eq!(
        request.requester_display(),
        RequesterDisplay::Unverified { scheme_host: None }
    );
    assert_eq!(request.mailbox_address(), address);

    let DeliveryAction::Http(mut delivery) =
        request.respond(&mut rng, approved()).unwrap()
    else {
        panic!("Expected an http delivery");
    };
    assert_eq!(
        delivery.url,
        format!("https://mailbox.test/mailbox?address={address}")
    );
    delivery.url = server.localize(&delivery.url);

    let other = Address::from_array([7; 32]);
    assert!(
        client
            .mailbox_get(&local_mailbox_url, &other)
            .await
            .unwrap()
            .is_none()
    );

    client.deliver(&delivery).await.unwrap();
    // A byte-identical retry succeeds; any other body conflicts.
    client.deliver(&delivery).await.unwrap();
    let mut other = delivery.clone();
    other.body.push(0);
    let occupied = client.deliver(&other).await.unwrap_err();
    assert!(occupied.to_string().contains("409"));

    let blob = client
        .mailbox_poll(
            &local_mailbox_url,
            &address,
            Duration::from_millis(10),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_granted(&pending.accept_body(&blob.0).unwrap());
    // Blobs survive reads.
    assert!(
        client
            .mailbox_get(&local_mailbox_url, &address)
            .await
            .unwrap()
            .is_some()
    );
}

#[test]
fn forged_and_mismatched_responses_are_rejected() {
    let mut rng = SysRng::new();
    let delivery = Delivery::Redirect("https://billsplit.com/cb".into());
    let pending = PendingRequest::new(&mut rng, params(delivery)).unwrap();
    let request = pending.request().clone();
    let response = |result: CredentialResult| CredentialResponse {
        result,
        one_time_secret: request.one_time_secret,
        account: request.params.account.clone(),
        metadata: None,
    };
    let grant = |scopes: BTreeSet<String>| {
        CredentialResult::Granted(Grant {
            credential: CREDENTIAL.clone(),
            scopes,
            permissions: BTreeSet::new(),
            expires_at: None,
        })
    };
    let accept = |response: &CredentialResponse,
                  sealed_to: &CredentialRequest| {
        let blob = response
            .clone()
            .seal(&mut SysRng::new(), sealed_to)
            .unwrap();
        pending.accept_body(&blob.0).unwrap_err()
    };

    // A forwarded request with the account rewritten yields a response the
    // real REQUESTER can't decrypt.
    let mut rewritten = request.clone();
    rewritten.params.account = Some("@victim".into());
    let DeliveryAction::Redirect(url) =
        rewritten.respond(&mut rng, approved()).unwrap()
    else {
        panic!("Expected a redirect");
    };
    assert!(matches!(
        pending.accept_redirect(&url),
        Err(AcceptError::Decrypt(_))
    ));

    // A response sealed under a different request's key.
    let other =
        PendingRequest::new(&mut rng, params(request.params.delivery.clone()))
            .unwrap();
    let mut sealed_to_other = request.clone();
    sealed_to_other.ephemeral_hpke_pubkey =
        other.request().ephemeral_hpke_pubkey;
    let ok = response(grant(request.params.scopes.clone()));
    assert!(matches!(
        accept(&ok, &sealed_to_other),
        AcceptError::Decrypt(_)
    ));

    // Correctly sealed, but the echoed fields or grant don't match.
    let mut wrong_secret = ok.clone();
    wrong_secret.one_time_secret = OneTimeSecret::from_rng(&mut rng);
    assert!(matches!(
        accept(&wrong_secret, &request),
        AcceptError::SecretMismatch
    ));
    let mut wrong_account = ok.clone();
    wrong_account.account = None;
    assert!(matches!(
        accept(&wrong_account, &request),
        AcceptError::AccountMismatch
    ));
    let extra_scope =
        ["read_info", "receive", "spend"].map(String::from).into();
    assert!(matches!(
        accept(&response(grant(extra_scope)), &request),
        AcceptError::GrantMismatch
    ));

    // Malformed redirects.
    assert!(matches!(
        pending.accept_redirect("https://billsplit.com/cb"),
        Err(AcceptError::MissingResponse)
    ));
    assert!(matches!(
        pending.accept_redirect("https://billsplit.com/cb?response=!!"),
        Err(AcceptError::Base64(_))
    ));
    assert!(matches!(
        pending.accept_redirect("https://billsplit.com/cb?response=AAAA"),
        Err(AcceptError::Decrypt(_))
    ));
}

/// A grant carrying a real Lexe credential with every echoed field at its
/// limit must fit in the mailbox.
#[test]
fn largest_response_fits() {
    let mut rng = SysRng::new();
    let mut params = params(Delivery::Mailbox(LEXE_MAILBOX_URL.into()));
    params.account = Some("a".repeat(64));
    params.metadata = Some("m".repeat(1024));
    let pending = PendingRequest::new(&mut rng, params).unwrap();
    let request = pending.request();
    let response = CredentialResponse {
        result: CredentialResult::Granted(Grant {
            credential: CREDENTIAL.clone(),
            scopes: ["read_info", "read_payments", "receive", "spend"]
                .map(String::from)
                .into(),
            permissions: ["cancel_payment".to_owned()].into(),
            expires_at: Some(TimestampMs::from_millis(1821484800000).unwrap()),
        }),
        one_time_secret: request.one_time_secret,
        account: request.params.account.clone(),
        metadata: request.params.metadata.clone(),
    };
    let blob = response.seal(&mut rng, request).unwrap();
    assert!(blob.0.len() <= MailboxStore::DEFAULT_MAX_BLOB_LEN);
}

#[test]
fn invalid_requests_are_rejected() {
    let mut rng = SysRng::new();
    let redirect = Delivery::Redirect("https://billsplit.com/cb".into());
    assert_eq!(
        PendingRequest::new_plaintext(&mut rng, params(redirect)).unwrap_err(),
        RequestError::Missing("ephemeral_hpke_pubkey")
    );
    let http_post = Delivery::Post("http://paygate.com/lexe".into());
    assert!(matches!(
        PendingRequest::new(&mut rng, params(http_post)).unwrap_err(),
        RequestError::Invalid("post_url", _)
    ));
    let mut too_long = params(Delivery::Mailbox("https://m.test/x".into()));
    too_long.metadata = Some("x".repeat(1025));
    assert!(matches!(
        PendingRequest::new(&mut rng, too_long).unwrap_err(),
        RequestError::Invalid("metadata", _)
    ));
}
