// More comprehensive struct test

import "../std/io"

struct Point {
    x: i32,
    y: i32,
}

struct Rectangle {
    width: i32,
    height: i32,
}

fn main() -> i32 {
    let p1 = Point { x: 5, y: 10 }
    let p2 = Point { x: 15, y: 20 }
    
    print_str("Point 1:")
    print_i32(p1.x)
    print_space()
    print_i32(p1.y)
    println()
    
    print_str("Point 2:")
    print_i32(p2.x)
    print_space()
    print_i32(p2.y)
    println()
    
    let rect = Rectangle { width: 10, height: 5 }
    print_str("Rectangle dimensions:")
    print_i32(rect.width)
    print_str(" x ")
    print_i32(rect.height)
    println()
    
    // Compute area inline
    print_str("Area:")
    print_i32(rect.width * rect.height)
    println()
    
    0
}
