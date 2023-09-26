// use serde::Serialize;
//
// #[derive(Serialize)]
// struct Foo {
//     a: u32,
//     b: String,
// }

pub fn main() {
    println!("hello world!");

    // let foo = Foo {
    //     a: 123,
    //     b: "hello".to_owned(),
    // };
    //
    // let foo_json = serde_json::to_string_pretty(&foo).unwrap();
    //
    // println!("asdf {foo_json}");

    // let foo_hash =
    //     ring::digest::digest(&ring::digest::SHA256, foo_json.as_bytes());
    // println!("foo_hash: {:?}", &foo_hash.as_ref());
    //
    // use ring::rand::SecureRandom;
    // let mut bytes = [0_u8; 32];
    // let rng = ring::rand::SystemRandom::new();
    // rng.fill(&mut bytes[..]).unwrap();
    // println!("random bytes: asdf {bytes:?}");

    let keypair_pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &ring::rand::SystemRandom::new(),
    )
    .unwrap();
    let keypair = ring::signature::EcdsaKeyPair::from_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        keypair_pkcs8.as_ref(),
    )
    .unwrap();

    let msg = b"hello world";
    let sig = keypair
        .sign(&ring::rand::SystemRandom::new(), msg.as_slice())
        .unwrap();

    println!("sig: {:?}", sig.as_ref());
}
