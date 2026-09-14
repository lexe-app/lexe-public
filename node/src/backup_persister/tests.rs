use std::future::Future;

use futures::poll;
use lexe_api::vfs::{self, VfsFile};
use tokio::sync::oneshot;

use super::*;

#[tokio::test(start_paused = true)]
async fn batch_delay_starts_with_first_update() {
    let (worker, mut test) = test_persister();
    tokio::pin!(worker);
    assert!(poll!(&mut worker).is_pending());

    // Idle time doesn't count toward the next batch's delay.
    time::sleep(Duration::from_secs(90)).await;
    let mut file = VfsFile::new(
        vfs::SINGLETON_DIRECTORY,
        vfs::CHANNEL_MANAGER_FILENAME,
        vec![1],
    );
    test.commands
        .try_send(BackupCommand::Persist(file.clone()))
        .unwrap();
    assert!(poll!(&mut worker).is_pending());
    time::sleep(Duration::from_secs(59)).await;
    assert!(poll!(&mut worker).is_pending());
    assert!(test.requests.try_recv().is_err());

    // A later snapshot replaces the first without restarting the delay.
    file.data = vec![2];
    test.commands
        .try_send(BackupCommand::Persist(file.clone()))
        .unwrap();
    assert!(poll!(&mut worker).is_pending());
    time::sleep(Duration::from_secs(1)).await;
    assert!(poll!(&mut worker).is_pending());
    let request = test.requests.try_recv().unwrap();
    assert_eq!(
        request.files,
        BackupBatch::from([(file.id, Some(file.data))])
    );
    request.response.send(Ok(())).unwrap();
    assert!(poll!(&mut worker).is_pending());

    drop(test.commands);
    assert!(poll!(&mut worker).is_ready());
    assert!(test.requests.try_recv().is_err());
    assert!(!test.shutdown.try_recv());
}

#[tokio::test(start_paused = true)]
async fn shutdown_drains_updates_and_reserved_archive() {
    let (worker, mut test) = test_persister();
    tokio::pin!(worker);
    let mut manager = VfsFile::new(
        vfs::SINGLETON_DIRECTORY,
        vfs::CHANNEL_MANAGER_FILENAME,
        vec![1],
    );
    let monitor = VfsFile::new(vfs::CHANNEL_MONITORS_DIR, "monitor", vec![2]);
    let archive =
        VfsFile::new(vfs::CHANNEL_MONITORS_ARCHIVE_DIR, "monitor", vec![3]);
    test.commands
        .try_send(BackupCommand::Persist(manager.clone()))
        .unwrap();
    manager.data = vec![4];
    test.commands
        .try_send(BackupCommand::Persist(manager.clone()))
        .unwrap();
    test.commands
        .try_send(BackupCommand::Persist(monitor.clone()))
        .unwrap();
    let permit = test.commands.clone().try_reserve_owned().unwrap();

    // Shutdown is ready before the worker has read any commands.
    test.backup_shutdown.send();
    assert!(poll!(&mut worker).is_pending());
    assert!(test.commands.is_closed());
    assert!(test.requests.try_recv().is_err());

    // An archive started before shutdown can still finish its VFS work.
    permit.send(BackupCommand::Archive {
        source_file_id: monitor.id.clone(),
        archive_file: archive.clone(),
    });
    assert!(poll!(&mut worker).is_pending());
    let request = test.requests.try_recv().unwrap();
    assert_eq!(
        request.files,
        BackupBatch::from([
            (manager.id, Some(manager.data)),
            (monitor.id, None),
            (archive.id, Some(archive.data)),
        ])
    );
    request.response.send(Ok(())).unwrap();
    assert!(poll!(&mut worker).is_ready());
    assert!(test.requests.try_recv().is_err());
    assert!(!test.shutdown.try_recv());
}

