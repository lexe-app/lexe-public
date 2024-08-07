use common::{
    ed25519, enclave,
    rng::SysRng,
    tls::attestation::{
        self,
        verifier::{EnclavePolicy, SgxQuoteVerifier},
    },
};

fn main() {
    println!("SGX test");

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

    let now = common::tls::rustls::pki_types::UnixTime::now();
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
