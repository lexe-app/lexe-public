use ring::rand::SecureRandom;
use serde::Serialize;

#[derive(Serialize)]
struct Foo {
    a: u32,
    b: String,
}

pub fn main() {
    println!("hello world!");

    let foo = Foo {
        a: 123,
        b: "hello".to_owned(),
    };

    let foo_json = serde_json::to_string_pretty(&foo).unwrap();

    println!("asdf {foo_json}");

    let foo_hash =
        ring::digest::digest(&ring::digest::SHA256, foo_json.as_bytes());
    println!("foo_hash: {:?}", &foo_hash.as_ref());

    let mut bytes = [0_u8; 32];
    let rng = ring::rand::SystemRandom::new();
    rng.fill(&mut bytes[..]).unwrap();
    println!("random bytes: asdf {bytes:?}");
}
