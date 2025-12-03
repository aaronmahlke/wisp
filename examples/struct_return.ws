// Test struct return values

import std.io

struct Point {
    x: i32,
    y: i32,
}

fn make_point(x: i32, y: i32) -> Point {
    Point { x: x, y: y }
}

fn main() -> i32 {
    let p = make_point(10, 20)
    print_str("Point:")
    print_i32(p.x)
    print_space()
    print_i32(p.y)
    println()
    p.x + p.y
}

