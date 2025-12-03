import std.io.{ Display, print }
import std.ops.Add
import std.string.String

struct Point {
    x: i32,
    y: i32
}

impl Add for Point {
    fn add(self, rhs: Self) -> Self {
        Point {x: self.x + rhs.x, y: self.y + rhs.y}
    }
}

impl Display for Point {
    fn to_string(&self) -> String {
        "Point: ({self.x}, {self.y})"
    }
}

fn add<T: Add>(a: T, b: T) -> T {
    a + b
}

fn main() {
    let a_point = Point { x: 3, y: 8 }
    let b_point = Point { x: 5, y: 7 }
    let point = add(a_point, b_point)
    print(&point)
}
