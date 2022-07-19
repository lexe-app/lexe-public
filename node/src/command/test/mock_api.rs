use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::api::{
    ApiClient, ApiError, DirectoryId, Enclave, File, FileId, Instance, Node,
    NodeInstanceEnclave, UserPort,
};
use crate::command::test;
use crate::persister;
use crate::types::UserId;

type FileName = String;
type Data = Vec<u8>;

pub struct MockApiClient {
    vfs: Mutex<VirtualFileSystem>,
}

impl MockApiClient {
    // We add these unnecessary parameters so that the API exactly matches that
    // of LexeApiClient::new().
    pub fn new() -> Self {
        let vfs = Mutex::new(VirtualFileSystem::new());
        Self { vfs }
    }
}

#[async_trait]
impl ApiClient for MockApiClient {
    /// Always return the dummy version
    async fn get_node(
        &self,
        _user_id: UserId,
    ) -> Result<Option<Node>, ApiError> {
        let node = Node {
            public_key: test::PUBKEY.into(),
            user_id: test::USER_ID.into(),
        };
        Ok(Some(node))
    }

    /// Always return the dummy version
    async fn get_instance(
        &self,
        _user_id: UserId,
        _measurement: String,
    ) -> Result<Option<Instance>, ApiError> {
        let instance = Instance {
            id: test::instance_id(),
            measurement: test::MEASUREMENT.into(),
            node_public_key: test::PUBKEY.into(),
        };
        Ok(Some(instance))
    }

    /// Always return the dummy version
    async fn get_enclave(
        &self,
        _user_id: UserId,
        _measurement: String,
    ) -> Result<Option<Enclave>, ApiError> {
        let enclave = Enclave {
            id: test::enclave_id(),
            seed: test::seed(),
            instance_id: test::instance_id(),
        };
        Ok(Some(enclave))
    }

    async fn create_node_instance_enclave(
        &self,
        req: NodeInstanceEnclave,
    ) -> Result<NodeInstanceEnclave, ApiError> {
        Ok(req)
    }

    async fn get_file(
        &self,
        file_id: FileId,
    ) -> Result<Option<File>, ApiError> {
        let file_opt = self.vfs.lock().await.get(file_id.clone());
        Ok(file_opt)
    }

    async fn create_file(&self, file: File) -> Result<File, ApiError> {
        let file_opt = self.vfs.lock().await.insert(file.clone());
        assert!(file_opt.is_none());
        Ok(file)
    }

    async fn upsert_file(&self, file: File) -> Result<File, ApiError> {
        self.vfs.lock().await.insert(file.clone());
        Ok(file)
    }

    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: FileId) -> Result<String, ApiError> {
        let file_opt = self.vfs.lock().await.remove(file_id);
        assert!(file_opt.is_none());
        Ok(String::from("OK"))
    }

    async fn get_directory(
        &self,
        dir_id: DirectoryId,
    ) -> Result<Vec<File>, ApiError> {
        let files_vec = self.vfs.lock().await.get_dir(dir_id);
        Ok(files_vec)
    }

    async fn notify_runner(
        &self,
        user_port: UserPort,
    ) -> Result<UserPort, ApiError> {
        Ok(user_port)
    }
}

struct VirtualFileSystem {
    inner: HashMap<DirectoryId, HashMap<FileName, Data>>,
}

impl VirtualFileSystem {
    fn new() -> Self {
        let mut inner = HashMap::new();

        // Insert all directories used by the persister
        let singleton_dir = DirectoryId {
            instance_id: test::instance_id(),
            directory: persister::SINGLETON_DIRECTORY.into(),
        };
        let channel_peers_dir = DirectoryId {
            instance_id: test::instance_id(),
            directory: persister::CHANNEL_PEERS_DIRECTORY.into(),
        };
        let channel_monitors_dir = DirectoryId {
            instance_id: test::instance_id(),
            directory: persister::CHANNEL_MONITORS_DIRECTORY.into(),
        };
        inner.insert(singleton_dir, HashMap::new());
        inner.insert(channel_peers_dir, HashMap::new());
        inner.insert(channel_monitors_dir, HashMap::new());

        Self { inner }
    }

    fn get(&self, file_id: FileId) -> Option<File> {
        let dir_id = DirectoryId {
            instance_id: file_id.instance_id,
            directory: file_id.directory,
        };
        self.inner
            .get(&dir_id)
            .expect("Missing directory")
            .get(&file_id.name)
            .map(|data| File {
                instance_id: dir_id.instance_id,
                directory: dir_id.directory,
                name: file_id.name,
                data: data.clone(),
            })
    }

    fn insert(&mut self, file: File) -> Option<File> {
        let dir_id = DirectoryId {
            instance_id: file.instance_id,
            directory: file.directory,
        };
        self.inner
            .get_mut(&dir_id)
            .expect("Missing directory")
            .insert(file.name.clone(), file.data)
            .map(|data| File {
                instance_id: dir_id.instance_id,
                directory: dir_id.directory,
                name: file.name,
                data,
            })
    }

    fn remove(&mut self, file_id: FileId) -> Option<File> {
        let dir_id = DirectoryId {
            instance_id: file_id.instance_id,
            directory: file_id.directory,
        };
        self.inner
            .get_mut(&dir_id)
            .expect("Missing directory")
            .remove(&file_id.name)
            .map(|data| File {
                instance_id: dir_id.instance_id,
                directory: dir_id.directory,
                name: file_id.name,
                data,
            })
    }

    fn get_dir(&self, dir_id: DirectoryId) -> Vec<File> {
        self.inner
            .get(&dir_id)
            .expect("Missing directory")
            .iter()
            .map(|(name, data)| File {
                instance_id: dir_id.instance_id.clone(),
                directory: dir_id.directory.clone(),
                name: name.clone(),
                data: data.clone(),
            })
            .collect()
    }
}
