import std.io.{ print, Display }
import std.option.{ Option, Some, None }

fn main() {
    let x: Option<i32> = Some(42)
    let y: Option<i32> = None
    
    // Test is_some/is_none
    if x.is_some() {
        print(&"x is Some")
    }
    
    if y.is_none() {
        print(&"y is None")
    }
    
    // Test or
    let val = x.or(0)
    print(&val)
    
    let val2 = y.or(99)
    print(&val2)
}
