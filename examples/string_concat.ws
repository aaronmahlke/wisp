import std.io
import std/string

extern fn puts(s: i64) -> i32

fn main() {
    let a = String.from("Hello, ");
    let b = String.from("World!");
    let c = String.from("Hi") + b;  // Should desugar to a.add(b)

    // Print the concatenated string
    let _ = puts(c.as_ptr());
}
