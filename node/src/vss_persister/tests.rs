use std::{collections::BTreeMap, env};

use lexe_api::vfs::{self, VfsFileId};
use lexe_crypto::rng::FastRng;
use lexe_vss_client::{GetObjectRequest, VssError};

use super::*;

#[test]
fn batch_to_request() {
    let manager =
        VfsFileId::new(vfs::SINGLETON_DIRECTORY, vfs::CHANNEL_MANAGER_FILENAME);
    let monitor = VfsFileId::new(vfs::CHANNEL_MONITORS_DIR, "monitor");
    let archive = VfsFileId::new(vfs::CHANNEL_MONITORS_ARCHIVE_DIR, "monitor");
    let mut files = BackupBatch::from([
        (manager.clone(), Some(vec![1])),
        (monitor.clone(), None),
        (archive.clone(), Some(Vec::new())),
    ]);
    let request = VssPersister::build_request(&mut files);
    assert!(files.is_empty());
    assert_eq!(request.store_id, constants::VSS_STORE_ID);
    assert_eq!(request.global_version, None);
    let mut puts = request.transaction_items;
    puts.sort_unstable_by(|a, b| a.key.cmp(&b.key));
    let mut expected = vec![
        KeyValue {
            key: manager.to_string(),
            version: NO_VERSION_CHECK,
            value: vec![1],
        },
        KeyValue {
            key: archive.to_string(),
            version: NO_VERSION_CHECK,
            value: Vec::new(),
        },
    ];
    expected.sort_by(|a, b| a.key.cmp(&b.key));
    assert_eq!(puts, expected);
    assert_eq!(
        request.delete_items,
        vec![KeyValue {
            key: monitor.to_string(),
            version: NO_VERSION_CHECK,
            value: Vec::new(),
        }]
    );
}

#[test]
fn lexe_tls_hosts() {
    for (host, is_lexe, is_loopback) in [
        ("lexe.app", true, false),
        ("vss.lexe.app", true, false),
        ("vss.lexe.app.", true, false),
        ("vss.lx", true, false),
        ("backend.prod.lx.", true, false),
        ("evillexe.app", false, false),
        ("lexe.app.example", false, false),
        ("vss.lx.example", false, false),
        ("vss.example", false, false),
        ("localhost.", false, true),
        ("127.0.0.1", false, true),
        ("[::1]", false, true),
        ("192.0.2.1", false, false),
    ] {
        for env in [DeployEnv::Dev, DeployEnv::Staging, DeployEnv::Prod] {
            assert_eq!(
                VssProvider::uses_lexe_tls(host, env),
                is_lexe || (env.is_dev() && is_loopback),
                "{host}: {env}"
            );
        }
    }
}

// ```bash
// $ cargo test -p node --lib -- --ignored zeus
// ```
#[ignore = "Makes requests to Zeus VSS"]
#[tokio::test]
async fn zeus() {
    let headers = BTreeMap::new();
    provider_roundtrip("zeus", "https://vss.zeusln.com/vss", headers).await;
}

// ```bash
// $ cargo test -p node --lib -- --ignored mineracks
// ```
#[ignore = "Makes requests to Mineracks VSS"]
#[tokio::test]
async fn mineracks() {
    let headers = BTreeMap::new();
    provider_roundtrip("mineracks", "https://vss.mineracks.com/vss", headers)
        .await;
}

// ```bash
// $ export Z2B_VSS_WRITE_SECRET="$( \
//     just secretctl read nix/secrets/staging/vss-headers.json.age \
//     | jq -r '.z2b."X-VSS-Write-Secret"' \
//   )"
// $ cargo test -p node --lib -- --ignored z2b
// ```
#[ignore = "Requires Z2B_VSS_WRITE_SECRET and network access"]
#[tokio::test]
async fn z2b() {
    let secret = env::var("Z2B_VSS_WRITE_SECRET").unwrap_or_else(|_| {
        panic!("Set Z2B_VSS_WRITE_SECRET to the provider's write token")
    });
    let headers = BTreeMap::from([("X-VSS-Write-Secret".to_owned(), secret)]);
    provider_roundtrip("z2b", "https://vss.sms4sats.com/vss", headers).await;
}

async fn provider_roundtrip(
    name: &str,
    url: &str,
    headers: BTreeMap<String, String>,
) {
    // Keep the seed fixed to reuse the same upstream account.
    let mut rng = FastRng::from_u64(20260924);
    let root_seed = RootSeed::from_rng(&mut rng);
    let config = VssProviderConfig {
        name: name.to_owned(),
        url: url.to_owned(),
        headers,
    };
    let provider = VssProvider::new(&mut rng, &config, DeployEnv::Dev).unwrap();
    let persister = VssPersister::new(&root_seed, &provider);
    let id = VfsFileId::new("vss-test", "backup");
    let request = GetObjectRequest {
        store_id: constants::VSS_STORE_ID.to_owned(),
        key: id.to_string(),
    };

    // Create and overwrite a backup, reading it back after each persist.
    for data in [b"first backup".to_vec(), b"updated backup".to_vec()] {
        let mut files = BackupBatch::from([(id.clone(), Some(data.clone()))]);
        persister.persist(&mut files).await.unwrap();
        let stored = persister
            .client
            .get_object(&request)
            .await
            .unwrap()
            .value
            .unwrap();
        assert_eq!(stored.key, request.key);
        assert_eq!(stored.value, data);
    }

    // Remove the test backup through the same persistence path.
    let mut files = BackupBatch::from([(id, None)]);
    persister.persist(&mut files).await.unwrap();
    let result = persister.client.get_object(&request).await;
    assert!(
        matches!(result, Err(VssError::NoSuchKeyError(_))),
        "Expected deleted backup to be absent: {result:?}"
    );
}
