import std.io.{ print, Display }
import std.string.String
import std.ops.Add

struct Counter {
    value: i32
}

impl Add for Counter {
    fn add(self, rhs: Self) -> Self {
        Counter { value: self.value + rhs.value }
    }
}

impl Display for Counter {
    fn to_string(&self) -> String {
        "Counter({self.value})"
    }
}

fn main() {
    // Test with primitives
    let mut x = 10
    x += 5
    print(&x)  // Should print 15
    
    x -= 3
    print(&x)  // Should print 12
    
    x *= 2
    print(&x)  // Should print 24
    
    x /= 4
    print(&x)  // Should print 6
    
    // Test with custom type
    let mut c = Counter { value: 10 }
    c += Counter { value: 5 }
    print(&c)  // Should print Counter(15)
}
