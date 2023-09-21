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
}
