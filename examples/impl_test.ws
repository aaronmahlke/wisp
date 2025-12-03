import std.io

struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn add(&self, other: &Point) -> Point {
        Point { x: self.x + other.x, y: self.y + other.y }
    }
    
    fn scale(&self, factor: i32) -> Point {
        Point { x: self.x * factor, y: self.y * factor }
    }
    
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
    let p1 = Point { x: 3, y: 4 };
    let p2 = Point { x: 1, y: 2 };
    
    // Method calls: expr.method(args)
    print(&"p1 = ");
    p1.print_point();
    println();
    
    print(&"p2 = ");
    p2.print_point();
    println();
    
    let p3 = p1.add(&p2);
    print(&"p1 + p2 = ");
    p3.print_point();
    println();
    
    let p4 = p1.scale(2);
    print(&"p1 * 2 = ");
    p4.print_point();
    println();
    
    0
}
