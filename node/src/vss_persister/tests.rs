use axum::{Router, body::Bytes, routing::post};
use lexe_api::{
    server::{self, LayerConfig},
    vfs::{self, VfsFile, VfsFileId},
};
use lexe_common::net;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lexe_vss_client::prost::Message;
use tokio::sync::mpsc;
use tracing::info_span;

use super::*;

#[tokio::test]
async fn persist_puts_and_deletes() {
    let (persister, mut requests, server) = test_persister("0");
    let manager = VfsFile::new(
        vfs::SINGLETON_DIRECTORY,
        vfs::CHANNEL_MANAGER_FILENAME,
        vec![1],
    );
    let monitor = VfsFileId::new(vfs::CHANNEL_MONITORS_DIR, "monitor");
    let archive =
        VfsFile::new(vfs::CHANNEL_MONITORS_ARCHIVE_DIR, "monitor", Vec::new());
    let mut files = BackupBatch::from([
        (manager.id.clone(), Some(manager.data.clone())),
        (monitor.clone(), None),
        (archive.id.clone(), Some(archive.data.clone())),
    ]);
    let capacity = files.capacity();
    persister.persist(&mut files).await.unwrap();
    assert!(files.is_empty());
    assert_eq!(files.capacity(), capacity);

    let request = requests.try_recv().unwrap();
    assert_eq!(request.store_id, "lexe");
    assert_eq!(request.global_version, None);
    let mut puts = request.transaction_items;
    puts.sort_by(|a, b| a.key.cmp(&b.key));
    let mut expected = vec![kv(archive), kv(manager)];
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
    assert!(requests.try_recv().is_err());
    server.abort();
}

#[tokio::test]
async fn persist_propagates_errors() {
    let (persister, mut requests, server) = test_persister("unsupported");
    let file_id =
        VfsFileId::new(vfs::SINGLETON_DIRECTORY, vfs::CHANNEL_MANAGER_FILENAME);
    let mut files = BackupBatch::from([(file_id, Some(vec![1]))]);
    let capacity = files.capacity();
    persister.persist(&mut files).await.unwrap_err();
    assert!(files.is_empty());
    assert_eq!(files.capacity(), capacity);
    assert!(requests.try_recv().is_ok());
    server.abort();
}

fn kv(file: VfsFile) -> KeyValue {
    KeyValue {
        key: file.id.to_string(),
        version: NO_VERSION_CHECK,
        value: file.data,
    }
}

fn test_persister(
    version: &'static str,
) -> (VssPersister, mpsc::Receiver<PutObjectRequest>, LxTask<()>) {
    let (requests_tx, requests_rx) = mpsc::channel(8);
    let router = Router::new().route(
        "/vss/putObjects",
        post(move |body: Bytes| {
            let tx = requests_tx.clone();
            async move {
                let request = PutObjectRequest::decode(body).unwrap();
                tx.send(request).await.unwrap();
                ([("vss-protocol-version", version)], Bytes::new())
            }
        }),
    );
    let (server, url) = server::spawn_server_task(
        net::LOCALHOST_WITH_EPHEMERAL_PORT,
        router,
        LayerConfig::default(),
        None,
        "(vss-test)".into(),
        info_span!("(vss-test)"),
        NotifyOnce::new(),
    )
    .unwrap();
    let client = VssClient::from_client(
        &format!("{url}/vss"),
        reqwest::Client::builder().no_proxy().build().unwrap(),
        RootSeed::from_u64(1).derive_vss_auth_key(),
    );
    (VssPersister { client }, requests_rx, server)
}
