use bitcoind::{self, BitcoinD};

#[allow(dead_code)]
struct OwnerTestHarness {
    bitcoind: BitcoinD,
}

impl OwnerTestHarness {
    async fn init() -> Self {
        let exe_path = bitcoind::downloaded_exe_path()
            .expect("Didn't specify bitcoind version in feature flags");
        let bitcoind =
            BitcoinD::new(exe_path).expect("Failed to init bitcoind");
        Self { bitcoind }
    }
}

#[tokio::test]
async fn init() {
    OwnerTestHarness::init().await;
}
