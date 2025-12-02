import "../std/io"
import "../std/string"

fn main() {
    // Basic for loop with range
    print(&"Count 0-4:");
    for i in 0..5 {
        print(&"  {i}");
    }
    
    // Countdown (using arithmetic)
    print(&"Countdown:");
    for i in 0..5 {
        let n = 4 - i;
        print(&"  {n}");
    }
    
    // Nested loops - print a triangle
    print(&"Triangle:");
    for row in 1..6 {
        let mut line = String.new();
        for _col in 0..row {
            line.push_str("*");
        }
        print(&line);
    }
}
