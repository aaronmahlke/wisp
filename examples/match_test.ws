import std.option.{ Option, Some, None }
import std.io.print

fn main() {
    let x: Option<i32> = Some(42)
    let y: Option<i32> = None

    // Test direct match on owned enum
    if x.is_some() {
        print(&"x: is Some")
    } else {
        print(&"x: is None")
    }

    // Test is_some on None
    if y.is_some() {
        print(&"y: is Some")
    } else {
        print(&"y: is None")
    }
}

