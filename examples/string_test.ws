import "../std/io"
import "../std/string"

fn main() -> i32 {
    print_line("=== String Test ===");
    
    // Create a String from a str literal
    let s = String.from("Hello, World!");
    
    print("Length: ");
    print_i64(s.len());
    println();
    
    print("Is empty: ");
    if s.is_empty() {
        print("yes");
    } else {
        print("no");
    }
    println();
    
    // Clean up
    s.drop();
    
    print_line("=== Done! ===");
    0
}