#[tokio::test(start_paused = true)]
async fn archives_share_batch_and_next_batch_gets_full_delay() {
    let (worker, mut test) = test_persister();
    tokio::pin!(worker);
    let mut manager = VfsFile::new(
        vfs::SINGLETON_DIRECTORY,
        vfs::CHANNEL_MANAGER_FILENAME,
        vec![0],
    );
    test.commands
        .try_send(BackupCommand::Persist(manager.clone()))
        .unwrap();
    assert!(poll!(&mut worker).is_pending());
    time::sleep(Duration::from_secs(59)).await;

    let mut expected = BackupBatch::new();
    for filename in ["monitor1", "monitor2"] {
        let monitor =
            VfsFile::new(vfs::CHANNEL_MONITORS_DIR, filename, vec![1]);
        let archive =
            VfsFile::new(vfs::CHANNEL_MONITORS_ARCHIVE_DIR, filename, vec![2]);
        test.commands
            .try_send(BackupCommand::Persist(monitor.clone()))
            .unwrap();
        test.commands
            .try_send(BackupCommand::Archive {
                source_file_id: monitor.id.clone(),
                archive_file: archive.clone(),
            })
            .unwrap();
        expected.insert(monitor.id, None);
        expected.insert(archive.id, Some(archive.data));
    }

    // Later updates join the batch containing the archives.
    manager.data = vec![3];
    test.commands
        .try_send(BackupCommand::Persist(manager.clone()))
        .unwrap();
    expected.insert(manager.id.clone(), Some(manager.data.clone()));

    // Archives neither flush immediately nor restart the batch's delay.
    assert!(poll!(&mut worker).is_pending());
    assert!(test.requests.try_recv().is_err());
    time::sleep(Duration::from_secs(1)).await;
    assert!(poll!(&mut worker).is_pending());
    let request = test.requests.try_recv().unwrap();
    assert_eq!(request.files, expected);
    request.response.send(Ok(())).unwrap();
    assert!(poll!(&mut worker).is_pending());
    assert!(test.requests.try_recv().is_err());

    // The next batch gets its full delay even after the old timer expires.
    time::sleep(Duration::from_secs(90)).await;
    manager.data = vec![4];
    test.commands
        .try_send(BackupCommand::Persist(manager.clone()))
        .unwrap();
    assert!(poll!(&mut worker).is_pending());
    time::sleep(Duration::from_secs(59)).await;
    assert!(poll!(&mut worker).is_pending());
    assert!(test.requests.try_recv().is_err());
    time::sleep(Duration::from_secs(1)).await;
    assert!(poll!(&mut worker).is_pending());
    let request = test.requests.try_recv().unwrap();
    assert_eq!(
        request.files,
        BackupBatch::from([(manager.id, Some(manager.data))])
    );
    request.response.send(Ok(())).unwrap();
    assert!(poll!(&mut worker).is_pending());

    test.backup_shutdown.send();
    assert!(poll!(&mut worker).is_ready());
    assert!(test.requests.try_recv().is_err());
    assert!(!test.shutdown.try_recv());
}

#[tokio::test(start_paused = true)]
async fn failed_backup_triggers_shutdown() {
    let (worker, mut test) = test_persister();
    tokio::pin!(worker);
    test.commands
        .try_send(BackupCommand::Archive {
            source_file_id: VfsFileId::new(
                vfs::CHANNEL_MONITORS_DIR,
                "monitor",
            ),
            archive_file: VfsFile::new(
                vfs::CHANNEL_MONITORS_ARCHIVE_DIR,
                "monitor",
                vec![1],
            ),
        })
        .unwrap();
    assert!(poll!(&mut worker).is_pending());
    time::sleep(BackupPersister::PERSIST_DELAY).await;
    assert!(poll!(&mut worker).is_pending());
    let request = test.requests.try_recv().unwrap();
    request
        .response
        .send(Err(anyhow::anyhow!("failed")))
        .unwrap();
    assert!(poll!(&mut worker).is_pending());
    assert!(test.shutdown.try_recv());

    test.backup_shutdown.send();
    assert!(poll!(&mut worker).is_ready());
    assert!(test.requests.try_recv().is_err());
}

struct TestContext {
    backup_shutdown: NotifyOnce,
    commands: mpsc::Sender<BackupCommand>,
    requests: mpsc::UnboundedReceiver<TestRequest>,
    shutdown: NotifyOnce,
}

fn test_persister() -> (impl Future<Output = ()>, TestContext) {
    let (requests_tx, requests) = mpsc::unbounded_channel();
    let (persister, commands) =
        BackupPersister::new(BackupStore::Test(TestStore(requests_tx)));
    let test = TestContext {
        backup_shutdown: NotifyOnce::new(),
        commands,
        requests,
        shutdown: NotifyOnce::new(),
    };
    let worker =
        persister.run(test.backup_shutdown.clone(), test.shutdown.clone());
    (worker, test)
}

pub(crate) struct TestStore(mpsc::UnboundedSender<TestRequest>);

#[derive(Debug)]
struct TestRequest {
    files: BackupBatch,
    response: oneshot::Sender<anyhow::Result<()>>,
}

impl TestStore {
    // Polling the worker captures a batch; the test controls its completion.
    pub(super) async fn persist(
        &self,
        files: &mut BackupBatch,
    ) -> anyhow::Result<()> {
        // Keep the batch allocation, like the production stores.
        #[allow(clippy::drain_collect)]
        let files = files.drain().collect::<BackupBatch>();
        let (response, response_rx) = oneshot::channel();
        self.0.send(TestRequest { files, response }).unwrap();
        response_rx.await.unwrap()
    }
}
