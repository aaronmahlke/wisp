import std.ops.Copy
import std.io.{ Display, print }

// Point has all Copy fields (i32, i32) - so it can be Copy
struct Point {
    x: i32,
    y: i32
}

impl Copy for Point {}

impl Display for Point {
    fn to_string(&self) -> String {
        "Point({self.x}, {self.y})"
    }
}

fn use_point(p: Point) {
    print(&p)
}

fn main() {
    let p = Point { x: 10, y: 20 }
    use_point(p)  // First use
    use_point(p)  // Second use - would fail without Copy!
    print(&p)     // Can still use p here
}

