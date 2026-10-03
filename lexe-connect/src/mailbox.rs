//! The mailbox address and an in-memory mailbox store.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use lexe_byte_array::ByteArray;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum PutError {
    /// The address holds a different blob.
    Occupied,
    /// The blob exceeds the store's max blob length.
    TooLarge,
    /// The store is at capacity with unexpired blobs.
    Full,
}

/// Where a response blob is deposited in a mailbox:
/// `SHA256(DOMAIN_SEPARATOR || one_time_secret)`, i.e. the hash of the
/// request's HPKE `info`, rendered as 64 lowercase hex characters.
/// Only the holders of the connection string can compute it.
//
// Spec: `mailbox_url` delivery.
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct Address([u8; 32]);

lexe_byte_array::impl_byte_array!(Address, 32);
lexe_byte_array::impl_fromstr_fromhex!(Address, 32);
lexe_byte_array::impl_debug_display_as_hex!(Address);
lexe_serde::impl_serde_hexstr_or_bytes!(Address);

/// An in-memory mailbox: the first blob written to an address wins, and
/// blobs expire after a TTL. Callers pass `now` so expiration is testable.
//
// Spec: `mailbox_url` delivery.
pub struct MailboxStore {
    blobs: Mutex<HashMap<Address, StoredBlob>>,
    max_blob_len: usize,
    max_blobs: usize,
    ttl: Duration,
}

struct StoredBlob {
    blob: Vec<u8>,
    expires_at: Instant,
}

impl MailboxStore {
    /// Lexe's public mailbox TTL.
    //
    // Spec: `mailbox_url` delivery.
    pub const DEFAULT_TTL: Duration = Duration::from_secs(5 * 60);
    /// A sealed response echoing max-length `account` and `metadata` around
    /// a real Lexe credential is ~4.3 KiB; see `largest_response_fits`.
    pub const DEFAULT_MAX_BLOB_LEN: usize = 8192;
    /// Bounds memory at roughly `DEFAULT_MAX_BLOBS * DEFAULT_MAX_BLOB_LEN`,
    /// ~4 MiB. Each blob lives for `DEFAULT_TTL`, so this also caps
    /// sustained throughput at `DEFAULT_MAX_BLOBS / DEFAULT_TTL`, ~1.7
    /// responses per second.
    pub const DEFAULT_MAX_BLOBS: usize = 512;

    /// A store with the `DEFAULT_*` limits.
    pub fn new() -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
            max_blob_len: Self::DEFAULT_MAX_BLOB_LEN,
            max_blobs: Self::DEFAULT_MAX_BLOBS,
            ttl: Self::DEFAULT_TTL,
        }
    }

    /// The largest blob [`put`](Self::put) accepts.
    pub fn with_max_blob_len(mut self, max_blob_len: usize) -> Self {
        self.max_blob_len = max_blob_len;
        self
    }

    /// The most blobs held at once; bounds memory at roughly
    /// `max_blobs * max_blob_len`.
    pub fn with_max_blobs(mut self, max_blobs: usize) -> Self {
        self.max_blobs = max_blobs;
        self
    }

    /// How long a blob is held after it is [`put`](Self::put).
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// The blob at `address`, if one is present and unexpired. Blobs
    /// survive reads until they expire, so REQUESTERs can retry.
    pub fn get(&self, address: &Address, now: Instant) -> Option<Vec<u8>> {
        let mut locked_blobs = self.blobs.lock().unwrap();
        let stored = locked_blobs.get(address)?;

        if stored.expires_at <= now {
            locked_blobs.remove(address);
            return None;
        }

        Some(stored.blob.clone())
    }

    pub fn put(
        &self,
        address: Address,
        blob: Vec<u8>,
        now: Instant,
    ) -> Result<(), PutError> {
        if blob.len() > self.max_blob_len {
            return Err(PutError::TooLarge);
        }

        let mut locked_blobs = self.blobs.lock().unwrap();
        if locked_blobs.len() >= self.max_blobs {
            locked_blobs.retain(|_, stored| stored.expires_at > now);
            if locked_blobs.len() >= self.max_blobs {
                return Err(PutError::Full);
            }
        }

        // A retry of the same delivery succeeds without extending the TTL.
        if let Some(stored) = locked_blobs.get(&address)
            && stored.expires_at > now
        {
            return if stored.blob == blob {
                Ok(())
            } else {
                Err(PutError::Occupied)
            };
        }

        locked_blobs.insert(
            address,
            StoredBlob {
                blob,
                expires_at: now + self.ttl,
            },
        );

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use lexe_byte_array::ByteArray;

    use super::*;

    #[test]
    fn first_write_wins_retries_then_expires() {
        let store = MailboxStore::new().with_ttl(Duration::from_secs(10));
        let a = Address::from_array([1; 32]);
        let t0 = Instant::now();

        assert_eq!(store.get(&a, t0), None);
        assert_eq!(store.put(a, vec![1], t0), Ok(()));
        assert_eq!(store.put(a, vec![2], t0), Err(PutError::Occupied));
        assert_eq!(store.get(&a, t0), Some(vec![1]));
        let t_retry = t0 + Duration::from_secs(9);
        assert_eq!(store.put(a, vec![1], t_retry), Ok(()));
        assert_eq!(store.get(&a, t_retry), Some(vec![1]));

        // The retry did not extend the TTL.
        let t1 = t0 + Duration::from_secs(10);
        assert_eq!(store.get(&a, t1), None);
        assert_eq!(store.put(a, vec![2], t1), Ok(()));
        assert_eq!(store.get(&a, t1), Some(vec![2]));

        let too_large = vec![0; MailboxStore::DEFAULT_MAX_BLOB_LEN + 1];
        let b = Address::from_array([2; 32]);
        assert_eq!(store.put(b, too_large, t1), Err(PutError::TooLarge));
    }

    #[test]
    fn full_store_sweeps_expired() {
        const MAX_BLOBS: usize = 4;
        let store = MailboxStore::new()
            .with_ttl(Duration::from_secs(1))
            .with_max_blobs(MAX_BLOBS);
        let t0 = Instant::now();
        for i in 0..MAX_BLOBS {
            let mut bytes = [0u8; 32];
            bytes[..8].copy_from_slice(&(i as u64).to_le_bytes());
            store.put(Address::from_array(bytes), vec![], t0).unwrap();
        }
        let a = Address::from_array([0xff; 32]);
        assert_eq!(store.put(a, vec![], t0), Err(PutError::Full));
        assert_eq!(store.put(a, vec![], t0 + Duration::from_secs(1)), Ok(()));
    }
}
