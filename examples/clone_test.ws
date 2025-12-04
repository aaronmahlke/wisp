import std.string.String
import std.io.{ Display, print }

fn use_string(s: String) {
    print(&s)
}

fn main() {
    let s = String.from("hello world")
    use_string(s.clone())  // Clone it to avoid move
    use_string(s.clone())  // Can still use s because we cloned
    print(&s)              // Still valid
}
