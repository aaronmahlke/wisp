import std.option.{ Option, Some, None }
import std.io.print

fn main() {
    let x: Option<i32> = Some(42)
    let y: Option<i32> = None
    print(&42)
}
