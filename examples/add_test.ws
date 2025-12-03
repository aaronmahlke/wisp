import std.io.{ Display, print }
import std.ops.Add

fn add<T: Add>(a: T, b: T) -> T {
    a + b
}

fn main() {
    let result = add(23, 19)
    print(&result)
}
