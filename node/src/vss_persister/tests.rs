use lexe_api::vfs::{self, VfsFileId};

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
    assert_eq!(request.store_id, "lexe");
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
