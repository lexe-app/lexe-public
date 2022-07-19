#![allow(dead_code)] // TODO Remove

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};

use warp::{reply, Filter, Rejection, Reply};

use crate::api::{DirectoryId, File, FileId};
use crate::command::test;
use crate::persister;

type FileName = String;
type Data = Vec<u8>;
type Backend = Arc<Mutex<MockBackend>>;

pub struct MockBackend {
    vfs: HashMap<DirectoryId, HashMap<FileName, Data>>,
}

impl MockBackend {
    fn new() -> Self {
        let mut vfs = HashMap::new();

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
        vfs.insert(singleton_dir, HashMap::new());
        vfs.insert(channel_peers_dir, HashMap::new());
        vfs.insert(channel_monitors_dir, HashMap::new());

        Self { vfs }
    }

    fn get(&self, file_id: FileId) -> Option<File> {
        let dir_id = DirectoryId {
            instance_id: file_id.instance_id,
            directory: file_id.directory,
        };
        self.vfs
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
        self.vfs
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
        self.vfs
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
        self.vfs
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

/// Makes the database accessible to subsequent `Filter`s.
fn with_backend(
    backend: Backend,
) -> impl Filter<Extract = (Backend,), Error = Infallible> + Clone {
    warp::any().map(move || backend.clone())
}

/// Mimics the file-based routes offered to the node by the Runner during init
pub fn routes() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone
{
    let v1 = warp::path("v1");

    let backend = Arc::new(Mutex::new(MockBackend::new()));

    let create_file = warp::post()
        .and(with_backend(backend.clone()))
        .and(warp::body::json())
        .then(create_file);
    let get_file = warp::get()
        .and(with_backend(backend.clone()))
        .and(warp::query())
        .then(get_file);
    let upsert_file = warp::put()
        .and(with_backend(backend.clone()))
        .and(warp::body::json())
        .then(upsert_file);
    let delete_file = warp::delete()
        .and(with_backend(backend.clone()))
        .and(warp::body::json())
        .then(delete_file);
    let file = warp::path("file")
        .and(create_file.or(get_file).or(upsert_file).or(delete_file));

    let get_directory = warp::get()
        .and(with_backend(backend))
        .and(warp::query())
        .then(get_directory);
    let directory = warp::path("directory").and(get_directory);

    v1.and(file.or(directory))
}

/// POST /v1/file -> File
async fn create_file(backend: Backend, file: File) -> impl Reply {
    let file_opt = backend.lock().unwrap().insert(file.clone());
    assert!(file_opt.is_none());
    reply::json(&file)
}

/// GET /v1/file -> Option<File>
async fn get_file(backend: Backend, file_id: FileId) -> impl Reply {
    let file_opt = backend.lock().unwrap().get(file_id.clone());
    reply::json(&file_opt)
}

/// PUT /v1/file File -> File
async fn upsert_file(backend: Backend, file: File) -> impl Reply {
    backend.lock().unwrap().insert(file.clone());
    reply::json(&file)
}

/// DELETE /v1/file -> OK
async fn delete_file(backend: Backend, file_id: FileId) -> impl Reply {
    let file_opt = backend.lock().unwrap().remove(file_id);
    assert!(file_opt.is_none());
    String::from("OK")
}

/// GET /v1/directory -> Vec<File>
async fn get_directory(backend: Backend, dir_id: DirectoryId) -> impl Reply {
    let files_vec = backend.lock().unwrap().get_dir(dir_id);
    reply::json(&files_vec)
}
