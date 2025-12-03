import std.io.{ print, Display }
import std.string.String
import std.ops.PartialEq

struct Point {
    x: i32,
    y: i32
}

impl PartialEq for Point {
    fn eq(&self, rhs: &Self) -> bool {
        self.x == rhs.x && self.y == rhs.y
    }
}

impl Display for Point {
    fn to_string(&self) -> String {
        "Point({self.x}, {self.y})"
    }
}

fn main() {
    let a = Point { x: 3, y: 5 }
    let b = Point { x: 3, y: 5 }
    let c = Point { x: 1, y: 2 }
    
    // Test == with PartialEq trait
    if a == b {
        print(&"a == b: true")
    }
    
    if a == c {
        print(&"a == c: true")
    } else {
        print(&"a == c: false")
    }
    
    // Test != (should use negated PartialEq::eq)
    if a != c {
        print(&"a != c: true")
    }
}
