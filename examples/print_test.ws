import std.io.{ print }

fn main() -> i32 {
    print(&"=== Unified Print Test ===");
    
    // Print different types with the same function
    print(&42);
    print(&true);
    print(&"Hello, World!");
    
    // Negative numbers
    print(&-123);
    
    print(&"=== Done! ===");
    0
}
