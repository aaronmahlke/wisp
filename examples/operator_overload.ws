import std.io
import std/ops

struct Point {
    x: i32,
    y: i32,
}

impl Add for Point {
    fn add(self, rhs: Point) -> Point {
        Point { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

fn main() {
    let a = Point { x: 1, y: 2 };
    let b = Point { x: 3, y: 4 };
    let c = a + b;  // Should desugar to a.add(b)
    
    print(&"c.x = ");
    print(&c.x);
    println();
    print(&"c.y = ");
    print(&c.y);
    println();
}

