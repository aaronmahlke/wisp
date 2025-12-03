// Test new import syntax

// Namespace import - io.print(), io.Display
import std.io

// Also import string module
import std/string

fn main() {
    // Use namespaced access
    io.print(&"Hello from namespace import!")
    
    // Create a String using namespaced access
    let s = string.String.from("Hello, World!")
    io.print(&s)
}

