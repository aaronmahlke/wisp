import std.io

fn add(x: i32, y: i32) -> i32 {
    x + y
}

fn subtract(x: i32, y: i32) -> i32 {
    x - y
}

fn greet(name: i32, age: i32) {
    print(&"Name: ");
    print(&name);
    print(&", Age: ");
    print(&age);
    println();
}

fn print_line(s: str) {
    print(&s);
    println();
}

fn main() -> i32 {
    print_line("=== Named Arguments Test ===");
    
    // Positional call
    let result1 = add(10, 20);
    print(&"Positional add(10, 20) = ");
    print(&result1);
    println();
    
    // Named call
    let result2 = add(x: 10, y: 20);
    print(&"Named add(x: 10, y: 20) = ");
    print(&result2);
    println();
    
    // Named call with different order - should still be 30 if reordering works
    let result3 = add(y: 5, x: 100);
    print(&"Named add(y: 5, x: 100) = ");
    print(&result3);
    print(&" (should be 105)");
    println();
    
    // Test with subtraction to verify reordering
    // subtract(x: 100, y: 5) should be 100 - 5 = 95
    let result4 = subtract(x: 100, y: 5);
    print(&"subtract(x: 100, y: 5) = ");
    print(&result4);
    print(&" (should be 95)");
    println();
    
    // subtract(y: 5, x: 100) should ALSO be 100 - 5 = 95 if reordering works
    // If NOT reordering, it would be 5 - 100 = -95
    let result5 = subtract(y: 5, x: 100);
    print(&"subtract(y: 5, x: 100) = ");
    print(&result5);
    print(&" (should be 95 if reordering works, -95 if not)");
    println();
    
    // Greet with named args
    greet(name: 42, age: 25);
    
    print_line("=== Done! ===");
    0
}
