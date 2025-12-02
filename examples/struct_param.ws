// Test struct parameters

import "../std/io"

struct Rectangle {
    width: i32,
    height: i32,
}

fn area(r: &Rectangle) -> i32 {
    r.width * r.height
}

fn perimeter(r: &Rectangle) -> i32 {
    2 * (r.width + r.height)
}

fn main() -> i32 {
    let rect = Rectangle { width: 10, height: 5 }
    
    print_str("Width:")
    print_i32(rect.width)
    println()
    
    print_str("Height:")
    print_i32(rect.height)
    println()
    
    print_str("Area:")
    print_i32(area(&rect))
    println()
    
    print_str("Perimeter:")
    print_i32(perimeter(&rect))
    println()
    
    0
}
