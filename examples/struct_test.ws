// Test struct support

import std.io

struct Point {
    x: i32,
    y: i32,
}

fn main() -> i32 {
    let p = Point { x: 10, y: 20 }
    print_i32(p.x)
    println()
    print_i32(p.y)
    println()
    p.x + p.y
}

