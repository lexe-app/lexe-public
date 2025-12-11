uniffi::setup_scaffolding!("lexe");

#[uniffi::export]
fn add(a: u32, b: u32) -> u32 {
    a + b
}
