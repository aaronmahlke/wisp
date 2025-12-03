import std.ops.Add
// Test file for auto-import feature
// This should show an error for undefined trait 'Add'
// and offer a code action to import std.ops.Add

struct Point {
    x: i32,
    y: i32,
}

// Hovering 'Add' should show the trait definition
// and offer "Import std.ops.Add" as a code action
impl Add for Point {
    fn add(self, rhs: Self) -> Self {
        Point {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

fn main() {
    let p1 = Point { x: 1, y: 2 };
    let p2 = Point { x: 3, y: 4 };
    // This should work after importing Add
    let p3 = p1 + p2;
}

