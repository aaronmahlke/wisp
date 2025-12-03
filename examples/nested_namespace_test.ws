// Test nested namespaces: import std -> std.io.print
import std

fn main() {
    std.io.print(&"Hello from nested namespace!");
    
    // Also test creating a String via nested namespace
    let s = std.string.String.from("Nested works!");
    std.io.print(&s);
}

