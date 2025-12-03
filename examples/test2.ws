import std.io.{print, Display}
import std.string.String

struct SomeStruct {
    value: i32,
}

impl Display for SomeStruct {
    fn to_string(&self) -> String {
        "{self.value}"
    }
}


fn add(a_thing: i32, b_thing: i32) -> i32 {
    a_thing + b_thing
}

fn somefunc() {}


fn main() {
    let something = SomeStruct { value: 3}
    let a = SomeStruct {value: 32 }
    somefunc();
    let x = add(23, 23)
    let b = add(b_thing: 23)
}
