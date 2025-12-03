// Test multiple item imports using .{ } syntax
import std.io.{ print, Display }
import std.string.String

fn main() {
    let s = String.from("World");
    print(&"Hello, ");
    s.print();
}

