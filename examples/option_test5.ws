import std.option
import std.io.print

fn main() {
    let x: option.Option<i32> = option.Some(42)
    let val = x.or(0)
    print(&val)
}
