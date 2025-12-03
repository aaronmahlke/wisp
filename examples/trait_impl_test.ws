import std.io.{ Display, print }
import std.string.String

struct Point {
    x: i32,
    y: i32,
}

// Hover over "Display" should show the trait definition
// Typing "fn " inside this impl block should suggest: fn to_string(&self) -> String
impl Display for Point {
    fn to_string(&self) -> String {
        "Point({self.x}, {self.y})"
    }
}

fn main() {
    let p = Point { x: 10, y: 20 };
    print(&p);
}

