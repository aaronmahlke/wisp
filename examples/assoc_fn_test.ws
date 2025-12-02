import "../std/io"

struct Point {
    x: i32,
    y: i32,
}

impl Point {
    // Associated function (no self) - like a static method
    fn new(x: i32, y: i32) -> Point {
        Point { x: x, y: y }
    }

    // Method (has self)
    fn print_point(&self) {
        let x = self.x;
        let y = self.y;
        print(&"(");
        print(&x);
        print(&", ");
        print(&y);
        print(&")");
    }
}

fn main() -> i32 {
    // Call associated function
    let p = Point.new(x: 10, y: 20);

    print(&"p = ");
    p.print_point();
    println();

    0
}
