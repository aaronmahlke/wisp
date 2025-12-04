import std.io.print

fn main() {
    // Test escape sequences
    print(&"He said \"hello\"")
    print(&"Line 1\nLine 2")
    print(&"Tab:\there")
    print(&"Backslash: \\")
    
    // Test in interpolation
    let name = "world"
    print(&"Hello \"{name}\"!")
}

