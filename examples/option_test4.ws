import std.option.{ Option, Some, None }
import std.io.print

fn main() {
    let x: Option<i32> = Some(42)
    let val = x.or(0)
    print(&val)
}
