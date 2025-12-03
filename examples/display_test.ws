import std.io.{ print, Display }
import std.string.String

struct MyStruct {
    value: i32,
}

impl Display for MyStruct {
    fn to_string(&self) -> String {
        String.from("MyStruct")
    }
}

fn main() {
    let s = MyStruct { value: 42 };
    print(&s);
}

