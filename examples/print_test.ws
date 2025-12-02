import "../std/io"

fn main() -> i32 {
    print(&"=== Unified Print Test ===");
    println();
    
    // Print different types with the same function
    print(&42);
    println();
    
    print(&true);
    println();
    
    print(&"Hello, World!");
    println();
    
    // Negative numbers
    print(&-123);
    println();
    
    print(&"=== Done! ===");
    println();
    0
}
