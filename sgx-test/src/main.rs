use common::{ed25519, enclave, rng::SysRng};
use lexe_tls::attestation::{
    self,
    verifier::{EnclavePolicy, SgxQuoteVerifier},
};

const HELP: &str = r#"
sgx-test [OPTIONS] [TEST]

TESTS:
    sgx           Test SGX platform (default)
    panic         Test backtrace on panic
    oom           Test backtrace on OOM

OPTIONS:
    -h, --help    Print help

"#;

fn main() {
    // Disable _non-panic_ `std::backtrace::Backtrace::capture()`.
    //
    // 2025-02-04: In SGX and outside a panic, `Backtrace::capture()` appears to
    // enter an infinite loop, causing the caller to hang indefinitely.
    //
    // See: <https://docs.rs/anyhow/latest/anyhow/struct.Error.html#method.backtrace>
    unsafe { std::env::set_var("RUST_LIB_BACKTRACE", "0") };
    unsafe { std::env::set_var("RUST_BACKTRACE", "full") };

    let args = std::env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        help();
    }

    let command = args.get(1).map(|s| s.as_str()).unwrap_or("sgx");
    match command {
        "sgx" => test_sgx(),
        "panic" => test_panic(),
        "oom" => test_oom(),
        _ => {
            eprintln!("unrecognized command: '{command}'");
            help();
        }
    }
}

fn help() -> ! {
    eprint!("{HELP}");
    std::process::exit(1);
}

fn test_sgx() {
    println!("Ensure SGX platform primitives work (sealing, attestation, etc)");

    println!("machine_id: {}", enclave::machine_id());
    println!("measurement: {}", enclave::measurement());

    println!("\nSEALING");

    let mut rng = SysRng::new();
    let label = b"label".as_slice();
    let data = b"my data".as_slice();
    let sealed = enclave::seal(&mut rng, label, data.into())
        .expect("Failed to seal some data");
    println!(
        "seal('label', 'my data') := {}",
        hex::display(&sealed.serialize())
    );
    let unsealed = enclave::unseal(sealed, label)
        .expect("Failed to unseal some sealed data");
    assert_eq!(&unsealed, data);

    println!("\nREMOTE ATTESTATION");

    let pubkey = ed25519::PublicKey::new([69; 32]);
    println!("fake pubkey we're attesting to: {pubkey}");

    let evidence = attestation::quote::quote_enclave(&mut rng, &pubkey)
        .expect("Failed to produce remote attestation");

    println!("SGX DER-serialized evidence:");
    println!("quote: {}", hex::display(&evidence.quote));

    let now = lexe_tls::rustls::pki_types::UnixTime::now();
    let quote_verifier = SgxQuoteVerifier;
    let report = quote_verifier
        .verify(&evidence.quote, now)
        .expect("Invalid SGX quote");

    println!("SGX enclave Report:");
    println!("measurement: {}", hex::display(&report.mrenclave));
    println!("mrsigner: {}", hex::display(&report.mrsigner));
    println!("reportdata: {}", hex::display(&report.reportdata));
    println!("attributes: {:?}", report.attributes);
    println!("miscselect: {:?}", report.miscselect);
    println!("cpusvn: {}", hex::display(&report.cpusvn));
    println!("isvsvn: {}", report.isvsvn);
    println!("isvsvn: {}", report.isvprodid);

    let enclave_policy = EnclavePolicy::trust_self();
    let reportdata = enclave_policy
        .verify(&report)
        .expect("Quote is for an untrusted enclave");

    let mut expected_reportdata = [0u8; 64];
    expected_reportdata[0..32].copy_from_slice(pubkey.as_slice());

    assert!(
        reportdata.contains(&pubkey),
        "SGX Quote doesn't contain dummy pubkey"
    );
    assert_eq!(
        reportdata.as_inner(),
        &expected_reportdata,
        "SGX Quote contains extraneous data"
    );
}

#[inline(never)]
fn test_panic() {
    println!("This should panic and print a backtrace:");
    // We should get panic+backtrace even with `catch_unwind` -> `resume_unwind`
    // TODO(phlip9): Get `resume_unwind` working with `-C panic=unwind` in SGX.
    // The panic message and backtrace is swallowed somehow.
    let res = std::panic::catch_unwind(|| {
        do_panic();
    });
    match res {
        Ok(()) => unreachable!(),
        Err(err) => {
            eprintln!("caught panic");
            std::panic::resume_unwind(err);
        }
    }
}

#[inline(never)]
fn do_panic() {
    panic!("this is a panic!");
}

fn test_oom() {
    println!("This should OOM and print a backtrace:");
    let mut xs = Vec::new();
    for x in 0usize..1_000_000 {
        xs.push((x & 0xff) as u8);
    }
    println!("{:?}", &xs[123_456..123_477]);

    eprintln!(
        "ERROR: we should have OOM'ed by now! Is the heap_size too large?"
    );
    std::process::exit(1);
}
