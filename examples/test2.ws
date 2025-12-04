import std.string.String
import std.ops.Add
import std.io.{ Display, print }
import std.option.Option

struct Point {
    x: i32,
    y: i32
}

impl Point {
    fn rotate(self) -> Point {
        self * 2
    }
}

impl Mul<i32> for Point {
    fn mul(self, rhs: i32) -> Self {
        Point {x: self.x * rhs, y: self.y * rhs }
    }
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

fn add<T>(a: T, b: T) -> Option<T> {
    Some(a + b)
}


fn main() {
    let a_point = Point { x: 3, y: 8 }
    let b_point = Point { x: 5, y: 7 }
    let point = add(a_point, b_point);

    print(&point.or(b_point))

    print(&point.or(a_point))
}
